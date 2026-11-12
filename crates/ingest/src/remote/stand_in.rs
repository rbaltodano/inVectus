//! A local stand-in for the services a pull talks to, so each integration's whole pull (paging,
//! retries, errors, the CSV it writes) and its listing can be tested without accounts. Answers
//! follow each API's documented shapes; they are not recordings of the real services.
//! `INVECTUS_STAND_IN_OUT=<dir>` keeps each pulled CSV there for checking imports by hand.

use super::browse::choices;
use super::{PullContext, STAND_IN, Source, pull};
use invectus_core::CancelToken;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::net::TcpListener;
use std::path::Path;
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug)]
pub struct Request {
    pub method: String,
    /// `<host>/<path>` without the query, e.g. `api.notion.com/v1/search`.
    pub path: String,
    pub query: HashMap<String, String>,
    pub headers: HashMap<String, String>,
    pub body: Value,
}

pub struct Reply {
    status: u16,
    body: String,
    headers: Vec<(String, String)>,
}

fn ok(v: Value) -> Reply {
    Reply {
        status: 200,
        body: v.to_string(),
        headers: vec![],
    }
}

fn status(code: u16, v: Value) -> Reply {
    Reply {
        status: code,
        body: v.to_string(),
        headers: vec![],
    }
}

/// Requests seen so far; dropping it sends requests to the real services again.
struct StandIn {
    seen: Arc<Mutex<Vec<Request>>>,
}

impl StandIn {
    fn seen(&self) -> Vec<Request> {
        self.seen.lock().unwrap().clone()
    }
    fn count(&self, path: &str) -> usize {
        self.seen().iter().filter(|r| r.path == path).count()
    }
}

impl Drop for StandIn {
    fn drop(&mut self) {
        STAND_IN.with(|s| *s.borrow_mut() = None);
    }
}

fn start(handler: impl Fn(&Request) -> Reply + Send + Sync + 'static) -> StandIn {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            if reader.read_line(&mut line).unwrap_or(0) == 0 {
                continue;
            }
            let mut parts = line.split_whitespace();
            let method = parts.next().unwrap_or("").to_string();
            let target = parts
                .next()
                .unwrap_or("")
                .trim_start_matches('/')
                .to_string();
            let mut headers = HashMap::new();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h.trim().is_empty() {
                    break;
                }
                if let Some((k, v)) = h.split_once(':') {
                    headers.insert(k.trim().to_lowercase(), v.trim().to_string());
                }
            }
            let len = headers
                .get("content-length")
                .and_then(|l| l.parse().ok())
                .unwrap_or(0);
            let mut body = vec![0; len];
            reader.read_exact(&mut body).unwrap();
            let (path, q) = target.split_once('?').unwrap_or((&target, ""));
            let query = q
                .split('&')
                .filter_map(|kv| kv.split_once('='))
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect();
            let req = Request {
                method,
                path: path.to_string(),
                query,
                headers,
                body: serde_json::from_slice(&body).unwrap_or(Value::Null),
            };
            log.lock().unwrap().push(req.clone());
            let reply = handler(&req);
            let mut head = format!(
                "HTTP/1.1 {} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                reply.status,
                reply.body.len()
            );
            if !reply
                .headers
                .iter()
                .any(|(k, _)| k.eq_ignore_ascii_case("content-type"))
            {
                head.push_str("Content-Type: application/json\r\n");
            }
            for (k, v) in &reply.headers {
                head.push_str(&format!("{k}: {v}\r\n"));
            }
            head.push_str("\r\n");
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(reply.body.as_bytes());
        }
    });
    STAND_IN.with(|s| *s.borrow_mut() = Some(format!("http://{addr}")));
    StandIn { seen }
}

