//! Pulling a remote source into its local snapshot (SPEC §7.7: explicit network use only).
//! A pull is the only network step: it writes `<project>/sources/<kind>/<id>.csv` plus a
//! `.json` record (origin, time, rows, SHA-256) beside it, and imports then read that file
//! offline. Callers check the project's network switch; [`pull`] refuses in offline mode.
//!
//! - Google Sheets: the sheet must be shared as "anyone with the link"; the CSV export is
//!   fetched without signing in.
//! - Notion: a database queried with an internal integration token (Notion API 2022-06-28),
//!   one row per page with its properties as columns, plus the page's text as `body`.
//! - Zotero: a library or collection through the Web API v3 with an API key ([`zotero`]).
//! - Airtable: a table (optionally a view) with a personal access token ([`airtable`]).
//! - Typeform and Tally: a form's completed responses, one row per response ([`typeform`],
//!   [`tally`]); Canny: feature-request posts with status and votes ([`canny`]); Productboard:
//!   customer notes or features ([`productboard`]).

mod airtable;
mod canny;
mod productboard;
mod tally;
mod typeform;
mod zotero;

use crate::source::Source;
use anyhow::{Context, Result, bail};
use invectus_core::CancelToken;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::path::Path;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USER_AGENT: &str = concat!("inVectus/", env!("CARGO_PKG_VERSION"));
/// Largest CSV export accepted from Google Sheets.
const MAX_SHEET_BYTES: u64 = 200 * 1024 * 1024;
const NOTION_API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2022-06-28";
/// Notion allows about three requests a second per integration.
const NOTION_SPACING: Duration = Duration::from_millis(350);
/// Text blocks read per page body, and how deep nested blocks (toggles, lists) are followed.
const MAX_BODY_BLOCKS: usize = 1_000;
const MAX_BODY_DEPTH: usize = 3;

#[derive(Clone, Debug, Serialize)]
pub struct Pulled {
    pub origin: String,
    pub snapshot: String,
    pub rows: usize,
    pub bytes: u64,
    pub sha256: String,
    pub pulled_at: u64,
}

/// What a pull may use besides the network: the integration's token
/// ([`Source::integration`]), progress and cancellation.
pub struct PullContext<'a> {
    pub token: Option<String>,
    pub cancel: &'a CancelToken,
    /// `(phase, done, total)`; `total` is 0 when unknown.
    pub progress: &'a mut dyn FnMut(&str, usize, usize),
}

/// Fetch `source` and replace its snapshot. Local sources are refused.
pub fn pull(source: &Source, cx: &mut PullContext) -> Result<Pulled> {
    invectus_core::net::ensure_online(&format!("pulling {}", source.describe()))?;
    let (csv, rows) = match source {
        Source::GoogleSheet {
            spreadsheet_id,
            gid,
            ..
        } => {
            (cx.progress)("downloading", 0, 1);
            let csv = google_sheet_csv(spreadsheet_id, gid.as_deref())?;
            let rows = csv_rows(&csv);
            (csv, rows)
        }
        Source::Notion { database_id, .. } => {
            let token = need_token(cx, "Notion", "NOTION")?;
            let table = notion_database(database_id, &token, cx)?;
            let rows = table.1.len();
            (write_csv(&table.0, &table.1), rows)
        }
        Source::Zotero {
            library,
            collection,
            ..
        } => {
            let token = need_token(cx, "Zotero", "ZOTERO")?;
            let table = zotero::items(library, collection.as_deref(), &token, cx)?;
            let rows = table.1.len();
            (write_csv(&table.0, &table.1), rows)
        }
        Source::Airtable {
            base_id,
            table,
            view,
            ..
        } => {
            let token = need_token(cx, "Airtable", "AIRTABLE")?;
            let t = airtable::records(base_id, table, view.as_deref(), &token, cx)?;
            let rows = t.1.len();
            (write_csv(&t.0, &t.1), rows)
        }
        Source::Typeform { form_id, .. } => {
            let token = need_token(cx, "Typeform", "TYPEFORM")?;
            let t = typeform::responses(form_id, &token, cx)?;
            let rows = t.1.len();
            (write_csv(&t.0, &t.1), rows)
        }
        Source::Tally { form_id, .. } => {
            let token = need_token(cx, "Tally", "TALLY")?;
            let t = tally::submissions(form_id, &token, cx)?;
            let rows = t.1.len();
            (write_csv(&t.0, &t.1), rows)
        }
        Source::Productboard { entity, .. } => {
            let token = need_token(cx, "Productboard", "PRODUCTBOARD")?;
            let t = productboard::entities(entity, &token, cx)?;
            let rows = t.1.len();
            (write_csv(&t.0, &t.1), rows)
        }
        Source::Canny { board, .. } => {
            let token = need_token(cx, "Canny", "CANNY")?;
            let t = canny::posts(board.as_deref(), &token, cx)?;
            let rows = t.1.len();
            (write_csv(&t.0, &t.1), rows)
        }
        other => bail!("{} is a local source; nothing to pull", other.describe()),
    };
    let snapshot = source.path();
    write_snapshot(snapshot, csv.as_bytes())?;
    let pulled = Pulled {
        origin: source.describe(),
        snapshot: snapshot.display().to_string(),
        rows,
        bytes: csv.len() as u64,
        sha256: hex::encode(Sha256::digest(csv.as_bytes())),
        pulled_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs()),
    };
    std::fs::write(
        snapshot.with_extension("json"),
        serde_json::to_vec_pretty(&json!({ "source": source, "pull": &pulled }))?,
    )?;
    Ok(pulled)
}

