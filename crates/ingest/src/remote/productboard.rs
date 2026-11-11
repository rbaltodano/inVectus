//! Productboard public API (v1) with an access token (Settings → Integrations → Public API).
//! `notes`: customer feedback (demand) with its customer, company, tags and linked features.
//! `features`: the roadmap (coverage) with status and timeframe. HTML descriptions become text.

use super::{Paced, PullContext};
use anyhow::{Result, bail};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.productboard.com";

const NOTE_COLUMNS: &[&str] = &[
    "note_id", "title", "content", "customer", "company", "tags", "features", "source", "created",
    "url",
];
const FEATURE_COLUMNS: &[&str] = &[
    "feature_id",
    "name",
    "description",
    "type",
    "status",
    "parent",
    "timeframe_start",
    "timeframe_end",
    "archived",
    "url",
];

pub(super) fn entities(
    entity: &str,
    token: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    // Productboard allows about 50 requests a second; stay well below.
    let mut p = Paced::new(Duration::from_millis(100));
    let auth = format!("Bearer {token}");
    let headers = [("Authorization", auth.as_str()), ("X-Version", "1")];
    let notes = entity == "notes";
    let mut url = Some(if notes {
        format!("{API}/notes?pageLimit=100")
    } else {
        format!("{API}/features?pageLimit=100")
    });
    let mut rows = Vec::new();
    while let Some(u) = url.take() {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let (status, v) = p.get(&u, &headers)?;
        if status != 200 {
            let msg = v["errors"][0]["detail"]
                .as_str()
                .or(v["message"].as_str())
                .unwrap_or("no message");
            bail!(match status {
                401 | 403 =>
                    format!("Productboard refused the token: check it in Integrations ({msg})"),
                _ => format!("Productboard answered HTTP {status}: {msg}"),
            });
        }
        for d in v["data"].as_array().into_iter().flatten() {
            rows.push(if notes { note_row(d) } else { feature_row(d) });
        }
        (cx.progress)(
            if notes {
                "reading notes"
            } else {
                "reading features"
            },
            rows.len(),
            0,
        );
        // Notes page with a cursor; features with a next link.
        url = match (v["pageCursor"].as_str(), v["links"]["next"].as_str()) {
            (Some(c), _) if notes && !c.is_empty() => {
                Some(format!("{API}/notes?pageLimit=100&pageCursor={c}"))
            }
            (_, Some(next)) if !next.is_empty() => Some(next.to_string()),
            _ => None,
        };
    }
    let columns = if notes { NOTE_COLUMNS } else { FEATURE_COLUMNS };
    Ok((columns.iter().map(|c| c.to_string()).collect(), rows))
}

fn s(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
        _ => String::new(),
    }
}

fn names(v: &Value, key: &str) -> String {
    v.as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    x.as_str()
                        .map(String::from)
                        .or_else(|| x[key].as_str().map(String::from))
                })
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn note_row(d: &Value) -> Vec<String> {
    let customer = d["user"]["name"]
        .as_str()
        .or(d["user"]["email"].as_str())
        .unwrap_or_default()
        .to_string();
    vec![
        s(&d["id"]),
        s(&d["title"]),
        html_text(d["content"].as_str().unwrap_or("")),
        customer,
        s(&d["company"]["domain"]),
        names(&d["tags"], "name"),
        names(&d["features"], "name"),
        s(&d["source"]["origin"]),
        s(&d["createdAt"]),
        s(&d["displayUrl"]),
    ]
}

fn feature_row(d: &Value) -> Vec<String> {
    let parent = ["feature", "component", "product"]
        .iter()
        .find_map(|k| d["parent"][k]["id"].as_str())
        .unwrap_or_default()
        .to_string();
    vec![
        s(&d["id"]),
        s(&d["name"]),
        html_text(d["description"].as_str().unwrap_or("")),
        s(&d["type"]),
        s(&d["status"]["name"]),
        parent,
        s(&d["timeframe"]["startDate"]),
        s(&d["timeframe"]["endDate"]),
        s(&d["archived"]),
        s(&d["links"]["html"]),
    ]
}

/// Text of an HTML fragment: tags dropped (block ends become line breaks), entities decoded.
fn html_text(html: &str) -> String {
    let mut out = String::new();
    let mut tag = String::new();
    let mut in_tag = false;
    for c in html.chars() {
        match c {
            '<' => {
                in_tag = true;
                tag.clear();
            }
            '>' if in_tag => {
                in_tag = false;
                let t = tag
                    .trim_start_matches('/')
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .to_lowercase();
                if matches!(
                    t.as_str(),
                    "p" | "br" | "li" | "div" | "h1" | "h2" | "h3" | "tr"
                ) {
                    out.push('\n');
                }
            }
            _ if in_tag => tag.push(c),
            _ => out.push(c),
        }
    }
    let decoded = out
        .replace("&nbsp;", " ")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&amp;", "&");
    decoded
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn notes_and_features_flatten() {
        assert_eq!(
            html_text("<p>Need <b>SSO</b> &amp; SCIM</p><p>Soon</p>"),
            "Need SSO & SCIM\nSoon"
        );
        let n = note_row(&json!({
            "id": "n1", "title": "SSO", "content": "<p>We need SSO</p>",
            "user": {"email": "a@x.com"}, "company": {"domain": "x.com"},
            "tags": ["enterprise", "auth"], "features": [{"id": "f1", "name": "SSO"}],
            "createdAt": "2025-01-01T00:00:00Z", "displayUrl": "https://pb/n1"
        }));
        assert_eq!(n.len(), NOTE_COLUMNS.len());
        assert_eq!(n[2], "We need SSO");
        assert_eq!(n[3], "a@x.com");
        assert_eq!(n[5], "enterprise, auth");
        assert_eq!(n[6], "SSO");
        let f = feature_row(&json!({
            "id": "f1", "name": "SSO", "description": "<p>Single sign-on</p>", "type": "feature",
            "status": {"name": "Planned"}, "parent": {"component": {"id": "c1"}},
            "timeframe": {"startDate": "2025-02-01", "endDate": "none"}, "archived": false,
            "links": {"html": "https://pb/f1"}
        }));
        assert_eq!(f.len(), FEATURE_COLUMNS.len());
        assert_eq!(
            (f[4].as_str(), f[5].as_str(), f[8].as_str()),
            ("Planned", "c1", "false")
        );
    }
}