/// Pull `spec` into a temporary project; returns the snapshot's header and rows.
fn pull_spec(spec: &str, token: &str) -> anyhow::Result<(Vec<String>, Vec<Vec<String>>)> {
    let dir = tempfile::tempdir()?;
    let source = Source::parse_remote(spec, dir.path())?.expect("a remote spec");
    let cancel = CancelToken::default();
    let mut progress = |_: &str, _: usize, _: usize| {};
    let mut cx = PullContext {
        token: Some(token.into()),
        cancel: &cancel,
        progress: &mut progress,
    };
    let pulled = pull(&source, &mut cx)?;
    let text = std::fs::read_to_string(&pulled.snapshot)?;
    if let Ok(out) = std::env::var("INVECTUS_STAND_IN_OUT") {
        let name = format!(
            "{}.csv",
            spec.replace(|c: char| !c.is_ascii_alphanumeric(), "-")
        );
        std::fs::write(Path::new(&out).join(name), &text)?;
    }
    let mut records = parse_csv(&text);
    let header = records.remove(0);
    let rows = records;
    assert_eq!(pulled.rows, rows.len(), "the pull record counts the rows");
    Ok((header, rows))
}

/// Records of a CSV text (quoted fields may hold commas, quotes and newlines).
fn parse_csv(text: &str) -> Vec<Vec<String>> {
    let (mut out, mut row, mut field) = (Vec::new(), Vec::new(), String::new());
    let (mut quoted, mut chars) = (false, text.chars().peekable());
    while let Some(c) = chars.next() {
        match (c, quoted) {
            ('"', true) if chars.peek() == Some(&'"') => {
                field.push('"');
                chars.next();
            }
            ('"', _) => quoted = !quoted,
            (',', false) => row.push(std::mem::take(&mut field)),
            ('\n', false) => {
                row.push(std::mem::take(&mut field));
                out.push(std::mem::take(&mut row));
            }
            _ => field.push(c),
        }
    }
    if !field.is_empty() || !row.is_empty() {
        row.push(field);
        out.push(row);
    }
    out
}

fn col(header: &[String], rows: &[Vec<String>], name: &str) -> Vec<String> {
    let i = header
        .iter()
        .position(|h| h == name)
        .unwrap_or_else(|| panic!("no column {name} in {header:?}"));
    rows.iter().map(|r| r[i].clone()).collect()
}

// ---- Notion ----------------------------------------------------------------------------------

const DB: &str = "0123456789abcdef0123456789abcdef";

fn notion_page(n: usize) -> Value {
    json!({ "object": "page", "id": format!("page-{n}"), "url": format!("https://www.notion.so/page-{n}"),
        "created_time": "2026-10-01T00:00:00.000Z", "last_edited_time": "2026-10-02T00:00:00.000Z",
        "properties": {
            "Name": { "type": "title", "title": [{ "plain_text": format!("Request {n}") }] },
            "Notes": { "type": "rich_text", "rich_text": [{ "plain_text": "Customers " }, { "plain_text": "want it" }] },
            "Status": { "type": "status", "status": { "name": "Open" } },
            "Tags": { "type": "multi_select", "multi_select": [{ "name": "ui" }, { "name": "export" }] },
            "Votes": { "type": "number", "number": n as f64 },
            "url": { "type": "url", "url": "https://example.com/r" },
            "Due": { "type": "date", "date": { "start": "2026-12-01", "end": null } },
        } })
}

