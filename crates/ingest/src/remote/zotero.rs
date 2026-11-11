//! Zotero Web API v3 (`api.zotero.org`) with an API key from zotero.org/settings/keys.
//! One row per top-level item (notes and attachments are skipped); creators, tags and
//! collections are joined with "; ".

use super::{Paced, PullContext, fmt_number};
use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.zotero.org";
const PAGE: usize = 100;

const COLUMNS: &[&str] = &[
    "key",
    "item_type",
    "title",
    "abstract",
    "creators",
    "date",
    "publication",
    "doi",
    "url",
    "tags",
    "collections",
    "date_added",
    "date_modified",
    "extra",
];

fn check(status: u16, v: &Value, what: &str) -> Result<()> {
    match status {
        200 => Ok(()),
        403 => bail!(
            "Zotero refused the API key for {what}: check it in Integrations, and that it can \
             read this library (for a group, allow group access when creating the key)"
        ),
        404 => bail!("Zotero has no {what} (check the collection or group link)"),
        _ => bail!(
            "Zotero answered HTTP {status} for {what}: {}",
            v.as_str().unwrap_or("")
        ),
    }
}

/// `users/<id>` for the key's own library, or `groups/<id>`.
fn library_path(z: &mut Paced, library: &str, key: &str) -> Result<String> {
    if let Some(id) = library.strip_prefix("group:") {
        return Ok(format!("groups/{id}"));
    }
    let (status, v) = z.get(
        &format!("{API}/keys/current"),
        &[("Zotero-API-Key", key), ("Zotero-API-Version", "3")],
    )?;
    check(status, &v, "this key")?;
    let id = v["userID"]
        .as_u64()
        .context("Zotero did not say which library this key belongs to")?;
    Ok(format!("users/{id}"))
}

pub(super) fn items(
    library: &str,
    collection: Option<&str>,
    key: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut z = Paced::new(Duration::from_millis(150));
    (cx.progress)("reading library", 0, 0);
    let lib = library_path(&mut z, library, key)?;
    let scope = match collection {
        Some(c) => format!("{lib}/collections/{c}/items/top"),
        None => format!("{lib}/items/top"),
    };
    let mut rows = Vec::new();
    let mut start = 0usize;
    loop {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let url = format!("{API}/{scope}?format=json&include=data&limit={PAGE}&start={start}");
        let (status, v) = z.get(
            &url,
            &[("Zotero-API-Key", key), ("Zotero-API-Version", "3")],
        )?;
        check(status, &v, "this library")?;
        let page = v.as_array().cloned().unwrap_or_default();
        for item in &page {
            if let Some(row) = item_row(&item["data"]) {
                rows.push(row);
            }
        }
        (cx.progress)("reading items", rows.len(), 0);
        if page.len() < PAGE {
            break;
        }
        start += PAGE;
    }
    Ok((COLUMNS.iter().map(|c| c.to_string()).collect(), rows))
}

fn item_row(d: &Value) -> Option<Vec<String>> {
    let item_type = d["itemType"].as_str().unwrap_or("");
    if matches!(item_type, "note" | "attachment" | "annotation") {
        return None;
    }
    let s = |k: &str| match &d[k] {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(fmt_number).unwrap_or_default(),
        _ => String::new(),
    };
    let creators = d["creators"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(
                    |c| match (c["lastName"].as_str(), c["firstName"].as_str()) {
                        (Some(l), Some(f)) if !f.is_empty() => Some(format!("{l}, {f}")),
                        (Some(l), _) => Some(l.to_string()),
                        _ => c["name"].as_str().map(String::from),
                    },
                )
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default();
    let tags = d["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t["tag"].as_str())
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default();
    let collections = d["collections"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(Value::as_str)
                .collect::<Vec<_>>()
                .join("; ")
        })
        .unwrap_or_default();
    // The venue lives in a different field per item type.
    let publication = [
        "publicationTitle",
        "bookTitle",
        "proceedingsTitle",
        "websiteTitle",
        "university",
        "publisher",
    ]
    .iter()
    .map(|k| s(k))
    .find(|v| !v.is_empty())
    .unwrap_or_default();
    Some(vec![
        s("key"),
        item_type.to_string(),
        s("title"),
        s("abstractNote"),
        creators,
        s("date"),
        publication,
        s("DOI"),
        s("url"),
        tags,
        collections,
        s("dateAdded"),
        s("dateModified"),
        s("extra"),
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn items_flatten_and_notes_are_skipped() {
        let row = item_row(&json!({
            "key": "ABCD1234", "itemType": "journalArticle", "title": "Ortho-k and myopia",
            "abstractNote": "Slows axial growth.", "date": "2021-03",
            "creators": [{"creatorType": "author", "lastName": "Lee", "firstName": "A."},
                         {"creatorType": "author", "name": "Myopia Study Group"}],
            "publicationTitle": "Optometry", "DOI": "10.1/x", "url": "",
            "tags": [{"tag": "myopia"}, {"tag": "ortho-k", "type": 1}],
            "collections": ["COLL0001"], "dateAdded": "2024-01-01T00:00:00Z",
            "dateModified": "2024-02-01T00:00:00Z", "extra": ""
        }))
        .unwrap();
        assert_eq!(row.len(), COLUMNS.len());
        assert_eq!(row[2], "Ortho-k and myopia");
        assert_eq!(row[4], "Lee, A.; Myopia Study Group");
        assert_eq!(row[6], "Optometry");
        assert_eq!(row[9], "myopia; ortho-k");
        assert!(item_row(&json!({"itemType": "note", "note": "x"})).is_none());
        let book = item_row(&json!({"itemType": "bookSection", "bookTitle": "Handbook"})).unwrap();
        assert_eq!(
            book[6], "Handbook",
            "the venue field depends on the item type"
        );
    }
}