fn need_token(cx: &PullContext, name: &str, env: &str) -> Result<String> {
    cx.token.clone().with_context(|| {
        format!(
            "{name} is not connected: add its token in Integrations (or set \
             INVECTUS_{env}_TOKEN)"
        )
    })
}

/// GET requests to one API, spaced and retried on rate limits (429) and server errors.
pub(crate) struct Paced {
    agent: ureq::Agent,
    spacing: Duration,
    last: Option<Instant>,
}

impl Paced {
    pub(crate) fn new(spacing: Duration) -> Self {
        Self {
            agent: agent(),
            spacing,
            last: None,
        }
    }

    /// `(status, JSON body)`; the caller turns statuses other than 200 into errors.
    pub(crate) fn get(&mut self, url: &str, headers: &[(&str, &str)]) -> Result<(u16, Value)> {
        self.request(url, headers, None)
    }

    /// POST a JSON body (Canny puts its key in the body); same pacing and retries as GET.
    pub(crate) fn post_json(&mut self, url: &str, body: &Value) -> Result<(u16, Value)> {
        self.request(url, &[], Some(body))
    }

    fn request(
        &mut self,
        url: &str,
        headers: &[(&str, &str)],
        body: Option<&Value>,
    ) -> Result<(u16, Value)> {
        let method = if body.is_some() { "POST" } else { "GET" };
        for attempt in 0..5u32 {
            if let Some(t) = self.last {
                std::thread::sleep(self.spacing.saturating_sub(t.elapsed()));
            }
            self.last = Some(Instant::now());
            let resp = match body {
                Some(b) => {
                    let mut req = self
                        .agent
                        .post(url)
                        .header("User-Agent", USER_AGENT)
                        .header("Content-Type", "application/json");
                    for (k, v) in headers {
                        req = req.header(*k, *v);
                    }
                    req.send(serde_json::to_string(b)?)
                }
                None => {
                    let mut req = self.agent.get(url).header("User-Agent", USER_AGENT);
                    for (k, v) in headers {
                        req = req.header(*k, *v);
                    }
                    req.call()
                }
            }
            .with_context(|| format!("{method} {url}"))?;
            let status = resp.status().as_u16();
            let wait = ["retry-after", "backoff"].iter().find_map(|h| {
                resp.headers()
                    .get(*h)
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.trim().parse::<u64>().ok())
            });
            if status == 429 || status >= 500 {
                std::thread::sleep(Duration::from_secs(wait.unwrap_or(1 << attempt).min(60)));
                continue;
            }
            let text = resp
                .into_body()
                .read_to_string()
                .context("reading the answer")?;
            let v = if text.trim().is_empty() {
                Value::Null
            } else {
                serde_json::from_str(&text).unwrap_or(Value::String(text))
            };
            if let Some(w) = wait {
                // Zotero's Backoff: slow down before the next request.
                std::thread::sleep(Duration::from_secs(w.min(60)));
            }
            return Ok((status, v));
        }
        bail!("the service kept refusing requests (rate limit); try again in a minute")
    }
}