fn notion(req: &Request) -> Reply {
    if req.headers.get("authorization").map(String::as_str) != Some("Bearer ntn_good") {
        return status(
            401,
            json!({ "object": "error", "code": "unauthorized", "message": "API token is invalid." }),
        );
    }
    assert_eq!(req.headers["notion-version"], "2022-06-28");
    let p = req.path.trim_start_matches("api.notion.com/v1/");
    if p == format!("databases/{DB}") {
        return ok(json!({ "object": "database", "properties": {
            "Votes": { "type": "number" }, "Name": { "type": "title" }, "Notes": { "type": "rich_text" },
            "Status": { "type": "status" }, "Tags": { "type": "multi_select" }, "url": { "type": "url" },
            "Due": { "type": "date" } } }));
    }
    if p == format!("databases/{DB}/query") {
        assert_eq!(req.method, "POST", "Notion queries a database with POST");
        // Three pages over two pages of results (the paging, not the page size, is under test).
        return if req.body.get("start_cursor").is_none() {
            ok(
                json!({ "results": (0..2).map(notion_page).collect::<Vec<_>>(), "has_more": true, "next_cursor": "c2" }),
            )
        } else {
            assert_eq!(req.body["start_cursor"], "c2");
            ok(json!({ "results": [notion_page(2)], "has_more": false, "next_cursor": null }))
        };
    }
    if let Some(block) = p
        .strip_prefix("blocks/")
        .and_then(|b| b.strip_suffix("/children"))
    {
        let para = |id: &str, t: &str, kids: bool| json!({ "id": id, "type": "paragraph", "has_children": kids, "paragraph": { "rich_text": [{ "plain_text": t }] } });
        return ok(match block {
            "page-0" if !req.query.contains_key("start_cursor") => json!({ "results": [
                para("b1", "First paragraph", false),
                { "id": "b2", "type": "bulleted_list_item", "has_children": true, "bulleted_list_item": { "rich_text": [{ "plain_text": "A point" }] } },
                { "id": "b3", "type": "child_page", "has_children": true, "child_page": { "title": "Sub page" } },
            ], "has_more": true, "next_cursor": "bc2" }),
            "page-0" => json!({ "results": [
                { "id": "b4", "type": "to_do", "has_children": false, "to_do": { "rich_text": [{ "plain_text": "Ship it" }], "checked": true } }
            ], "has_more": false }),
            "b2" => json!({ "results": [para("b2a", "Nested detail", false)], "has_more": false }),
            "b3" => panic!("child pages are their own items and must not be read"),
            _ => json!({ "results": [para("x", "Body text", false)], "has_more": false }),
        });
    }
    if p == "search" {
        assert_eq!(req.body["filter"]["value"], "database");
        return ok(if req.body.get("start_cursor").is_none() {
            json!({ "results": [{ "object": "database", "id": "aaaaaaaa-0000-0000-0000-000000000001", "title": [{ "plain_text": "Requests" }] }], "has_more": true, "next_cursor": "s2" })
        } else {
            json!({ "results": [{ "object": "database", "id": "aaaaaaaa-0000-0000-0000-000000000002", "title": [{ "plain_text": "Interviews" }] }], "has_more": false })
        });
    }
    status(
        404,
        json!({ "object": "error", "code": "object_not_found", "message": "Could not find database" }),
    )
}

#[test]
fn notion_pulls_every_page_with_properties_and_body() {
    let s = start(notion);
    let (h, rows) = pull_spec(&format!("notion:{DB}"), "ntn_good").unwrap();
    assert_eq!(
        h,
        [
            "page_id",
            "url",
            "created_time",
            "last_edited_time",
            "Name",
            "Due",
            "Notes",
            "Status",
            "Tags",
            "property_url",
            "Votes",
            "body"
        ]
    );
    assert_eq!(rows.len(), 3, "both pages of results");
    assert_eq!(rows[0][4], "Request 0");
    assert_eq!(col(&h, &rows, "Notes")[0], "Customers want it");
    assert_eq!(col(&h, &rows, "Tags")[0], "ui, export");
    assert_eq!(col(&h, &rows, "Votes")[2], "2");
    assert_eq!(col(&h, &rows, "Due")[0], "2026-12-01");
    assert_eq!(
        col(&h, &rows, "property_url")[0],
        "https://example.com/r",
        "a property named like a fixed column keeps its data"
    );
    assert_eq!(
        col(&h, &rows, "body")[0],
        "First paragraph\n- A point\nNested detail\n[x] Ship it"
    );
    assert_eq!(col(&h, &rows, "body")[2], "Body text");
    assert_eq!(
        s.count(&format!("api.notion.com/v1/databases/{DB}/query")),
        2
    );
}

#[test]
fn notion_errors_explain_what_to_do() {
    let _s = start(notion);
    let e = pull_spec(&format!("notion:{DB}"), "ntn_bad")
        .unwrap_err()
        .to_string();
    assert!(e.contains("refused the token"), "{e}");
    let e = pull_spec("notion:ffffffffffffffffffffffffffffffff", "ntn_good")
        .unwrap_err()
        .to_string();
    assert!(e.contains("Connections"), "{e}");
}

#[test]
fn notion_lists_databases_across_pages() {
    let _s = start(notion);
    let c = choices("notion", "ntn_good").unwrap();
    let t: Vec<&str> = c.iter().map(|c| c.title.as_str()).collect();
    assert_eq!(t, ["Requests", "Interviews"]);
    assert_eq!(c[1].spec, "notion:aaaaaaaa000000000000000000000002");
}

