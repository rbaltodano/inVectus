//! Outside sources for verification (SPEC §6.6): PubMed (E-utilities), OpenAlex, Semantic
//! Scholar (Graph API, no key) and arXiv (Atom API; preprints, no review filter). Requests are
//! explicit, rate-limited, retried with backoff, refused offline, and cached in the project
//! (`cache/verify/`, 7 days), so repeating a verification needs no network.

use anyhow::{Context, Result, bail};
use invectus_core::{Project, project_files::ProjectFiles};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USER_AGENT: &str = concat!(
    "inVectus/",
    env!("CARGO_PKG_VERSION"),
    " (local research tool)"
);
const CACHE_DAYS: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    PubMed,
    OpenAlex,
    SemanticScholar,
    Arxiv,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::PubMed => "pubmed",
            Self::OpenAlex => "openalex",
            Self::SemanticScholar => "semanticscholar",
            Self::Arxiv => "arxiv",
        }
    }
    pub const ALL: [Source; 4] = [
        Self::PubMed,
        Self::OpenAlex,
        Self::SemanticScholar,
        Self::Arxiv,
    ];
    /// The query exactly as this source receives it.
    pub fn query(self, q: &KeywordQuery) -> String {
        match self {
            Self::PubMed => q.pubmed(),
            Self::OpenAlex => q.openalex(),
            Self::SemanticScholar => q.semantic_scholar(),
            Self::Arxiv => q.arxiv(),
        }
    }
    /// Minimum time between requests (PubMed allows 3 per second without a key).
    fn spacing(self) -> Duration {
        match self {
            Self::PubMed => Duration::from_millis(400),
            Self::OpenAlex => Duration::from_millis(150),
            // The unauthenticated pool is shared; one request a second avoids most 429s.
            Self::SemanticScholar => Duration::from_millis(1100),
            // arXiv asks for one request every three seconds.
            Self::Arxiv => Duration::from_millis(3000),
        }
    }
}

impl std::str::FromStr for Source {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "pubmed" => Ok(Self::PubMed),
            "openalex" => Ok(Self::OpenAlex),
            "semanticscholar" | "semantic-scholar" | "s2" => Ok(Self::SemanticScholar),
            "arxiv" => Ok(Self::Arxiv),
            other => bail!("unknown source {other:?} (pubmed, openalex, semanticscholar, arxiv)"),
        }
    }
}

/// One item an outside source returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExternalHit {
    pub source: String,
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    pub url: Option<String>,
    /// Publication types (PubMed) or work type (OpenAlex).
    pub kinds: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceResult {
    pub source: String,
    /// The query exactly as sent.
    pub query: String,
    /// Total matches the source reported (only the first `hits` were fetched).
    pub total: Option<u64>,
    pub hits: Vec<ExternalHit>,
    pub cached: bool,
    pub fetched_at: u64,
}

/// Keyword search: OR within each group, AND between groups (SPEC §6.6: keyword search plus a
/// human read beats similarity alone).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeywordQuery {
    pub groups: Vec<Vec<String>>,
    /// Only reviews (coverage gaps ask whether a review exists).
    pub reviews_only: bool,
}

fn quote(term: &str) -> String {
    let t = term.replace('"', "");
    if t.contains([' ', '-']) {
        format!("\"{t}\"")
    } else {
        t
    }
}