/// The record of the last pull of `source`, if any.
pub fn last_pull(source: &Source) -> Option<Value> {
    let bytes = std::fs::read(source.path().with_extension("json")).ok()?;
    serde_json::from_slice::<Value>(&bytes)
        .ok()?
        .get("pull")
        .cloned()
}

fn write_snapshot(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path.parent().context("snapshot path has no folder")?;
    std::fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    let tmp = path.with_extension("csv.part");
    std::fs::write(&tmp, bytes).with_context(|| format!("writing {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("writing {}", path.display()))
}

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .timeout_global(Some(Duration::from_secs(120)))
        .build()
        .into()
}

// ---- Google Sheets -------------------------------------------------------------------------

fn google_sheet_csv(id: &str, gid: Option<&str>) -> Result<String> {
    let mut url = format!("https://docs.google.com/spreadsheets/d/{id}/export?format=csv");
    if let Some(g) = gid {
        url.push_str(&format!("&gid={g}"));
    }
    let resp = agent()
        .get(&url)
        .header("User-Agent", USER_AGENT)
        .call()
        .with_context(|| format!("GET {url}"))?;
    let status = resp.status().as_u16();
    let html = resp
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|t| t.contains("text/html"));
    if matches!(status, 401 | 403) || (status == 200 && html) {
        bail!(
            "Google did not share this sheet: set its sharing to \"Anyone with the link\" (viewer), \
             or download it as .xlsx or .csv and import the file"
        );
    }
    if status == 404 {
        bail!("Google Sheets has no sheet at this link (check the link and the tab)");
    }
    if status != 200 {
        bail!("Google Sheets answered HTTP {status}");
    }
    let bytes = resp
        .into_body()
        .with_config()
        .limit(MAX_SHEET_BYTES)
        .read_to_vec()
        .context("reading the sheet")?;
    let text = String::from_utf8(bytes).context("the sheet export is not UTF-8 text")?;
    if text.trim().is_empty() {
        bail!("the sheet is empty");
    }
    Ok(text.trim_start_matches('\u{feff}').to_string())
}

/// Data rows in a CSV text (records after the header), honouring quoted newlines.
fn csv_rows(csv: &str) -> usize {
    let (mut quoted, mut records, mut any) = (false, 0usize, false);
    for c in csv.chars() {
        match c {
            '"' => {
                quoted = !quoted;
                any = true;
            }
            '\n' if !quoted => {
                if any {
                    records += 1;
                }
                any = false;
            }
            '\r' => {}
            _ => any = true,
        }
    }
    if any {
        records += 1;
    }
    records.saturating_sub(1)
}

// ---- Notion --------------------------------------------------------------------------------

struct Notion<'a> {
    agent: ureq::Agent,
    token: &'a str,
    last: Option<Instant>,
}