#[test]
fn rate_limits_are_waited_out() {
    let hits = Arc::new(Mutex::new(0));
    let h = hits.clone();
    let _s = start(move |req| {
        let mut n = h.lock().unwrap();
        *n += 1;
        if *n <= 2 {
            return Reply {
                status: 429,
                body: "{}".into(),
                headers: vec![("Retry-After".into(), "0".into())],
            };
        }
        notion(req)
    });
    assert_eq!(choices("notion", "ntn_good").unwrap().len(), 2);
    assert!(*hits.lock().unwrap() >= 4);
}

// ---- Zotero ----------------------------------------------------------------------------------

fn zotero_item(n: usize, kind: &str) -> Value {
    json!({ "key": format!("K{n:07}"), "data": { "key": format!("K{n:07}"), "itemType": kind,
        "title": format!("Paper {n}"), "abstractNote": "An abstract.", "date": "2024",
        "creators": [{ "lastName": "Lee", "firstName": "A." }], "publicationTitle": "Optometry",
        "DOI": "10.1/x", "url": "https://doi.org/10.1/x", "tags": [{ "tag": "myopia" }],
        "collections": ["AAAAAAAA"], "dateAdded": "2024-01-01T00:00:00Z", "dateModified": "2024-01-02T00:00:00Z", "extra": "" } })
}

fn zotero(req: &Request) -> Reply {
    if req.headers.get("zotero-api-key").map(String::as_str) != Some("abcdefghijklmnopqrstuvwx") {
        return status(403, json!("Forbidden"));
    }
    assert_eq!(req.headers["zotero-api-version"], "3");
    let p = req.path.trim_start_matches("api.zotero.org/");
    let start: usize = req
        .query
        .get("start")
        .and_then(|s| s.parse().ok())
        .unwrap_or(0);
    match p {
        "keys/current" => ok(
            json!({ "userID": 42, "access": { "user": { "library": true }, "groups": { "all": { "library": true } } } }),
        ),
        "users/42/groups" => ok(json!([{ "id": 4711, "data": { "name": "Vision Lab" } }])),
        "users/42/items/top" | "users/42/collections/AAAAAAAA/items/top" => {
            // 100 items, then 3 more including a standalone note (skipped).
            let page: Vec<Value> = if start == 0 {
                (0..100).map(|n| zotero_item(n, "journalArticle")).collect()
            } else {
                vec![
                    zotero_item(100, "book"),
                    zotero_item(101, "note"),
                    zotero_item(102, "conferencePaper"),
                ]
            };
            ok(json!(page))
        }
        "groups/4711/items/top" => ok(json!([zotero_item(1, "thesis")])),
        "users/42/collections" => ok(if start == 0 {
            json!((0..100).map(|n| json!({ "key": format!("C{n:07}"), "data": { "name": format!("Topic {n:03}"), "parentCollection": false }, "meta": { "numItems": n } })).collect::<Vec<_>>())
        } else {
            json!([{ "key": "CSUB0001", "data": { "name": "Sub", "parentCollection": "C0000000" }, "meta": { "numItems": 1 } }])
        }),
        "groups/4711/collections" => ok(json!([])),
        _ => status(404, json!("Not found")),
    }
}

#[test]
fn zotero_pulls_libraries_collections_and_groups() {
    let s = start(zotero);
    let (h, rows) = pull_spec("zotero:library", "abcdefghijklmnopqrstuvwx").unwrap();
    assert_eq!(h[..4], ["key", "item_type", "title", "abstract"]);
    assert_eq!(
        rows.len(),
        102,
        "both pages; the standalone note is skipped"
    );
    assert_eq!(col(&h, &rows, "creators")[0], "Lee, A.");
    let (_, rows) = pull_spec("zotero:collection:AAAAAAAA", "abcdefghijklmnopqrstuvwx").unwrap();
    assert_eq!(rows.len(), 102);
    let before = s.count("api.zotero.org/keys/current");
    let (_, rows) = pull_spec("zotero:group:4711", "abcdefghijklmnopqrstuvwx").unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        s.count("api.zotero.org/keys/current"),
        before,
        "a group library needs no key lookup"
    );
    let e = pull_spec("zotero:library", "xxxxxxxxxxxxxxxxxxxxxxxx")
        .unwrap_err()
        .to_string();
    assert!(e.contains("refused the API key"), "{e}");
}

