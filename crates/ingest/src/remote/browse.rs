//! What an integration's token can pull, listed so the person picks instead of pasting a link:
//! Notion databases, Zotero libraries and collections, Airtable tables and views, Typeform and
//! Tally forms, Canny boards. Network use like a pull: the caller runs it only on the person's
//! request, and offline mode refuses it. Each choice carries the spec a pull takes.

use super::{Notion, Paced, agent, plain};
use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::time::Duration;

/// Listing stops after this many choices; the link field still reaches the rest.
const MAX_CHOICES: usize = 500;
/// Airtable bases read for their tables (one request each).
const MAX_AIRTABLE_BASES: usize = 40;

#[derive(Clone, Debug, Default, PartialEq, Serialize)]
pub struct Choice {
    /// What [`super::pull`] takes (through `Source::parse_remote`).
    pub spec: String,
    pub title: String,
    /// The library, base or workspace it belongs to, when there is more than one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Items, responses or posts, when the service says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub count: Option<u64>,
    /// Last edit (RFC 3339), when the service says.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edited: Option<String>,
    /// A smaller part of the item above it (a collection inside a collection, a view of a table).
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub nested: bool,
}

/// Everything `token` can pull from `integration`.
pub fn choices(integration: &str, token: &str) -> Result<Vec<Choice>> {
    invectus_core::net::ensure_online(&format!("listing what {integration} can pull"))?;
    let mut out = match integration {
        "notion" => notion(token)?,
        "zotero" => zotero(token)?,
        "airtable" => airtable(token)?,
        "typeform" => typeform(token)?,
        "tally" => tally(token)?,
        "canny" => canny(token)?,
        "productboard" => productboard(),
        other => bail!("{other} has nothing to list"),
    };
    out.truncate(MAX_CHOICES);
    Ok(out)
}

fn productboard() -> Vec<Choice> {
    vec![
        Choice {
            spec: "productboard:notes".into(),
            title: "Notes".into(),
            group: Some("Customer feedback".into()),
            ..Choice::default()
        },
        Choice {
            spec: "productboard:features".into(),
            title: "Features".into(),
            group: Some("The roadmap".into()),
            ..Choice::default()
        },
    ]
}

fn text(v: &Value) -> Option<String> {
    v.as_str()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(String::from)
}

// ---- Notion: databases shared with the integration ----------------------------------------

fn notion(token: &str) -> Result<Vec<Choice>> {
    let mut n = Notion {
        agent: agent(),
        token,
        last: None,
    };
    let mut out = Vec::new();
    let mut cursor: Option<String> = None;
    loop {
        let mut body = json!({
            "filter": { "property": "object", "value": "database" },
            "sort": { "timestamp": "last_edited_time", "direction": "descending" },
            "page_size": 100,
        });
        if let Some(c) = &cursor {
            body["start_cursor"] = json!(c);
        }
        let v = n.call("POST", "/search", Some(&body))?;
        out.extend(notion_choices(&v));
        cursor = match (v["has_more"].as_bool(), text(&v["next_cursor"])) {
            (Some(true), Some(c)) if out.len() < MAX_CHOICES => Some(c),
            _ => break,
        };
    }
    Ok(out)
}