impl Notion<'_> {
    fn call(&mut self, method: &str, path: &str, body: Option<&Value>) -> Result<Value> {
        let url = format!("{NOTION_API}{path}");
        for attempt in 0..5 {
            if let Some(t) = self.last {
                std::thread::sleep(NOTION_SPACING.saturating_sub(t.elapsed()));
            }
            self.last = Some(Instant::now());
            let auth = format!("Bearer {}", self.token);
            let resp = if method == "POST" {
                self.agent
                    .post(&url)
                    .header("Authorization", &auth)
                    .header("Notion-Version", NOTION_VERSION)
                    .header("Content-Type", "application/json")
                    .header("User-Agent", USER_AGENT)
                    .send(serde_json::to_string(body.unwrap_or(&json!({})))?)
            } else {
                self.agent
                    .get(&url)
                    .header("Authorization", &auth)
                    .header("Notion-Version", NOTION_VERSION)
                    .header("User-Agent", USER_AGENT)
                    .call()
            }
            .with_context(|| format!("{method} {url}"))?;
            let status = resp.status().as_u16();
            if status == 429 || status >= 500 {
                let wait = resp
                    .headers()
                    .get("retry-after")
                    .and_then(|v| v.to_str().ok())
                    .and_then(|v| v.parse::<u64>().ok())
                    .unwrap_or(1 << attempt);
                std::thread::sleep(Duration::from_secs(wait.min(30)));
                continue;
            }
            let text = resp
                .into_body()
                .read_to_string()
                .context("reading Notion's answer")?;
            let v: Value = serde_json::from_str(&text).context("Notion returned invalid JSON")?;
            if status != 200 {
                let msg = v["message"].as_str().unwrap_or("no message");
                bail!(match status {
                    401 => "Notion refused the token: check it in Integrations".to_string(),
                    404 => format!(
                        "Notion could not find this database. Open it in Notion, choose ••• → \
                         Connections, and add your integration ({msg})"
                    ),
                    _ => format!("Notion answered HTTP {status}: {msg}"),
                });
            }
            return Ok(v);
        }
        bail!("Notion kept refusing requests (rate limit); try again in a minute")
    }
}

/// Columns and rows for a database: `page_id`, `url`, `created_time`, `last_edited_time`, each
/// property (title first, then by name), then `body`.
fn notion_database(
    id: &str,
    token: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut n = Notion {
        agent: agent(),
        token,
        last: None,
    };
    (cx.progress)("reading database", 0, 0);
    let db = n.call("GET", &format!("/databases/{id}"), None)?;
    let schema = db["properties"]
        .as_object()
        .context("Notion returned a database without properties")?;
    let mut props: Vec<(&String, &str)> = schema
        .iter()
        .map(|(k, v)| (k, v["type"].as_str().unwrap_or("")))
        .collect();
    props.sort_by_key(|(k, t)| (*t != "title", k.to_lowercase()));
    let fixed = ["page_id", "url", "created_time", "last_edited_time"];
    let mut columns: Vec<String> = fixed.iter().map(|s| s.to_string()).collect();
    for (k, _) in &props {
        // A property named like a fixed column keeps its data under a distinct name.
        let name = if fixed.contains(&k.as_str()) || k.as_str() == "body" {
            format!("property_{k}")
        } else {
            (*k).clone()
        };
        columns.push(name);
    }
    columns.push("body".into());

    let mut pages = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let mut body = json!({ "page_size": 100 });
        if let Some(c) = &cursor {
            body["start_cursor"] = json!(c);
        }
        let v = n.call("POST", &format!("/databases/{id}/query"), Some(&body))?;
        pages.extend(v["results"].as_array().cloned().unwrap_or_default());
        (cx.progress)("listing pages", pages.len(), 0);
        match v["next_cursor"].as_str() {
            Some(c) if v["has_more"].as_bool() == Some(true) => cursor = Some(c.to_string()),
            _ => break,
        }
    }

    let total = pages.len();
    let mut rows = Vec::with_capacity(total);
    for (i, page) in pages.iter().enumerate() {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let page_id = page["id"].as_str().unwrap_or_default().to_string();
        let mut row = vec![
            page_id.clone(),
            page["url"].as_str().unwrap_or_default().to_string(),
            page["created_time"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            page["last_edited_time"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
        ];
        for (k, _) in &props {
            row.push(property_text(&page["properties"][k.as_str()]));
        }
        let mut lines = Vec::new();
        if !page_id.is_empty() {
            page_text(&mut n, &page_id, 0, &mut lines)?;
        }
        row.push(lines.join("\n"));
        rows.push(row);
        (cx.progress)("reading pages", i + 1, total);
    }
    Ok((columns, rows))
}

fn plain(rich: &Value) -> String {
    rich.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t["plain_text"].as_str())
                .collect::<String>()
        })
        .unwrap_or_default()
}