#[test]
fn zotero_lists_both_libraries_and_every_collection() {
    let _s = start(zotero);
    let c = choices("zotero", "abcdefghijklmnopqrstuvwx").unwrap();
    // My Library: whole + 101 collections; Vision Lab: whole.
    assert_eq!(c.len(), 103);
    assert_eq!(c[0].spec, "zotero:library");
    assert_eq!(c[1].title, "Topic 000");
    assert_eq!(c[2].title, "Topic 000 / Sub");
    assert_eq!(c.last().unwrap().spec, "zotero:group:4711");
}

// ---- Airtable --------------------------------------------------------------------------------

const BASE: &str = "appAAAAAAAAAAAAAA";
const TABLE: &str = "tblAAAAAAAAAAAAAA";

fn airtable(req: &Request) -> Reply {
    if req.headers.get("authorization").map(String::as_str) != Some("Bearer patgood") {
        return status(
            401,
            json!({ "error": { "type": "AUTHENTICATION_REQUIRED", "message": "Authentication required" } }),
        );
    }
    let p = req.path.trim_start_matches("api.airtable.com/v0/");
    if p == format!("{BASE}/{TABLE}") {
        let rec = |n: usize, f: Value| json!({ "id": format!("rec{n:014}"), "createdTime": "2026-01-01T00:00:00.000Z", "fields": f });
        return ok(match req.query.get("offset").map(String::as_str) {
            None => json!({ "records": [
                rec(1, json!({ "Title": "Dark mode", "Votes": 4 })),
                rec(2, json!({ "Title": "Export", "Owner": { "id": "usr1", "name": "Ada" }, "record_id": "legacy-2" })),
            ], "offset": "itr2/rec2" }),
            Some("itr2/rec2") => json!({ "records": [
                rec(3, json!({ "Title": "Offline", "Tags": ["sync", "mobile"], "Files": [{ "id": "att", "filename": "spec.pdf" }] })),
            ] }),
            o => panic!("unexpected offset {o:?}"),
        });
    }
    if p == "meta/bases" {
        return ok(match req.query.get("offset") {
            None => {
                json!({ "bases": [{ "id": BASE, "name": "Product", "permissionLevel": "create" }], "offset": "b2" })
            }
            Some(_) => {
                json!({ "bases": [{ "id": "appBBBBBBBBBBBBBB", "name": "Research", "permissionLevel": "read" }] })
            }
        });
    }
    if p == format!("meta/bases/{BASE}/tables") {
        return ok(
            json!({ "tables": [{ "id": TABLE, "name": "Requests", "views": [{ "id": "viwAAAAAAAAAAAAAA", "name": "Grid" }, { "id": "viwBBBBBBBBBBBBBB", "name": "Open" }] }] }),
        );
    }
    if p == "meta/bases/appBBBBBBBBBBBBBB/tables" {
        return status(
            403,
            json!({ "error": { "type": "INVALID_PERMISSIONS", "message": "Missing scope" } }),
        );
    }
    status(
        404,
        json!({ "error": { "type": "NOT_FOUND", "message": "Not found" } }),
    )
}

#[test]
fn airtable_pulls_every_page_with_fields_in_first_seen_order() {
    let s = start(airtable);
    let (h, rows) = pull_spec(
        &format!("airtable:{BASE}/{TABLE}/viwBBBBBBBBBBBBBB"),
        "patgood",
    )
    .unwrap();
    assert_eq!(
        h,
        [
            "record_id",
            "created_time",
            "Title",
            "Votes",
            "Owner",
            "field_record_id",
            "Files",
            "Tags"
        ]
    );
    assert_eq!(rows.len(), 3);
    assert_eq!(col(&h, &rows, "Owner")[1], "Ada");
    assert_eq!(col(&h, &rows, "field_record_id")[1], "legacy-2");
    assert_eq!(col(&h, &rows, "Tags")[2], "sync, mobile");
    assert_eq!(col(&h, &rows, "Files")[2], "spec.pdf");
    assert!(
        s.seen()
            .iter()
            .all(|r| !r.path.ends_with(TABLE) || r.query["view"] == "viwBBBBBBBBBBBBBB"),
        "the view is passed on every page"
    );
}

#[test]
fn airtable_listing_says_when_the_scope_is_missing() {
    let _s = start(airtable);
    let e = choices("airtable", "patgood").unwrap_err().to_string();
    assert!(e.contains("schema.bases:read"), "{e}");
}

