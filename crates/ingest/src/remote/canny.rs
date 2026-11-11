//! Canny API (v1) with the account's secret API key (Settings → API). One row per post:
//! a feature request with its status, votes, board, category, tags and author.

use super::{Paced, PullContext, fmt_number};
use anyhow::{Context, Result, bail};
use serde_json::{Value, json};
use std::time::Duration;

const API: &str = "https://canny.io/api/v1";
const PAGE: usize = 100;

const COLUMNS: &[&str] = &[
    "post_id", "title", "details", "status", "votes", "comments", "board", "category", "tags",
    "author", "created", "eta", "url",
];

fn check(status: u16, v: &Value) -> Result<()> {
    if status == 200 && v.get("error").is_none() {
        return Ok(());
    }
    let msg = v["error"].as_str().unwrap_or("no message");
    if status == 401 || status == 403 || msg.contains("invalid api key") {
        bail!("Canny refused the API key: check it in Integrations ({msg})");
    }
    bail!("Canny answered HTTP {status}: {msg}")
}

/// A board given by id, URL name or name → its id.
fn board_id(c: &mut Paced, key: &str, board: &str) -> Result<String> {
    let (status, v) = c.post_json(&format!("{API}/boards/list"), &json!({ "apiKey": key }))?;
    check(status, &v)?;
    let boards = v["boards"].as_array().context("Canny returned no boards")?;
    let wanted = board.to_lowercase();
    boards
        .iter()
        .find(|b| {
            ["id", "urlName", "name"]
                .iter()
                .any(|k| b[*k].as_str().is_some_and(|s| s.to_lowercase() == wanted))
        })
        .and_then(|b| b["id"].as_str().map(String::from))
        .with_context(|| {
            let names: Vec<&str> = boards.iter().filter_map(|b| b["name"].as_str()).collect();
            format!("Canny has no board {board:?}; boards: {}", names.join(", "))
        })
}

pub(super) fn posts(
    board: Option<&str>,
    key: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut c = Paced::new(Duration::from_millis(300));
    (cx.progress)("reading boards", 0, 0);
    let board = board.map(|b| board_id(&mut c, key, b)).transpose()?;
    let mut rows = Vec::new();
    let mut skip = 0usize;
    loop {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let mut body = json!({ "apiKey": key, "limit": PAGE, "skip": skip, "sort": "newest" });
        if let Some(b) = &board {
            body["boardID"] = json!(b);
        }
        let (status, v) = c.post_json(&format!("{API}/posts/list"), &body)?;
        check(status, &v)?;
        let page = v["posts"].as_array().cloned().unwrap_or_default();
        rows.extend(page.iter().map(post_row));
        (cx.progress)("reading posts", rows.len(), 0);
        if v["hasMore"].as_bool() != Some(true) || page.is_empty() {
            break;
        }
        skip += page.len();
    }
    Ok((COLUMNS.iter().map(|c| c.to_string()).collect(), rows))
}

fn post_row(p: &Value) -> Vec<String> {
    let s = |v: &Value| match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(fmt_number).unwrap_or_default(),
        _ => String::new(),
    };
    let tags = p["tags"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|t| t["name"].as_str())
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default();
    vec![
        s(&p["id"]),
        s(&p["title"]),
        s(&p["details"]),
        s(&p["status"]),
        s(&p["score"]),
        s(&p["commentCount"]),
        s(&p["board"]["name"]),
        s(&p["category"]["name"]),
        tags,
        s(&p["author"]["name"]),
        s(&p["created"]),
        s(&p["eta"]),
        s(&p["url"]),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posts_flatten() {
        let row = post_row(&json!({
            "id": "p1", "title": "Dark mode", "details": "Please", "status": "planned",
            "score": 42, "commentCount": 3, "board": {"name": "Features"},
            "category": null, "tags": [{"name": "ui"}, {"name": "theme"}],
            "author": {"name": "Ada"}, "created": "2025-01-01T00:00:00.000Z", "eta": null,
            "url": "https://acme.canny.io/features/p/dark-mode"
        }));
        assert_eq!(row.len(), COLUMNS.len());
        assert_eq!(row[1], "Dark mode");
        assert_eq!(row[4], "42");
        assert_eq!(row[6], "Features");
        assert_eq!(row[7], "");
        assert_eq!(row[8], "ui, theme");
    }
}