/// A page property as text; lists are joined with ", ".
fn property_text(p: &Value) -> String {
    let t = p["type"].as_str().unwrap_or("");
    let v = &p[t];
    let names = |arr: &Value, key: &str| -> String {
        arr.as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| x[key].as_str().map(String::from))
                    .collect::<Vec<_>>()
                    .join(", ")
            })
            .unwrap_or_default()
    };
    match t {
        "title" | "rich_text" => plain(v),
        "number" => v.as_f64().map(fmt_number).unwrap_or_default(),
        "select" | "status" => v["name"].as_str().unwrap_or_default().to_string(),
        "multi_select" => names(v, "name"),
        "people" => names(v, "name"),
        "relation" => names(v, "id"),
        "date" => match (v["start"].as_str(), v["end"].as_str()) {
            (Some(s), Some(e)) => format!("{s}/{e}"),
            (Some(s), None) => s.to_string(),
            _ => String::new(),
        },
        "checkbox" => v.as_bool().map(|b| b.to_string()).unwrap_or_default(),
        "url" | "email" | "phone_number" | "created_time" | "last_edited_time" => {
            v.as_str().unwrap_or_default().to_string()
        }
        "created_by" | "last_edited_by" => v["name"].as_str().unwrap_or_default().to_string(),
        "files" => names(v, "name"),
        "unique_id" => match (v["prefix"].as_str(), v["number"].as_i64()) {
            (Some(p), Some(n)) => format!("{p}-{n}"),
            (None, Some(n)) => n.to_string(),
            _ => String::new(),
        },
        "formula" => {
            let ft = v["type"].as_str().unwrap_or("");
            match &v[ft] {
                Value::String(s) => s.clone(),
                Value::Number(n) => n.as_f64().map(fmt_number).unwrap_or_default(),
                Value::Bool(b) => b.to_string(),
                Value::Object(d) => d.get("start").and_then(Value::as_str).unwrap_or("").into(),
                _ => String::new(),
            }
        }
        "rollup" => {
            let rt = v["type"].as_str().unwrap_or("");
            match rt {
                "number" => v["number"].as_f64().map(fmt_number).unwrap_or_default(),
                "array" => v["array"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .map(property_text)
                            .filter(|s| !s.is_empty())
                            .collect::<Vec<_>>()
                            .join(", ")
                    })
                    .unwrap_or_default(),
                _ => String::new(),
            }
        }
        _ => String::new(),
    }
}

pub(crate) fn fmt_number(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e15 {
        format!("{}", f as i64)
    } else {
        f.to_string()
    }
}

/// Text of a page's blocks, one line per block, following nested blocks a few levels down.
/// Child pages and databases are not entered (they are their own items).
fn page_text(n: &mut Notion, block: &str, depth: usize, out: &mut Vec<String>) -> Result<()> {
    let mut cursor: Option<String> = None;
    loop {
        if out.len() >= MAX_BODY_BLOCKS {
            return Ok(());
        }
        let mut path = format!("/blocks/{block}/children?page_size=100");
        if let Some(c) = &cursor {
            path.push_str(&format!("&start_cursor={c}"));
        }
        let v = n.call("GET", &path, None)?;
        for b in v["results"].as_array().into_iter().flatten() {
            let t = b["type"].as_str().unwrap_or("");
            let text = plain(&b[t]["rich_text"]);
            let line = match t {
                "bulleted_list_item" | "numbered_list_item" => format!("- {text}"),
                "to_do" => {
                    let done = b[t]["checked"].as_bool() == Some(true);
                    format!("[{}] {text}", if done { "x" } else { " " })
                }
                _ => text,
            };
            if !line.trim().is_empty() {
                out.push(line);
            }
            let nested = !matches!(t, "child_page" | "child_database");
            if nested
                && b["has_children"].as_bool() == Some(true)
                && depth + 1 < MAX_BODY_DEPTH
                && let Some(id) = b["id"].as_str()
            {
                page_text(n, id, depth + 1, out)?;
            }
        }
        match v["next_cursor"].as_str() {
            Some(c) if v["has_more"].as_bool() == Some(true) => cursor = Some(c.to_string()),
            _ => return Ok(()),
        }
    }
}