// ---- Typeform --------------------------------------------------------------------------------

fn typeform(req: &Request) -> Reply {
    if req.headers.get("authorization").map(String::as_str) != Some("Bearer tfp_good") {
        return status(
            403,
            json!({ "code": "AUTHENTICATION_FAILED", "description": "Authentication credentials not found" }),
        );
    }
    let p = req.path.trim_start_matches("api.typeform.com/");
    match p {
        "forms/abc123" => ok(json!({ "id": "abc123", "fields": [
            { "id": "q1", "title": "What is hard?", "type": "long_text" },
            { "id": "q2", "title": "How likely to recommend?", "type": "opinion_scale" },
            { "id": "g", "title": "More", "type": "group", "properties": { "fields": [
                { "id": "q3", "title": "Plan", "type": "multiple_choice" },
                { "id": "q4", "title": "What is hard?", "type": "short_text" } ] } } ],
            "hidden": ["source"] })),
        "forms/abc123/responses" => {
            assert_eq!(req.query["completed"], "true");
            let item = |n: usize| {
                json!({ "response_id": format!("r{n}"), "token": format!("t{n}"), "submitted_at": "2026-10-01T00:00:00Z",
                "hidden": { "source": "email" },
                "answers": [
                    { "field": { "id": "q1" }, "type": "text", "text": format!("Answer {n}") },
                    { "field": { "id": "q2" }, "type": "number", "number": 9 },
                    { "field": { "id": "q3" }, "type": "choice", "choice": { "label": "Pro" } } ] })
            };
            ok(match req.query.get("before").map(String::as_str) {
                None => {
                    json!({ "total_items": 1002, "items": (0..1000).map(item).collect::<Vec<_>>() })
                }
                Some("t999") => json!({ "total_items": 1002, "items": [item(1000), item(1001)] }),
                b => panic!("unexpected before {b:?}"),
            })
        }
        "forms" => ok(match req.query["page"].as_str() {
            "1" => {
                json!({ "page_count": 2, "items": [{ "id": "abc123", "title": "Exit survey" }] })
            }
            _ => json!({ "page_count": 2, "items": [{ "id": "def456", "title": "Onboarding" }] }),
        }),
        _ => status(
            404,
            json!({ "code": "FORM_NOT_FOUND", "description": "Not found" }),
        ),
    }
}

#[test]
fn typeform_pulls_every_completed_response() {
    let _s = start(typeform);
    let (h, rows) = pull_spec("typeform:abc123", "tfp_good").unwrap();
    assert_eq!(
        h,
        [
            "response_id",
            "submitted_at",
            "What is hard?",
            "How likely to recommend?",
            "Plan",
            "What is hard? (2)",
            "hidden_source"
        ]
    );
    assert_eq!(rows.len(), 1002, "paged with the last token");
    assert_eq!(rows[0][2..5], ["Answer 0", "9", "Pro"]);
    assert_eq!(rows[0][6], "email");
    let e = pull_spec("typeform:abc123", "tfp_bad")
        .unwrap_err()
        .to_string();
    assert!(e.contains("refused the token"), "{e}");
    let c = choices("typeform", "tfp_good").unwrap();
    assert_eq!(c.len(), 2, "both pages of forms");
}

// ---- Tally -----------------------------------------------------------------------------------

fn tally(req: &Request) -> Reply {
    if req.headers.get("authorization").map(String::as_str) != Some("Bearer tly-good") {
        return status(401, json!({ "message": "Unauthorized" }));
    }
    let p = req.path.trim_start_matches("api.tally.so/");
    match p {
        "forms/w4Kx9b/submissions" => {
            assert_eq!(req.query["filter"], "completed");
            let questions = json!([
                { "id": "q1", "title": "What would you improve?", "type": "TEXTAREA" },
                { "id": "q2", "title": "Plan", "type": "MULTIPLE_CHOICE", "fields": [{ "uuid": "f1", "options": [{ "id": "o1", "text": "Free" }, { "id": "o2", "text": "Pro" }] }] } ]);
            let sub = |n: usize| {
                json!({ "id": format!("s{n}"), "submittedAt": "2026-10-01T00:00:00Z", "isCompleted": true,
                "responses": [{ "questionId": "q1", "value": format!("Idea {n}") }, { "questionId": "q2", "value": ["o2"] }] })
            };
            ok(match req.query["page"].as_str() {
                "1" => {
                    json!({ "page": 1, "hasMore": true, "questions": questions, "submissions": (0..50).map(sub).collect::<Vec<_>>() })
                }
                "2" => {
                    json!({ "page": 2, "hasMore": false, "questions": questions, "submissions": [sub(50)] })
                }
                p => panic!("unexpected page {p}"),
            })
        }
        "forms" => ok(
            json!({ "items": [{ "id": "w4Kx9b", "name": "Feedback", "numberOfSubmissions": 51, "status": "PUBLISHED" }], "page": 1, "hasMore": false }),
        ),
        _ => status(404, json!({ "message": "Not found" })),
    }
}