impl KeywordQuery {
    fn boolean(&self, field: &str) -> String {
        self.groups
            .iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let terms: Vec<String> = g.iter().map(|t| format!("{}{field}", quote(t))).collect();
                format!("({})", terms.join(" OR "))
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// PubMed syntax: terms in title/abstract; reviews by publication type.
    pub fn pubmed(&self) -> String {
        let q = self.boolean("[tiab]");
        if self.reviews_only {
            format!("{q} AND (review[pt] OR systematic review[pt] OR meta-analysis[pt])")
        } else {
            q
        }
    }

    /// OpenAlex full-text search syntax (filters are separate parameters).
    pub fn openalex(&self) -> String {
        self.boolean("")
    }

    /// Semantic Scholar's bulk search takes `+` (AND), `|` (OR) and quoted phrases.
    pub fn semantic_scholar(&self) -> String {
        self.groups
            .iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let terms: Vec<String> = g.iter().map(|t| quote(t)).collect();
                format!("({})", terms.join(" | "))
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// arXiv search_query: terms in title or abstract (`all:`), AND between groups.
    pub fn arxiv(&self) -> String {
        self.groups
            .iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let terms: Vec<String> = g.iter().map(|t| format!("all:{}", quote(t))).collect();
                format!("({})", terms.join(" OR "))
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

/// Parse an E-utilities esearch reply: (total count, PMIDs).
pub fn parse_pubmed_search(v: &serde_json::Value) -> Result<(u64, Vec<String>)> {
    let r = v
        .get("esearchresult")
        .context("no esearchresult in PubMed reply")?;
    let count = r["count"]
        .as_str()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let ids = r["idlist"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok((count, ids))
}

/// Parse an E-utilities esummary reply into hits, in the order of `ids`.
pub fn parse_pubmed_summaries(v: &serde_json::Value, ids: &[String]) -> Vec<ExternalHit> {
    ids.iter()
        .filter_map(|id| {
            let d = &v["result"][id];
            let title = d["title"].as_str()?.trim().to_string();
            Some(ExternalHit {
                source: "pubmed".into(),
                id: id.clone(),
                year: d["pubdate"]
                    .as_str()
                    .and_then(|p| p.get(..4))
                    .and_then(|y| y.parse().ok()),
                url: Some(format!("https://pubmed.ncbi.nlm.nih.gov/{id}/")),
                kinds: d["pubtype"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                title,
            })
        })
        .collect()
}

/// Parse an OpenAlex `/works` reply: (total count, hits).
pub fn parse_openalex(v: &serde_json::Value) -> Result<(u64, Vec<ExternalHit>)> {
    let results = v["results"]
        .as_array()
        .context("no results in OpenAlex reply")?;
    let hits = results
        .iter()
        .filter_map(|w| {
            let title = w["display_name"]
                .as_str()
                .or(w["title"].as_str())?
                .trim()
                .to_string();
            let id = w["id"].as_str()?.to_string();
            Some(ExternalHit {
                source: "openalex".into(),
                url: w["doi"]
                    .as_str()
                    .map(String::from)
                    .or_else(|| Some(id.clone())),
                id,
                year: w["publication_year"].as_i64().map(|y| y as i32),
                kinds: w["type"]
                    .as_str()
                    .map(|t| vec![t.to_string()])
                    .unwrap_or_default(),
                title,
            })
        })
        .collect();
    Ok((v["meta"]["count"].as_u64().unwrap_or(0), hits))
}

/// Earliest time of the next request per source, shared across threads. Only read and moved
/// forward under the lock; the waiting happens outside it.
static NEXT_SLOT: Mutex<[Option<Instant>; 4]> = Mutex::new([None; 4]);

/// Most attempts per request (the first try plus two retries), and the whole budget for one
/// verification's network work.
const ATTEMPTS: u32 = 3;
pub const BUDGET: Duration = Duration::from_secs(90);

thread_local! {
    /// The cancel token and deadline of the verification running on this thread.
    static LIMITS: std::cell::RefCell<(Option<invectus_core::CancelToken>, Option<Instant>)> =
        const { std::cell::RefCell::new((None, None)) };
}

/// Run `f` with network waits bounded by `deadline` and stopped by `cancel`.
pub fn with_limits<T>(
    cancel: Option<&invectus_core::CancelToken>,
    deadline: Instant,
    f: impl FnOnce() -> T,
) -> T {
    let before = LIMITS.with(|l| l.replace((cancel.cloned(), Some(deadline))));
    struct Restore(Option<(Option<invectus_core::CancelToken>, Option<Instant>)>);
    impl Drop for Restore {
        fn drop(&mut self) {
            if let Some(b) = self.0.take() {
                LIMITS.with(|l| *l.borrow_mut() = b);
            }
        }
    }
    let _restore = Restore(Some(before));
    f()
}

/// Sleep `d` unless cancelled or past the deadline first.
fn wait(d: Duration) -> Result<()> {
    let (cancel, deadline) = LIMITS.with(|l| l.borrow().clone());
    let end = Instant::now() + d;
    loop {
        if cancel.as_ref().is_some_and(|c| c.is_cancelled()) {
            bail!("cancelled");
        }
        if deadline.is_some_and(|t| Instant::now() >= t) {
            bail!("the outside sources took too long; try again later");
        }
        let left = end.saturating_duration_since(Instant::now());
        if left.is_zero() {
            return Ok(());
        }
        std::thread::sleep(left.min(Duration::from_millis(100)));
    }
}

/// Every attempt shares the check's remaining budget, including response-body reads.
/// Checking here also prevents a cancelled check from starting another request.
fn request_timeout() -> Result<Duration> {
    wait(Duration::ZERO)?;
    let maximum = Duration::from_secs(if cfg!(test) { 3 } else { 30 });
    LIMITS.with(|l| match l.borrow().1 {
        Some(deadline) => {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                bail!("the outside sources took too long; try again later");
            }
            Ok(remaining.min(maximum))
        }
        None => Ok(maximum),
    })
}

#[cfg(test)]
static ROUTE: Mutex<Option<String>> = Mutex::new(None);

/// `url`, or under test the same host and path on a local stand-in.
fn routed(url: &str) -> String {
    #[cfg(test)]
    if let Some(base) = ROUTE.lock().unwrap().clone()
        && let Some(rest) = url.strip_prefix("https://")
    {
        return format!("{base}/{rest}");
    }
    url.to_string()
}

/// The client for outside sources: https only, no redirects (a redirect could replay an API
/// key elsewhere), bounded connect, answer and total times (shorter under test).
fn agent() -> ureq::Agent {
    let t = |real: u64, test: u64| Some(Duration::from_secs(if cfg!(test) { test } else { real }));
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .https_only(!cfg!(test))
        .max_redirects(0)
        .timeout_connect(t(10, 2))
        .timeout_recv_response(t(20, 2))
        .timeout_global(t(30, 3))
        .build()
        .into()
}

fn get_json(source: Source, url: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
    let body = get_text_with(source, url, params, &[])?;
    serde_json::from_str(&body).with_context(|| format!("{} returned invalid JSON", source.name()))
}

fn cache_path(root: &std::path::Path, source: Source, key: &str) -> PathBuf {
    let h = hex::encode(Sha256::digest(key.as_bytes()));
    root.join("cache")
        .join("verify")
        .join(source.name())
        .join(format!("{}.json", &h[..32]))
}

/// Saved answers from outside sources for the project at `root` (`cache/verify/`).
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct CacheUsage {
    pub files: u64,
    pub bytes: u64,
}

fn cache_dir(root: &std::path::Path) -> PathBuf {
    root.join("cache").join("verify")
}

fn tally(dir: &std::path::Path, usage: &mut CacheUsage) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let Ok(meta) = std::fs::symlink_metadata(e.path()) else {
            continue;
        };
        if meta.is_dir() {
            tally(&e.path(), usage);
        } else if meta.is_file() {
            usage.files += 1;
            usage.bytes += meta.len();
        }
    }
}

/// How many saved answers there are and their size.
pub fn cache_usage(root: &std::path::Path) -> CacheUsage {
    let mut usage = CacheUsage::default();
    tally(&cache_dir(root), &mut usage);
    usage
}

/// Delete every saved answer, so the next check asks the sources again. Returns what was
/// removed. Links inside are removed, never followed.
pub fn clear_cache(root: &std::path::Path) -> Result<CacheUsage> {
    let usage = cache_usage(root);
    let dir = cache_dir(root);
    match std::fs::remove_dir_all(&dir) {
        Ok(()) => Ok(usage),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(CacheUsage::default()),
        Err(e) => Err(e).with_context(|| format!("removing {}", dir.display())),
    }
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Search one source (from the cache when a fresh answer exists).
pub fn search(
    project: &Project,
    source: Source,
    query: &KeywordQuery,
    max: usize,
) -> Result<SourceResult> {
    let files = ProjectFiles::for_project(project)?;
    search_with_files(&project.root, &files, source, query, max)
}

/// [`search`] for the project at `root`, without holding the project.
pub fn search_in(
    root: &std::path::Path,
    source: Source,
    query: &KeywordQuery,
    max: usize,
) -> Result<SourceResult> {
    let files = ProjectFiles::at(root)?;
    search_with_files(root, &files, source, query, max)
}

/// Uses the policy and key captured when the check was prepared, even if the app switches
/// projects while waiting for outside sources.
pub(crate) fn search_with_files(
    root: &std::path::Path,
    files: &ProjectFiles,
    source: Source,
    query: &KeywordQuery,
    max: usize,
) -> Result<SourceResult> {
    let q = source.query(query);
    let key = format!("{}|{q}|{}|{max}", source.name(), query.reviews_only);
    let path = cache_path(root, source, &key);
    let cached = match files.read(&path) {
        Ok(bytes) => Some(bytes),
        Err(e)
            if e.downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::NotFound) =>
        {
            None
        }
        Err(e) => return Err(e).context("reading the saved external-check answer"),
    };
    if let Some(bytes) = cached
        && let Ok(mut r) = serde_json::from_slice::<SourceResult>(&bytes)
        && now().saturating_sub(r.fetched_at) < CACHE_DAYS * 86_400
    {
        r.cached = true;
        return Ok(r);
    }
    let max_s = max.to_string();
    let result = match source {
        Source::PubMed => {
            let base = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
            let found = get_json(
                source,
                &format!("{base}/esearch.fcgi"),
                &[
                    ("db", "pubmed"),
                    ("term", &q),
                    ("retmode", "json"),
                    ("retmax", &max_s),
                    ("sort", "relevance"),
                    ("tool", "invectus"),
                ],
            )?;
            let (total, ids) = parse_pubmed_search(&found)?;
            let hits = if ids.is_empty() {
                Vec::new()
            } else {
                let joined = ids.join(",");
                let summaries = get_json(
                    source,
                    &format!("{base}/esummary.fcgi"),
                    &[
                        ("db", "pubmed"),
                        ("id", &joined),
                        ("retmode", "json"),
                        ("tool", "invectus"),
                    ],
                )?;
                parse_pubmed_summaries(&summaries, &ids)
            };
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
        Source::SemanticScholar => {
            let mut params = vec![
                ("query", q.as_str()),
                ("fields", "title,year,externalIds,url,publicationTypes"),
            ];
            if query.reviews_only {
                params.push(("publicationTypes", "Review"));
            }
            let v = get_json(
                source,
                "https://api.semanticscholar.org/graph/v1/paper/search/bulk",
                &params,
            )?;
            let (total, mut hits) = parse_semantic_scholar(&v)?;
            hits.truncate(max);
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
        Source::Arxiv => {
            // arXiv has no review filter: a reviews-only check still lists preprints.
            let xml = get_text(
                source,
                "https://export.arxiv.org/api/query",
                &[
                    ("search_query", q.as_str()),
                    ("max_results", max_s.as_str()),
                    ("sortBy", "relevance"),
                ],
            )?;
            let (total, hits) = parse_arxiv(&xml)?;
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
        Source::OpenAlex => {
            let mut params = vec![("search", q.as_str()), ("per-page", max_s.as_str())];
            if query.reviews_only {
                params.push(("filter", "type:review"));
            }
            let v = get_json(source, "https://api.openalex.org/works", &params)?;
            let (total, hits) = parse_openalex(&v)?;
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
    };
    files.write(&path, &serde_json::to_vec(&result)?)?;
    Ok(result)
}

/// Parse a Semantic Scholar bulk-search reply: (total, hits). Reviews are marked by
/// `publicationTypes` ("Review").
pub fn parse_semantic_scholar(v: &serde_json::Value) -> Result<(u64, Vec<ExternalHit>)> {
    let data = v["data"]
        .as_array()
        .context("no data in Semantic Scholar reply")?;
    let hits = data
        .iter()
        .filter_map(|p| {
            let id = p["paperId"].as_str()?.to_string();
            let title = p["title"].as_str()?.trim().to_string();
            let doi = p["externalIds"]["DOI"].as_str();
            Some(ExternalHit {
                source: "semanticscholar".into(),
                url: doi
                    .map(|d| format!("https://doi.org/{d}"))
                    .or_else(|| p["url"].as_str().map(String::from))
                    .or_else(|| Some(format!("https://www.semanticscholar.org/paper/{id}"))),
                year: p["year"].as_i64().map(|y| y as i32),
                kinds: p["publicationTypes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                id,
                title,
            })
        })
        .collect();
    Ok((v["total"].as_u64().unwrap_or(0), hits))
}

/// Parse an arXiv Atom feed: (total, hits). A small reader for the fixed feed shape, not a
/// general XML parser.
pub fn parse_arxiv(xml: &str) -> Result<(u64, Vec<ExternalHit>)> {
    if !xml.contains("<feed") {
        bail!("arXiv did not return a feed");
    }
    fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
        let open = s.find(&format!("<{name}"))?;
        let start = open + s[open..].find('>')? + 1;
        let end = start + s[start..].find(&format!("</{name}>"))?;
        Some(&s[start..end])
    }
    fn unescape(s: &str) -> String {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    }
    let total = tag(xml, "opensearch:totalResults")
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    let hits = xml
        .split("<entry>")
        .skip(1)
        .filter_map(|e| {
            let url = tag(e, "id")?.trim().to_string();
            let id = url.rsplit("/abs/").next().unwrap_or(&url).to_string();
            Some(ExternalHit {
                source: "arxiv".into(),
                title: unescape(tag(e, "title")?),
                year: tag(e, "published").and_then(|p| p.get(..4)?.parse().ok()),
                kinds: vec!["preprint".into()],
                url: Some(url),
                id,
            })
        })
        .collect();
    Ok((total, hits))
}

/// How one request ended.
enum Reply {
    /// No answer: refused, timed out or failed in transit (retried).
    Failed(anyhow::Error),
    /// HTTP 200, but the body could not be read (not retried).
    Unreadable(anyhow::Error),
    Body(String),
    /// Any other status; the body is not read.
    Status(u16),
}

/// Run one request on a helper thread and wait for it here, so cancellation or the end of the
/// budget stops the check at once rather than after the request (up to 30 s). An abandoned
/// request still ends by itself within its own timeout; its reply is logged and dropped, never
/// returned or cached.
fn send(
    req: ureq::RequestBuilder<ureq::typestate::WithoutBody>,
    source: Source,
    url: &str,
    purpose: &str,
) -> Result<Reply> {
    use std::sync::mpsc::{RecvTimeoutError, sync_channel};
    let (tx, rx) = sync_channel(1);
    let (url, purpose) = (url.to_string(), purpose.to_string());
    std::thread::Builder::new()
        .name(format!("invectus-check-{}", source.name()))
        .spawn(move || {
            let reply = match req.call() {
                Err(e) => {
                    invectus_core::net::record(&url, &purpose, None);
                    Reply::Failed(anyhow::Error::new(e))
                }
                Ok(resp) => {
                    let status = resp.status().as_u16();
                    invectus_core::net::record(&url, &purpose, Some(status));
                    if status == 200 {
                        match resp
                            .into_body()
                            .with_config()
                            .limit(16 * 1024 * 1024)
                            .read_to_string()
                        {
                            Ok(body) => Reply::Body(body),
                            Err(e) => Reply::Unreadable(anyhow::Error::new(e)),
                        }
                    } else {
                        Reply::Status(status)
                    }
                }
            };
            let _ = tx.send(reply);
        })
        .context("starting a request")?;
    loop {
        // Cancellation or expiry while waiting (including for the body) ends the check, and a
        // reply that arrives at the same moment is not used.
        wait(Duration::ZERO)?;
        match rx.recv_timeout(Duration::from_millis(50)) {
            Ok(reply) => {
                wait(Duration::ZERO)?;
                return Ok(reply);
            }
            Err(RecvTimeoutError::Timeout) => {}
            Err(RecvTimeoutError::Disconnected) => bail!("the request stopped unexpectedly"),
        }
    }
}

fn get_text(source: Source, url: &str, params: &[(&str, &str)]) -> Result<String> {
    get_text_with(source, url, params, &[])
}

fn get_text_with(
    source: Source,
    url: &str,
    params: &[(&str, &str)],
    headers: &[(&str, &str)],
) -> Result<String> {
    invectus_core::net::ensure_online(&format!("{} verification", source.name()))?;
    if !url.starts_with("https://") {
        bail!("refusing a non-https address for {}", source.name());
    }
    let host = invectus_core::net::host_of(url);
    let purpose = format!("Gap check: {}", source.name());
    let agent = agent();
    let mut last_err: Option<anyhow::Error> = None;
    for attempt in 0..ATTEMPTS {
        // Reserve the next slot under the lock, then wait for it outside the lock.
        let slot = {
            let mut next = NEXT_SLOT.lock().unwrap_or_else(|e| e.into_inner());
            let now = Instant::now();
            let at = next[source as usize].map_or(now, |t| t.max(now));
            next[source as usize] = Some(at + source.spacing());
            at
        };
        wait(slot.saturating_duration_since(Instant::now()))?;
        let target = routed(url);
        let mut req = agent.get(&target).header("User-Agent", USER_AGENT);
        for (k, v) in params {
            req = req.query(*k, *v);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        let timeout = request_timeout()?;
        let req = req.config().timeout_global(Some(timeout)).build();
        let status = match send(req, source, url, &purpose)? {
            Reply::Failed(e) => {
                last_err = Some(e.context(format!("GET {host}")));
                wait(Duration::from_secs(1 << attempt))?;
                continue;
            }
            Reply::Unreadable(e) => {
                return Err(e.context(format!("reading {}'s answer", source.name())));
            }
            Reply::Body(body) => return Ok(body),
            Reply::Status(status) => status,
        };
        match status {
            300..=399 => bail!(
                "{} answered with a redirect (HTTP {status}); not followed",
                source.name()
            ),
            401 | 403 => bail!("{} refused the request (HTTP {status})", source.name()),
            429 | 500..=599 => {
                last_err = Some(anyhow::anyhow!("{} answered HTTP {status}", source.name()));
                wait(Duration::from_secs(2 << attempt))?;
            }
            _ => bail!("{} answered HTTP {status}", source.name()),
        }
    }
    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("no attempt was made")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_use_each_sources_syntax() {
        let q = KeywordQuery {
            groups: vec![
                vec!["corneal".into(), "cross-linking".into()],
                vec!["choroidal thickness".into()],
            ],
            reviews_only: true,
        };
        assert_eq!(
            q.pubmed(),
            "(corneal[tiab] OR \"cross-linking\"[tiab]) AND (\"choroidal thickness\"[tiab]) \
             AND (review[pt] OR systematic review[pt] OR meta-analysis[pt])"
        );
        assert_eq!(
            q.openalex(),
            "(corneal OR \"cross-linking\") AND (\"choroidal thickness\")"
        );
        assert_eq!(
            q.semantic_scholar(),
            "(corneal | \"cross-linking\") + (\"choroidal thickness\")"
        );
        assert_eq!(
            q.arxiv(),
            "(all:corneal OR all:\"cross-linking\") AND (all:\"choroidal thickness\")"
        );
    }

    #[test]
    fn pubmed_and_openalex_replies_parse() {
        let search =
            serde_json::json!({"esearchresult": {"count": "42", "idlist": ["111", "222"]}});
        assert_eq!(
            parse_pubmed_search(&search).unwrap(),
            (42, vec!["111".to_string(), "222".to_string()])
        );
        let summary = serde_json::json!({"result": {
            "uids": ["111", "222"],
            "111": {"title": "A review of things.", "pubdate": "2021 Mar 3",
                    "pubtype": ["Journal Article", "Review"]},
            "222": {"title": "Another", "pubdate": ""}
        }});
        let hits = parse_pubmed_summaries(&summary, &["111".into(), "222".into(), "333".into()]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].year, Some(2021));
        assert_eq!(hits[0].kinds, vec!["Journal Article", "Review"]);
        assert_eq!(
            hits[0].url.as_deref(),
            Some("https://pubmed.ncbi.nlm.nih.gov/111/")
        );
        assert_eq!(hits[1].year, None);
        let oa = serde_json::json!({"meta": {"count": 7}, "results": [
            {"id": "https://openalex.org/W1", "display_name": "Work one", "publication_year": 2019,
             "doi": "https://doi.org/10.1/x", "type": "review"},
            {"id": "https://openalex.org/W2", "title": "Work two"}
        ]});
        let (n, hits) = parse_openalex(&oa).unwrap();
        assert_eq!(n, 7);
        assert_eq!(hits[0].url.as_deref(), Some("https://doi.org/10.1/x"));
        assert_eq!(hits[1].url.as_deref(), Some("https://openalex.org/W2"));
        assert!(parse_openalex(&serde_json::json!({})).is_err());
        let s2 = serde_json::json!({"total": 12, "data": [
            {"paperId": "abc", "title": "A survey", "year": 2022,
             "externalIds": {"DOI": "10.2/y"}, "publicationTypes": ["Review"]},
            {"paperId": "def", "title": "No doi", "year": null, "externalIds": {}},
            {"paperId": "ghi"}
        ]});
        let (n, hits) = parse_semantic_scholar(&s2).unwrap();
        assert_eq!((n, hits.len()), (12, 2));
        assert_eq!(hits[0].url.as_deref(), Some("https://doi.org/10.2/y"));
        assert_eq!(hits[0].kinds, vec!["Review"]);
        assert_eq!(
            hits[1].url.as_deref(),
            Some("https://www.semanticscholar.org/paper/def")
        );
        let feed = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom">
          <opensearch:totalResults xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">5</opensearch:totalResults>
          <entry><id>http://arxiv.org/abs/2101.00001v2</id><published>2021-01-01T00:00:00Z</published>
          <title>Corneal   cross-linking
            &amp; thickness</title></entry>
          <entry><id>http://arxiv.org/abs/2202.00002v1</id><title>Second</title></entry>
        </feed>"#;
        let (n, hits) = parse_arxiv(feed).unwrap();
        assert_eq!((n, hits.len()), (5, 2));
        assert_eq!(hits[0].title, "Corneal cross-linking & thickness");
        assert_eq!(hits[0].id, "2101.00001v2");
        assert_eq!(hits[0].year, Some(2021));
        assert_eq!(hits[1].year, None);
        assert!(parse_arxiv("<html>").is_err());
    }

    #[test]
    fn the_check_cache_can_be_measured_and_cleared() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(cache_usage(dir.path()), CacheUsage::default());
        assert_eq!(clear_cache(dir.path()).unwrap(), CacheUsage::default());
        for (source, key) in [(Source::PubMed, "a"), (Source::OpenAlex, "b")] {
            let p = cache_path(dir.path(), source, key);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(&p, b"{}").unwrap();
        }
        let other = dir.path().join("keep.txt");
        std::fs::write(&other, b"x").unwrap();
        assert_eq!(cache_usage(dir.path()), CacheUsage { files: 2, bytes: 4 });
        assert_eq!(
            clear_cache(dir.path()).unwrap(),
            CacheUsage { files: 2, bytes: 4 }
        );
        assert_eq!(cache_usage(dir.path()), CacheUsage::default());
        assert!(other.exists(), "only the check cache is removed");
    }

    #[test]
    fn offline_mode_refuses_uncached_searches_but_serves_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::create(&dir.path().join("t")).unwrap();
        let q = KeywordQuery {
            groups: vec![vec!["x".into()]],
            reviews_only: false,
        };
        invectus_core::net::scoped_offline(true, || {
            let err = search(&p, Source::OpenAlex, &q, 5).unwrap_err().to_string();
            assert!(err.contains("offline"), "{err}");
            let cached = SourceResult {
                source: "openalex".into(),
                query: q.openalex(),
                total: Some(1),
                hits: vec![],
                cached: false,
                fetched_at: now(),
            };
            let path = cache_path(
                &p.root,
                Source::OpenAlex,
                &format!("openalex|{}|false|5", q.openalex()),
            );
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            ProjectFiles::for_project(&p)
                .unwrap()
                .write(&path, &serde_json::to_vec(&cached).unwrap())
                .unwrap();
            let r = search(&p, Source::OpenAlex, &q, 5).unwrap();
            assert!(r.cached && r.total == Some(1));
        });
    }

    // ---- Availability and boundaries (audit 2026-11-12, item 2) --------------------------

    /// One stand-in at a time: the route is process-wide.
    static SERIAL: Mutex<()> = Mutex::new(());

    /// A local server that answers every request with `reply(n)` (n counts from 1) after
    /// `delay`; returns the request counter.
    fn stand_in(
        delay: Duration,
        reply: fn(usize) -> (u16, &'static str),
    ) -> std::sync::Arc<std::sync::atomic::AtomicUsize> {
        use std::io::{BufRead, BufReader, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen = count.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let n = seen.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                std::thread::spawn(move || {
                    let mut r = BufReader::new(stream.try_clone().unwrap());
                    let mut line = String::new();
                    while r.read_line(&mut line).unwrap_or(0) > 0 && line != "\r\n" {
                        line.clear();
                    }
                    std::thread::sleep(delay);
                    let (status, body) = reply(n);
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                        body.len()
                    );
                });
            }
        });
        *ROUTE.lock().unwrap() = Some(format!("http://{addr}"));
        count
    }

    fn query() -> KeywordQuery {
        KeywordQuery {
            groups: vec![vec!["retina".into()]],
            reviews_only: false,
        }
    }

    fn fresh_root() -> tempfile::TempDir {
        let root = tempfile::tempdir().unwrap();
        // Legacy, plaintext cache policy without opening a database (these tests exercise
        // transport failures rather than project creation).
        std::fs::write(
            root.path().join("project.json"),
            br#"{"format_version":1,"name":"Transport test","network_enabled":false}"#,
        )
        .unwrap();
        root
    }

    #[test]
    fn encrypted_cache_round_trips_offline_and_refuses_tampering_and_missing_keys() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let count = stand_in(Duration::ZERO, |_| {
            (
                200,
                r#"{"meta":{"count":1},"results":[{"id":"https://openalex.org/W1","display_name":"PRIVATE_CACHE_MARKER"}]}"#,
            )
        });
        let dir = tempfile::tempdir().unwrap();
        let project = Project::create(&dir.path().join("encrypted-cache")).unwrap();
        let q = query();
        let first = search(&project, Source::OpenAlex, &q, 5).unwrap();
        assert_eq!(first.hits[0].title, "PRIVATE_CACHE_MARKER");
        let key = format!("openalex|{}|false|5", q.openalex());
        let path = cache_path(&project.root, Source::OpenAlex, &key);
        let mut stored = std::fs::read(&path).unwrap();
        assert!(invectus_core::encrypted_file::is_encrypted(&stored));
        assert!(
            !stored
                .windows(b"PRIVATE_CACHE_MARKER".len())
                .any(|w| w == b"PRIVATE_CACHE_MARKER")
        );
        let cached =
            invectus_core::net::scoped_offline(true, || search(&project, Source::OpenAlex, &q, 5))
                .unwrap();
        assert!(cached.cached);
        assert_eq!(cached.hits[0].title, first.hits[0].title);
        *stored.last_mut().unwrap() ^= 1;
        std::fs::write(&path, stored).unwrap();
        let error = search(&project, Source::OpenAlex, &q, 5).unwrap_err();
        assert!(format!("{error:#}").contains("authentication"), "{error:#}");
        let code = project.recovery_code().unwrap();
        let id = project.config.project_id.as_ref().unwrap();
        invectus_core::keys::forget(id).unwrap();
        let error = search_in(&project.root, Source::OpenAlex, &q, 5).unwrap_err();
        assert!(format!("{error:#}").contains("key"), "{error:#}");
        project.restore_key(&code).unwrap();
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn offline_sends_zero_requests() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let count = stand_in(Duration::ZERO, |_| (200, "{}"));
        let root = fresh_root();
        let err = invectus_core::net::scoped_offline(true, || {
            search_in(root.path(), Source::OpenAlex, &query(), 5)
                .unwrap_err()
                .to_string()
        });
        assert!(err.contains("offline"), "{err}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn a_stalled_source_times_out() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _count = stand_in(Duration::from_secs(30), |_| (200, "{}"));
        let root = fresh_root();
        let t = Instant::now();
        // Three attempts, each cut off by the answer deadline, with backoff between.
        let err = with_limits(None, Instant::now() + Duration::from_secs(20), || {
            search_in(root.path(), Source::OpenAlex, &query(), 5).unwrap_err()
        });
        assert!(
            t.elapsed() < Duration::from_secs(15),
            "took {:?}: {err:#}",
            t.elapsed()
        );
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn the_budget_bounds_the_whole_check() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let count = stand_in(Duration::from_secs(30), |_| (200, "{}"));
        let root = fresh_root();
        let t = Instant::now();
        let err = with_limits(None, Instant::now() + Duration::from_millis(250), || {
            search_in(root.path(), Source::OpenAlex, &query(), 5)
                .unwrap_err()
                .to_string()
        });
        assert!(err.contains("too long"), "{err}");
        assert!(
            t.elapsed() < Duration::from_millis(1500),
            "took {:?}",
            t.elapsed()
        );
        // The abandoned request may reach the stand-in after the check gave up (it runs on a
        // helper thread), but no retry follows it.
        std::thread::sleep(Duration::from_millis(500));
        assert!(count.load(std::sync::atomic::Ordering::SeqCst) <= 1);
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn expired_or_cancelled_checks_send_no_requests() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let count = stand_in(Duration::ZERO, |_| (200, "{}"));
        let root = fresh_root();
        let error = with_limits(None, Instant::now(), || {
            search_in(root.path(), Source::OpenAlex, &query(), 5).unwrap_err()
        });
        assert!(error.to_string().contains("too long"), "{error:#}");
        let cancel = invectus_core::CancelToken::default();
        cancel.cancel();
        let error = with_limits(Some(&cancel), Instant::now() + BUDGET, || {
            search_in(root.path(), Source::OpenAlex, &query(), 5).unwrap_err()
        });
        assert!(error.to_string().contains("cancelled"), "{error:#}");
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 0);
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn cancellation_during_a_request_discards_the_reply() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        // The stand-in answers only after 2.5 s (inside the 3 s test timeout): cancelling
        // must not wait for it.
        let count = stand_in(Duration::from_millis(2500), |_| (200, "{}"));
        let cancel = invectus_core::CancelToken::default();
        let c2 = cancel.clone();
        let seen = count.clone();
        let cancelling = std::thread::spawn(move || {
            let deadline = Instant::now() + Duration::from_secs(5);
            while seen.load(std::sync::atomic::Ordering::SeqCst) == 0 {
                assert!(Instant::now() < deadline, "no request reached the server");
                std::thread::sleep(Duration::from_millis(5));
            }
            c2.cancel();
        });
        let t = Instant::now();
        let error = with_limits(Some(&cancel), Instant::now() + BUDGET, || {
            get_text(Source::OpenAlex, "https://api.openalex.org/works", &[]).unwrap_err()
        });
        cancelling.join().unwrap();
        assert!(error.to_string().contains("cancelled"), "{error:#}");
        assert!(
            t.elapsed() < Duration::from_millis(1500),
            "took {:?}",
            t.elapsed()
        );
        assert_eq!(count.load(std::sync::atomic::Ordering::SeqCst), 1);
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn cancelling_stops_a_check_promptly() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let _count = stand_in(Duration::ZERO, |_| (503, "{}"));
        let root = fresh_root();
        let cancel = invectus_core::CancelToken::default();
        let c2 = cancel.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            c2.cancel();
        });
        let t = Instant::now();
        let err = with_limits(Some(&cancel), Instant::now() + BUDGET, || {
            search_in(root.path(), Source::OpenAlex, &query(), 5)
                .unwrap_err()
                .to_string()
        });
        assert!(err.contains("cancelled"), "{err}");
        assert!(
            t.elapsed() < Duration::from_secs(2),
            "took {:?}",
            t.elapsed()
        );
        *ROUTE.lock().unwrap() = None;
    }

    #[test]
    fn retries_are_bounded_and_redirects_refused() {
        let _g = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        let count = stand_in(Duration::ZERO, |_| (500, "{}"));
        let root = fresh_root();
        let _ = with_limits(None, Instant::now() + BUDGET, || {
            search_in(root.path(), Source::OpenAlex, &query(), 5)
        });
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            ATTEMPTS as usize
        );
        let count = stand_in(Duration::ZERO, |_| (302, ""));
        let root = fresh_root();
        let err = with_limits(None, Instant::now() + BUDGET, || {
            search_in(root.path(), Source::OpenAlex, &query(), 5)
                .unwrap_err()
                .to_string()
        });
        assert!(err.contains("redirect"), "{err}");
        assert_eq!(
            count.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "a redirect is not retried"
        );
        *ROUTE.lock().unwrap() = None;
    }
}
