//! Typeform Responses API with a personal access token (scopes `forms:read`,
//! `responses:read`). One row per completed response: `response_id`, `submitted_at`, one column
//! per question (in form order; questions inside groups included), then hidden fields.

use super::{Paced, PullContext, fmt_number};
use anyhow::{Result, bail};
use serde_json::Value;
use std::time::Duration;

const API: &str = "https://api.typeform.com";

fn check(status: u16, v: &Value) -> Result<()> {
    let msg = v["description"]
        .as_str()
        .or(v["message"].as_str())
        .unwrap_or("no message");
    match status {
        200 => Ok(()),
        401 | 403 => bail!(
            "Typeform refused the token: create one with forms:read and responses:read in \
             Integrations ({msg})"
        ),
        404 => bail!("Typeform has no form with this id in your account ({msg})"),
        _ => bail!("Typeform answered HTTP {status}: {msg}"),
    }
}

/// Questions in form order, groups flattened: `(field id, title)`.
fn questions(fields: &Value, out: &mut Vec<(String, String)>) {
    for f in fields.as_array().into_iter().flatten() {
        let nested = &f["properties"]["fields"];
        if nested.is_array() {
            questions(nested, out);
        } else if let Some(id) = f["id"].as_str() {
            let title = f["title"].as_str().unwrap_or(id).trim().to_string();
            out.push((id.to_string(), title));
        }
    }
}

pub(super) fn responses(
    form: &str,
    token: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut t = Paced::new(Duration::from_millis(600));
    let auth = format!("Bearer {token}");
    (cx.progress)("reading form", 0, 0);
    let (status, def) = t.get(&format!("{API}/forms/{form}"), &[("Authorization", &auth)])?;
    check(status, &def)?;
    let mut qs = Vec::new();
    questions(&def["fields"], &mut qs);
    let hidden: Vec<String> = def["hidden"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|h| h.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();

    let mut items: Vec<Value> = Vec::new();
    let mut before: Option<String> = None;
    loop {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let mut url = format!("{API}/forms/{form}/responses?page_size=1000&completed=true");
        if let Some(b) = &before {
            url.push_str(&format!("&before={b}"));
        }
        let (status, v) = t.get(&url, &[("Authorization", &auth)])?;
        check(status, &v)?;
        let page = v["items"].as_array().cloned().unwrap_or_default();
        let done = page.len() < 1000;
        before = page
            .last()
            .and_then(|i| i["token"].as_str())
            .map(String::from);
        items.extend(page);
        (cx.progress)(
            "reading responses",
            items.len(),
            v["total_items"].as_u64().unwrap_or(0) as usize,
        );
        if done || before.is_none() {
            break;
        }
    }

    let mut columns = vec!["response_id".to_string(), "submitted_at".to_string()];
    let mut seen = std::collections::HashMap::<String, usize>::new();
    for (_, title) in &qs {
        let n = seen.entry(title.clone()).or_insert(0);
        *n += 1;
        columns.push(if *n == 1 {
            title.clone()
        } else {
            format!("{title} ({n})")
        });
    }
    columns.extend(hidden.iter().map(|h| format!("hidden_{h}")));
    let rows = items
        .iter()
        .map(|item| {
            let mut row = vec![
                item["response_id"].as_str().unwrap_or_default().to_string(),
                item["submitted_at"]
                    .as_str()
                    .unwrap_or_default()
                    .to_string(),
            ];
            let answers = item["answers"].as_array();
            for (id, _) in &qs {
                let a = answers.and_then(|a| {
                    a.iter()
                        .find(|x| x["field"]["id"].as_str() == Some(id.as_str()))
                });
                row.push(a.map(answer_text).unwrap_or_default());
            }
            for h in &hidden {
                row.push(item["hidden"][h].as_str().unwrap_or_default().to_string());
            }
            row
        })
        .collect();
    Ok((columns, rows))
}

fn answer_text(a: &Value) -> String {
    let t = a["type"].as_str().unwrap_or("");
    let v = &a[t];
    match t {
        "choice" => v["label"]
            .as_str()
            .or(v["other"].as_str())
            .unwrap_or_default()
            .to_string(),
        "choices" => {
            let mut labels: Vec<String> = v["labels"]
                .as_array()
                .map(|l| {
                    l.iter()
                        .filter_map(|x| x.as_str().map(String::from))
                        .collect()
                })
                .unwrap_or_default();
            if let Some(o) = v["other"].as_str() {
                labels.push(o.to_string());
            }
            labels.join(", ")
        }
        "payment" => v["amount"].as_str().unwrap_or_default().to_string(),
        _ => match v {
            Value::String(s) => s.clone(),
            Value::Number(n) => n.as_f64().map(fmt_number).unwrap_or_default(),
            Value::Bool(b) => b.to_string(),
            _ => String::new(),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn answers_and_grouped_questions_flatten() {
        let mut qs = Vec::new();
        questions(
            &json!([
                {"id": "a", "title": "What is hard?"},
                {"id": "g", "title": "Group", "properties": {"fields": [{"id": "b", "title": "Why?"}]}}
            ]),
            &mut qs,
        );
        assert_eq!(
            qs,
            vec![
                ("a".into(), "What is hard?".into()),
                ("b".into(), "Why?".into())
            ]
        );
        assert_eq!(
            answer_text(&json!({"type": "text", "text": "Billing"})),
            "Billing"
        );
        assert_eq!(answer_text(&json!({"type": "number", "number": 4})), "4");
        assert_eq!(
            answer_text(&json!({"type": "boolean", "boolean": true})),
            "true"
        );
        assert_eq!(
            answer_text(&json!({"type": "choice", "choice": {"label": "Weekly"}})),
            "Weekly"
        );
        assert_eq!(
            answer_text(
                &json!({"type": "choices", "choices": {"labels": ["A", "B"], "other": "C"}})
            ),
            "A, B, C"
        );
    }
}