#[test]
fn tally_pulls_every_completed_submission_with_option_labels() {
    let _s = start(tally);
    let (h, rows) = pull_spec("tally:w4Kx9b", "tly-good").unwrap();
    assert_eq!(
        h,
        [
            "submission_id",
            "submitted_at",
            "What would you improve?",
            "Plan"
        ]
    );
    assert_eq!(rows.len(), 51);
    assert_eq!(rows[50][2..], ["Idea 50", "Pro"]);
    let e = pull_spec("tally:w4Kx9b", "tly-bad")
        .unwrap_err()
        .to_string();
    assert!(e.contains("refused the API key"), "{e}");
    assert_eq!(choices("tally", "tly-good").unwrap()[0].count, Some(51));
}

// ---- Canny -----------------------------------------------------------------------------------

fn canny(req: &Request) -> Reply {
    if req.body["apiKey"] != "canny-good-key" {
        return ok(json!({ "error": "invalid api key" }));
    }
    let p = req.path.trim_start_matches("canny.io/api/v1/");
    match p {
        "boards/list" => ok(
            json!({ "boards": [{ "id": "b1", "name": "Feature Requests", "urlName": "feature-requests", "postCount": 120 }, { "id": "b2", "name": "Bugs", "urlName": "bugs", "postCount": 3 }] }),
        ),
        "posts/list" => {
            let skip = req.body["skip"].as_u64().unwrap() as usize;
            let n = if req.body.get("boardID").is_some() {
                assert_eq!(req.body["boardID"], "b1");
                120
            } else {
                123
            };
            let page: Vec<Value> = (skip..(skip + 100).min(n))
                .map(|i| json!({ "id": format!("p{i}"), "title": format!("Post {i}"), "details": "Please add", "status": "open",
                    "score": i, "commentCount": 1, "board": { "name": "Feature Requests" }, "category": null, "tags": [],
                    "author": { "name": "Ada" }, "created": "2026-01-01T00:00:00.000Z", "eta": null, "url": format!("https://acme.canny.io/p/{i}") }))
                .collect();
            ok(json!({ "posts": page, "hasMore": skip + 100 < n }))
        }
        _ => status(404, json!({ "error": "not found" })),
    }
}

#[test]
fn canny_pulls_a_board_by_name_or_every_board() {
    let _s = start(canny);
    let (h, rows) = pull_spec("canny:board:Feature Requests", "canny-good-key").unwrap();
    assert_eq!(h[0], "post_id");
    assert_eq!(rows.len(), 120);
    let (_, rows) = pull_spec("canny:all", "canny-good-key").unwrap();
    assert_eq!(rows.len(), 123);
    let e = pull_spec("canny:board:Nope", "canny-good-key")
        .unwrap_err()
        .to_string();
    assert!(e.contains("Feature Requests, Bugs"), "{e}");
    let e = pull_spec("canny:all", "wrong").unwrap_err().to_string();
    assert!(e.contains("refused the API key"), "{e}");
    let c = choices("canny", "canny-good-key").unwrap();
    assert_eq!(c[0].count, Some(123));
    // A listed board pulls.
    let (_, rows) = pull_spec(&c[1].spec, "canny-good-key").unwrap();
    assert_eq!(rows.len(), 120);
}

// ---- Productboard ----------------------------------------------------------------------------

