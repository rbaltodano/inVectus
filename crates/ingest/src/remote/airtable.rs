//! Airtable Web API (`api.airtable.com/v0`) with a personal access token (scope
//! `data.records:read`). One row per record: `record_id`, `created_time`, then each field in the
//! order first seen (Airtable leaves empty fields out of a record).

use super::{Paced, PullContext, fmt_number};
use anyhow::{Result, bail};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.airtable.com/v0";

pub(super) fn records(
    base: &str,
    table: &str,
    view: Option<&str>,
    token: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    // Airtable allows five requests a second per base.
    let mut a = Paced::new(Duration::from_millis(220));
    let auth = format!("Bearer {token}");
    let mut fields: Vec<String> = Vec::new();
    let mut records: Vec<(String, String, serde_json::Map<String, Value>)> = Vec::new();
    let mut offset: Option<String> = None;
    loop {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let mut url = format!("{API}/{base}/{table}?pageSize=100");
        if let Some(v) = view {
            url.push_str(&format!("&view={v}"));
        }
        if let Some(o) = &offset {
            url.push_str(&format!("&offset={o}"));
        }
        let (status, v) = a.get(&url, &[("Authorization", &auth)])?;
        if status != 200 {
            let msg = v["error"]["message"]
                .as_str()
                .or(v["error"].as_str())
                .unwrap_or("no message");
            bail!(match status {
                401 => "Airtable refused the token: check it in Integrations".to_string(),
                403 | 404 => format!(
                    "Airtable could not open this table. Give your token the \
                     data.records:read scope and access to this base ({msg})"
                ),
                _ => format!("Airtable answered HTTP {status}: {msg}"),
            });
        }
        for r in v["records"].as_array().into_iter().flatten() {
            let f = r["fields"].as_object().cloned().unwrap_or_default();
            for k in f.keys() {
                if !fields.contains(k) {
                    fields.push(k.clone());
                }
            }
            records.push((
                r["id"].as_str().unwrap_or_default().to_string(),
                r["createdTime"].as_str().unwrap_or_default().to_string(),
                f,
            ));
        }
        (cx.progress)("reading records", records.len(), 0);
        match v["offset"].as_str() {
            Some(o) => offset = Some(o.to_string()),
            None => break,
        }
    }
    let fixed = ["record_id", "created_time"];
    let mut columns: Vec<String> = fixed.iter().map(|s| s.to_string()).collect();
    columns.extend(fields.iter().map(|f| {
        if fixed.contains(&f.as_str()) {
            format!("field_{f}")
        } else {
            f.clone()
        }
    }));
    let rows = records
        .into_iter()
        .map(|(id, created, f)| {
            let mut row = vec![id, created];
            row.extend(
                fields
                    .iter()
                    .map(|k| f.get(k).map(value_text).unwrap_or_default()),
            );
            row
        })
        .collect();
    Ok((columns, rows))
}

/// A field value as text: lists joined with ", "; collaborators by name, attachments by file
/// name, AI text by its value; other objects as JSON.
fn value_text(v: &Value) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => s.clone(),
        Value::Number(n) => n.as_f64().map(fmt_number).unwrap_or_default(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => a
            .iter()
            .map(value_text)
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(o) => ["name", "filename", "value", "text", "email", "url"]
            .iter()
            .find_map(|k| o.get(*k).and_then(Value::as_str).map(String::from))
            .unwrap_or_else(|| v.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn field_values_flatten_to_text() {
        assert_eq!(value_text(&json!("x")), "x");
        assert_eq!(value_text(&json!(4.0)), "4");
        assert_eq!(value_text(&json!(["a", "b"])), "a, b");
        assert_eq!(value_text(&json!({"id": "usr1", "name": "Ada"})), "Ada");
        assert_eq!(
            value_text(&json!([{"id": "att1", "filename": "a.pdf", "url": "https://x"}])),
            "a.pdf"
        );
        assert_eq!(
            value_text(&json!({"state": "generated", "value": "AI text"})),
            "AI text"
        );
        assert_eq!(value_text(&json!(true)), "true");
        assert_eq!(value_text(&Value::Null), "");
    }
}