fn notion_choices(v: &Value) -> Vec<Choice> {
    v["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|r| r["object"] == "database" && r["archived"] != true && r["in_trash"] != true)
        .filter_map(|r| {
            let id = r["id"].as_str()?.replace('-', "");
            let title = plain(&r["title"]);
            Some(Choice {
                spec: format!("notion:{id}"),
                title: if title.trim().is_empty() {
                    "Untitled database".into()
                } else {
                    title.trim().to_string()
                },
                edited: text(&r["last_edited_time"]),
                ..Choice::default()
            })
        })
        .collect()
}

// ---- Zotero: the key's libraries and their collections ------------------------------------

const ZOTERO: &str = "https://api.zotero.org";
const ZOTERO_PAGE: usize = 100;

fn zotero(key: &str) -> Result<Vec<Choice>> {
    let mut z = Paced::new(Duration::from_millis(150));
    let headers = [("Zotero-API-Key", key), ("Zotero-API-Version", "3")];
    let get = |z: &mut Paced, url: &str| -> Result<Value> {
        let (status, v) = z.get(url, &headers)?;
        match status {
            200 => Ok(v),
            403 => bail!("Zotero refused the API key: check it in Integrations"),
            _ => bail!(
                "Zotero answered HTTP {status}: {}",
                v.as_str().unwrap_or("")
            ),
        }
    };
    let me = get(&mut z, &format!("{ZOTERO}/keys/current"))?;
    let user = me["userID"]
        .as_u64()
        .context("Zotero did not say which library this key belongs to")?;
    let mut libraries: Vec<(String, String, String)> = Vec::new(); // (path, spec prefix, name)
    if me["access"]["user"]["library"] == true {
        libraries.push((
            format!("users/{user}"),
            "zotero:".into(),
            "My Library".into(),
        ));
    }
    if me["access"].get("groups").is_some() {
        let groups = get(&mut z, &format!("{ZOTERO}/users/{user}/groups?format=json"))?;
        for g in groups.as_array().into_iter().flatten() {
            if let (Some(id), Some(name)) = (g["id"].as_u64(), text(&g["data"]["name"])) {
                libraries.push((format!("groups/{id}"), format!("zotero:group:{id}/"), name));
            }
        }
    }
    let mut out = Vec::new();
    for (path, prefix, name) in &libraries {
        let mut cols = Vec::new();
        let mut start = 0;
        loop {
            let page = get(
                &mut z,
                &format!(
                    "{ZOTERO}/{path}/collections?format=json&limit={ZOTERO_PAGE}&start={start}"
                ),
            )?;
            let page = page.as_array().cloned().unwrap_or_default();
            let n = page.len();
            cols.extend(page);
            if n < ZOTERO_PAGE || cols.len() >= MAX_CHOICES {
                break;
            }
            start += ZOTERO_PAGE;
        }
        out.extend(zotero_choices(prefix, name, &cols));
    }
    Ok(out)
}

/// A library's whole-library choice, then its collections in tree order (parents first).
fn zotero_choices(prefix: &str, library: &str, collections: &[Value]) -> Vec<Choice> {
    let whole = if prefix == "zotero:" {
        "zotero:library".to_string()
    } else {
        prefix.trim_end_matches('/').to_string()
    };
    let mut out = vec![Choice {
        spec: whole,
        title: "Whole library".into(),
        group: Some(library.to_string()),
        ..Choice::default()
    }];
    let mut children: HashMap<Option<String>, Vec<&Value>> = HashMap::new();
    for c in collections {
        let parent = text(&c["data"]["parentCollection"]);
        children.entry(parent).or_default().push(c);
    }
    for list in children.values_mut() {
        list.sort_by_key(|c| c["data"]["name"].as_str().unwrap_or("").to_lowercase());
    }
    fn walk(
        parent: Option<String>,
        trail: &str,
        children: &HashMap<Option<String>, Vec<&Value>>,
        prefix: &str,
        library: &str,
        out: &mut Vec<Choice>,
    ) {
        for c in children.get(&parent).into_iter().flatten() {
            let Some(key) = text(&c["key"]) else { continue };
            let name = text(&c["data"]["name"]).unwrap_or_else(|| "Untitled".into());
            let title = if trail.is_empty() {
                name.clone()
            } else {
                format!("{trail} / {name}")
            };
            out.push(Choice {
                spec: format!("{prefix}collection:{key}"),
                title: title.clone(),
                group: Some(library.to_string()),
                count: c["meta"]["numItems"].as_u64(),
                nested: !trail.is_empty(),
                ..Choice::default()
            });
            walk(Some(key), &title, children, prefix, library, out);
        }
    }
    walk(None, "", &children, prefix, library, &mut out);
    out
}

// ---- Airtable: bases → tables → views (needs the schema.bases:read scope) ------------------

const AIRTABLE: &str = "https://api.airtable.com/v0";

fn airtable(token: &str) -> Result<Vec<Choice>> {
    let mut a = Paced::new(Duration::from_millis(220));
    let auth = format!("Bearer {token}");
    let mut get = |url: &str| -> Result<Value> {
        let (status, v) = a.get(url, &[("Authorization", &auth)])?;
        if status == 200 {
            return Ok(v);
        }
        let msg = v["error"]["message"]
            .as_str()
            .or(v["error"].as_str())
            .unwrap_or("no message");
        bail!(match status {
            401 => "Airtable refused the token: check it in Integrations".to_string(),
            403 => format!(
                "Airtable will not list your bases: give your token the schema.bases:read scope, \
                 or paste a table link ({msg})"
            ),
            _ => format!("Airtable answered HTTP {status}: {msg}"),
        })
    };
    let mut bases = Vec::new();
    let mut offset: Option<String> = None;
    loop {
        let url = match &offset {
            Some(o) => format!("{AIRTABLE}/meta/bases?offset={o}"),
            None => format!("{AIRTABLE}/meta/bases"),
        };
        let v = get(&url)?;
        bases.extend(v["bases"].as_array().cloned().unwrap_or_default());
        offset = text(&v["offset"]);
        if offset.is_none() || bases.len() >= MAX_AIRTABLE_BASES {
            break;
        }
    }
    let mut out = Vec::new();
    for b in bases.iter().take(MAX_AIRTABLE_BASES) {
        let Some(id) = text(&b["id"]) else { continue };
        let name = text(&b["name"]).unwrap_or_else(|| id.clone());
        let tables = get(&format!("{AIRTABLE}/meta/bases/{id}/tables"))?;
        out.extend(airtable_choices(&id, &name, &tables));
        if out.len() >= MAX_CHOICES {
            break;
        }
    }
    Ok(out)
}

/// Each table, then its views other than the first (the first view is the whole table).
fn airtable_choices(base: &str, base_name: &str, tables: &Value) -> Vec<Choice> {
    let mut out = Vec::new();
    for t in tables["tables"].as_array().into_iter().flatten() {
        let (Some(id), Some(name)) = (text(&t["id"]), text(&t["name"])) else {
            continue;
        };
        out.push(Choice {
            spec: format!("airtable:{base}/{id}"),
            title: name.clone(),
            group: Some(base_name.to_string()),
            ..Choice::default()
        });
        for v in t["views"].as_array().into_iter().flatten().skip(1) {
            if let (Some(vid), Some(vname)) = (text(&v["id"]), text(&v["name"])) {
                out.push(Choice {
                    spec: format!("airtable:{base}/{id}/{vid}"),
                    title: format!("{name} / {vname}"),
                    group: Some(base_name.to_string()),
                    nested: true,
                    ..Choice::default()
                });
            }
        }
    }
    out
}

// ---- Typeform and Tally: forms --------------------------------------------------------------

fn typeform(token: &str) -> Result<Vec<Choice>> {
    let mut t = Paced::new(Duration::from_millis(500));
    let auth = format!("Bearer {token}");
    let mut out = Vec::new();
    for page in 1.. {
        let (status, v) = t.get(
            &format!("https://api.typeform.com/forms?page_size=200&page={page}"),
            &[("Authorization", &auth)],
        )?;
        if status != 200 {
            let msg = v["description"].as_str().unwrap_or("no message");
            bail!(match status {
                401 | 403 =>
                    format!("Typeform refused the token: it needs the forms:read scope ({msg})"),
                _ => format!("Typeform answered HTTP {status}: {msg}"),
            });
        }
        out.extend(typeform_choices(&v));
        let pages = v["page_count"].as_u64().unwrap_or(1);
        if page >= pages || out.len() >= MAX_CHOICES {
            break;
        }
    }
    Ok(out)
}

fn typeform_choices(v: &Value) -> Vec<Choice> {
    v["items"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|f| {
            let id = text(&f["id"])?;
            Some(Choice {
                spec: format!("typeform:{id}"),
                title: text(&f["title"]).unwrap_or_else(|| "Untitled form".into()),
                edited: text(&f["last_updated_at"]),
                ..Choice::default()
            })
        })
        .collect()
}

fn tally(token: &str) -> Result<Vec<Choice>> {
    let mut t = Paced::new(Duration::from_millis(250));
    let auth = format!("Bearer {token}");
    let mut out = Vec::new();
    for page in 1.. {
        let (status, v) = t.get(
            &format!("https://api.tally.so/forms?page={page}"),
            &[("Authorization", &auth)],
        )?;
        if status != 200 {
            let msg = v["message"].as_str().unwrap_or("no message");
            bail!(match status {
                401 | 403 => format!("Tally refused the API key: check it in Integrations ({msg})"),
                _ => format!("Tally answered HTTP {status}: {msg}"),
            });
        }
        out.extend(tally_choices(&v));
        if v["hasMore"].as_bool() != Some(true) || out.len() >= MAX_CHOICES {
            break;
        }
    }
    Ok(out)
}

fn tally_choices(v: &Value) -> Vec<Choice> {
    let items = v["items"].as_array().or(v.as_array());
    items
        .into_iter()
        .flatten()
        .filter(|f| f["status"] != "DELETED")
        .filter_map(|f| {
            let id = text(&f["id"])?;
            Some(Choice {
                spec: format!("tally:{id}"),
                title: text(&f["name"]).unwrap_or_else(|| "Untitled form".into()),
                count: f["numberOfSubmissions"].as_u64(),
                edited: text(&f["updatedAt"]),
                ..Choice::default()
            })
        })
        .collect()
}

// ---- Canny: boards --------------------------------------------------------------------------

fn canny(key: &str) -> Result<Vec<Choice>> {
    let mut c = Paced::new(Duration::from_millis(300));
    let (status, v) = c.post_json(
        "https://canny.io/api/v1/boards/list",
        &json!({ "apiKey": key }),
    )?;
    if status != 200 || v.get("error").is_some() {
        let msg = v["error"].as_str().unwrap_or("no message");
        if matches!(status, 401 | 403) || msg.contains("invalid api key") {
            bail!("Canny refused the API key: check it in Integrations ({msg})");
        }
        bail!("Canny answered HTTP {status}: {msg}");
    }
    Ok(canny_choices(&v))
}

fn canny_choices(v: &Value) -> Vec<Choice> {
    let boards: Vec<&Value> = v["boards"].as_array().into_iter().flatten().collect();
    let mut out = vec![Choice {
        spec: "canny:all".into(),
        title: "All boards".into(),
        count: boards
            .iter()
            .map(|b| b["postCount"].as_u64())
            .sum::<Option<u64>>(),
        ..Choice::default()
    }];
    out.extend(boards.iter().filter_map(|b| {
        let id = text(&b["id"])?;
        Some(Choice {
            spec: format!("canny:board:{id}"),
            title: text(&b["name"]).unwrap_or_else(|| id.clone()),
            count: b["postCount"].as_u64(),
            nested: true,
            ..Choice::default()
        })
    }));
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::source::Source;
    use std::path::Path;

    /// Every listed spec must parse back into the source a pull takes.
    fn parses(c: &[Choice]) {
        for choice in c {
            let s = Source::parse_remote(&choice.spec, Path::new("/p")).unwrap();
            assert!(s.is_some(), "{} does not parse", choice.spec);
        }
    }

    #[test]
    fn notion_lists_databases_with_titles() {
        let v = json!({ "results": [
            { "object": "database", "id": "0123abcd-0123-abcd-0123-abcd0123abcd",
              "title": [{ "plain_text": "Reading list" }], "last_edited_time": "2026-11-01T10:00:00.000Z" },
            { "object": "database", "id": "1123abcd-0123-abcd-0123-abcd0123abcd", "title": [], "archived": true },
            { "object": "database", "id": "2123abcd-0123-abcd-0123-abcd0123abcd", "title": [] },
        ]});
        let c = notion_choices(&v);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].spec, "notion:0123abcd0123abcd0123abcd0123abcd");
        assert_eq!(c[0].title, "Reading list");
        assert_eq!(c[1].title, "Untitled database");
        parses(&c);
    }

    #[test]
    fn zotero_lists_collections_in_tree_order() {
        let cols = vec![
            json!({ "key": "BBBBBBBB", "data": { "name": "Methods", "parentCollection": "AAAAAAAA" }, "meta": { "numItems": 4 } }),
            json!({ "key": "AAAAAAAA", "data": { "name": "Glaucoma", "parentCollection": false }, "meta": { "numItems": 12 } }),
            json!({ "key": "CCCCCCCC", "data": { "name": "Amblyopia", "parentCollection": false } }),
        ];
        let c = zotero_choices("zotero:", "My Library", &cols);
        let titles: Vec<&str> = c.iter().map(|c| c.title.as_str()).collect();
        assert_eq!(
            titles,
            [
                "Whole library",
                "Amblyopia",
                "Glaucoma",
                "Glaucoma / Methods"
            ]
        );
        assert_eq!(c[0].spec, "zotero:library");
        assert_eq!(c[2].spec, "zotero:collection:AAAAAAAA");
        assert_eq!(c[2].count, Some(12));
        assert!(c[3].nested);
        parses(&c);
        let g = zotero_choices("zotero:group:4711/", "Lab", &cols);
        assert_eq!(g[0].spec, "zotero:group:4711");
        assert_eq!(g[1].spec, "zotero:group:4711/collection:CCCCCCCC");
        parses(&g);
    }

    #[test]
    fn airtable_lists_tables_then_their_other_views() {
        let v = json!({ "tables": [{ "id": "tblAAAAAAAAAAAAAA", "name": "Requests", "views": [
            { "id": "viwAAAAAAAAAAAAAA", "name": "Grid view" },
            { "id": "viwBBBBBBBBBBBBBB", "name": "Open only" } ] }] });
        let c = airtable_choices("appAAAAAAAAAAAAAA", "Product", &v);
        assert_eq!(c.len(), 2);
        assert_eq!(c[0].spec, "airtable:appAAAAAAAAAAAAAA/tblAAAAAAAAAAAAAA");
        assert_eq!(c[1].title, "Requests / Open only");
        parses(&c);
    }

    #[test]
    fn forms_and_boards_list() {
        let t = typeform_choices(
            &json!({ "items": [{ "id": "abc123", "title": "Exit survey", "last_updated_at": "2026-10-01T00:00:00Z" }] }),
        );
        assert_eq!(t[0].spec, "typeform:abc123");
        parses(&t);
        let l = tally_choices(&json!({ "items": [
            { "id": "w4Kx9b", "name": "Feedback", "numberOfSubmissions": 31 },
            { "id": "zzzzzz", "name": "Old", "status": "DELETED" } ], "hasMore": false }));
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].count, Some(31));
        parses(&l);
        let c = canny_choices(&json!({ "boards": [
            { "id": "5f1", "name": "Feature Requests", "postCount": 40 },
            { "id": "5f2", "name": "Bugs", "postCount": 2 } ] }));
        assert_eq!(c[0].spec, "canny:all");
        assert_eq!(c[0].count, Some(42));
        assert_eq!(c[2].spec, "canny:board:5f2");
        parses(&c);
        parses(&productboard());
    }
}