fn productboard(req: &Request) -> Reply {
    if req.headers.get("authorization").map(String::as_str) != Some("Bearer pb-good") {
        return status(401, json!({ "errors": [{ "detail": "Invalid token" }] }));
    }
    assert_eq!(req.headers["x-version"], "1");
    let p = req.path.trim_start_matches("api.productboard.com/");
    match p {
        "notes" => {
            let note = |n: usize| {
                json!({ "id": format!("n{n}"), "title": format!("Note {n}"), "content": "<p>Wants <b>SSO</b> &amp; SCIM</p><p>Soon</p>",
                "user": { "email": "ada@acme.com" }, "company": { "domain": "acme.com" }, "tags": ["enterprise"],
                "features": [{ "id": "f1", "importance": 1 }], "source": { "origin": "intercom" }, "createdAt": "2026-01-01T00:00:00Z",
                "displayUrl": format!("https://acme.productboard.com/notes/{n}") })
            };
            ok(match req.query.get("pageCursor") {
                None => {
                    json!({ "data": (0..100).map(note).collect::<Vec<_>>(), "pageCursor": "pc2", "totalResults": 101 })
                }
                Some(_) => json!({ "data": [note(100)], "pageCursor": null, "totalResults": 101 }),
            })
        }
        "features" => {
            let feat = |n: usize| {
                json!({ "id": format!("f{n}"), "name": format!("Feature {n}"), "description": "<p>Single sign-on</p>",
                "type": "feature", "status": { "name": "In progress" }, "parent": { "component": { "id": "c1" } },
                "timeframe": { "startDate": "2026-01-01", "endDate": "none" }, "archived": false,
                "links": { "html": format!("https://acme.productboard.com/f/{n}") } })
            };
            ok(match req.query.get("pageOffset") {
                None => {
                    json!({ "data": (0..100).map(feat).collect::<Vec<_>>(), "links": { "next": "https://api.productboard.com/features?pageLimit=100&pageOffset=100" } })
                }
                Some(_) => json!({ "data": [feat(100), feat(101)], "links": { "next": null } }),
            })
        }
        _ => status(404, json!({ "errors": [{ "detail": "Not found" }] })),
    }
}

#[test]
fn productboard_pulls_notes_and_features_across_pages() {
    let _s = start(productboard);
    let (h, rows) = pull_spec("productboard:notes", "pb-good").unwrap();
    assert_eq!(rows.len(), 101);
    assert_eq!(col(&h, &rows, "content")[0], "Wants SSO & SCIM\nSoon");
    assert_eq!(col(&h, &rows, "customer")[0], "ada@acme.com");
    assert_eq!(
        col(&h, &rows, "features")[0],
        "f1",
        "linked features are named by id when the note gives no name"
    );
    let (h, rows) = pull_spec("productboard:features", "pb-good").unwrap();
    assert_eq!(rows.len(), 102);
    assert_eq!(col(&h, &rows, "parent")[0], "c1");
    let e = pull_spec("productboard:notes", "pb-bad")
        .unwrap_err()
        .to_string();
    assert!(e.contains("refused the token"), "{e}");
}

// ---- Google Sheets ---------------------------------------------------------------------------

#[test]
fn google_sheets_explains_private_and_missing_sheets() {
    let _s = start(|req| {
        let id = req.path.split('/').nth(3).unwrap_or("");
        match id.trim_end_matches('0') {
            "public" => Reply {
                status: 200,
                body: "\u{feff}name,idea\nAda,\"Offline, please\"\n".into(),
                headers: vec![],
            },
            "private" => Reply {
                status: 200,
                body: "<html>Sign in</html>".into(),
                headers: vec![("Content-Type".into(), "text/html; charset=utf-8".into())],
            },
            _ => status(404, json!({})),
        }
    });
    let sheet = |id: &str| format!("https://docs.google.com/spreadsheets/d/{id:0<44}/edit");
    let (h, rows) = pull_spec(&sheet("public"), "").unwrap();
    assert_eq!(h, ["name", "idea"], "the byte-order mark is dropped");
    assert_eq!(rows, [["Ada", "Offline, please"]]);
    let e = pull_spec(&sheet("private"), "").unwrap_err().to_string();
    assert!(e.contains("Anyone with the link"), "{e}");
    let e = pull_spec(&sheet("missing"), "").unwrap_err().to_string();
    assert!(e.contains("no sheet at this link"), "{e}");
}