// ---- CSV -----------------------------------------------------------------------------------

pub(crate) fn write_csv(columns: &[String], rows: &[Vec<String>]) -> String {
    fn field(out: &mut String, s: &str) {
        if s.contains([',', '"', '\n', '\r']) {
            out.push('"');
            out.push_str(&s.replace('"', "\"\""));
            out.push('"');
        } else {
            out.push_str(s);
        }
    }
    let mut out = String::new();
    for (i, c) in columns.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        field(&mut out, c);
    }
    out.push('\n');
    for r in rows {
        for (i, c) in r.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            field(&mut out, c);
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source;

    #[test]
    fn notion_properties_flatten_to_text() {
        let p = |v: Value| property_text(&v);
        assert_eq!(
            p(json!({"type":"title","title":[{"plain_text":"Hello "},{"plain_text":"world"}]})),
            "Hello world"
        );
        assert_eq!(p(json!({"type":"number","number":3.0})), "3");
        assert_eq!(p(json!({"type":"number","number":2.5})), "2.5");
        assert_eq!(
            p(json!({"type":"multi_select","multi_select":[{"name":"a"},{"name":"b"}]})),
            "a, b"
        );
        assert_eq!(
            p(json!({"type":"date","date":{"start":"2025-01-02","end":null}})),
            "2025-01-02"
        );
        assert_eq!(p(json!({"type":"checkbox","checkbox":true})), "true");
        assert_eq!(p(json!({"type":"status","status":{"name":"Done"}})), "Done");
        assert_eq!(
            p(json!({"type":"formula","formula":{"type":"number","number":7}})),
            "7"
        );
        assert_eq!(
            p(json!({"type":"unique_id","unique_id":{"prefix":"BUG","number":12}})),
            "BUG-12"
        );
        assert_eq!(p(json!({"type":"select","select":null})), "");
    }

    #[test]
    fn written_csv_reads_back_through_the_snapshot_reader() {
        let dir = tempfile::tempdir().unwrap();
        let s = Source::parse_remote("notion:0123456789abcdef0123456789abcdef", dir.path())
            .unwrap()
            .unwrap();
        let cols = vec!["page_id".to_string(), "Name".into(), "body".into()];
        let rows = vec![
            vec![
                "p1".to_string(),
                "A, \"quoted\"".into(),
                "line one\nline two".into(),
            ],
            vec!["p2".to_string(), "007".into(), String::new()],
        ];
        let csv = write_csv(&cols, &rows);
        assert_eq!(csv_rows(&csv), 2);
        write_snapshot(s.path(), csv.as_bytes()).unwrap();
        let t = source::read(&s, None).unwrap();
        assert_eq!(t.columns, cols);
        assert_eq!(t.rows[0][1].as_deref(), Some("A, \"quoted\""));
        assert_eq!(t.rows[0][2].as_deref(), Some("line one\nline two"));
        assert_eq!(t.rows[1][1].as_deref(), Some("007"), "ids stay text");
    }

    #[test]
    fn pulls_are_refused_offline_and_for_local_sources() {
        let dir = tempfile::tempdir().unwrap();
        let cancel = CancelToken::default();
        let mut tick = |_: &str, _: usize, _: usize| {};
        let mut cx = PullContext {
            token: None,
            cancel: &cancel,
            progress: &mut tick,
        };
        let local = Source::Csv {
            path: dir.path().join("x.csv"),
        };
        assert!(
            pull(&local, &mut cx)
                .unwrap_err()
                .to_string()
                .contains("local source")
        );
        let notion = Source::parse_remote("notion:0123456789abcdef0123456789abcdef", dir.path())
            .unwrap()
            .unwrap();
        let e = pull(&notion, &mut cx).unwrap_err().to_string();
        assert!(e.contains("not connected") || e.contains("offline"), "{e}");
    }
}
