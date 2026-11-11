//! Tally API with an API key (Settings → API keys). One row per completed submission:
//! `submission_id`, `submitted_at`, one column per question. Choice answers given as option ids
//! are turned into their labels when the question lists its options.

use super::{Paced, PullContext, fmt_number};
use anyhow::{Result, bail};
use serde_json::Value;
use std::collections::HashMap;
use std::time::Duration;

const API: &str = "https://api.tally.so";

pub(super) fn submissions(
    form: &str,
    token: &str,
    cx: &mut PullContext,
) -> Result<(Vec<String>, Vec<Vec<String>>)> {
    let mut t = Paced::new(Duration::from_millis(250));
    let auth = format!("Bearer {token}");
    let mut questions: Vec<(String, String)> = Vec::new();
    let mut options: HashMap<String, String> = HashMap::new();
    let mut subs: Vec<Value> = Vec::new();
    let mut page = 1usize;
    loop {
        if cx.cancel.is_cancelled() {
            bail!("cancelled");
        }
        let url = format!("{API}/forms/{form}/submissions?page={page}&filter=completed");
        let (status, v) = t.get(&url, &[("Authorization", &auth)])?;
        if status != 200 {
            let msg = v["message"].as_str().unwrap_or("no message");
            bail!(match status {
                401 | 403 => format!("Tally refused the API key: check it in Integrations ({msg})"),
                404 => format!("Tally has no form with this id in your workspace ({msg})"),
                _ => format!("Tally answered HTTP {status}: {msg}"),
            });
        }
        if questions.is_empty() {
            for q in v["questions"].as_array().into_iter().flatten() {
                if let Some(id) = q["id"].as_str() {
                    let title = q["title"].as_str().unwrap_or(id).trim().to_string();
                    questions.push((id.to_string(), title));
                }
                collect_options(q, &mut options);
            }
        }
        subs.extend(v["submissions"].as_array().cloned().unwrap_or_default());
        (cx.progress)("reading submissions", subs.len(), 0);
        if v["hasMore"].as_bool() != Some(true) {
            break;
        }
        page += 1;
    }
    let mut columns = vec!["submission_id".to_string(), "submitted_at".to_string()];
    columns.extend(questions.iter().map(|(_, t)| t.clone()));
    let rows = subs
        .iter()
        .map(|s| {
            let mut row = vec![
                s["id"].as_str().unwrap_or_default().to_string(),
                s["submittedAt"].as_str().unwrap_or_default().to_string(),
            ];
            let responses = s["responses"].as_array();
            for (id, _) in &questions {
                let v = responses
                    .and_then(|r| {
                        r.iter()
                            .find(|x| x["questionId"].as_str() == Some(id.as_str()))
                    })
                    .map(|r| &r["value"]);
                row.push(v.map(|v| value_text(v, &options)).unwrap_or_default());
            }
            row
        })
        .collect();
    Ok((columns, rows))
}

/// Option id → label, from a question's fields or options (any depth).
fn collect_options(v: &Value, out: &mut HashMap<String, String>) {
    match v {
        Value::Object(o) => {
            if let (Some(id), Some(text)) = (
                o.get("id").and_then(Value::as_str),
                o.get("text").or(o.get("label")).and_then(Value::as_str),
            ) {
                out.insert(id.to_string(), text.to_string());
            }
            o.values().for_each(|x| collect_options(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| collect_options(x, out)),
        _ => {}
    }
}

fn value_text(v: &Value, options: &HashMap<String, String>) -> String {
    match v {
        Value::Null => String::new(),
        Value::String(s) => options.get(s).cloned().unwrap_or_else(|| s.clone()),
        Value::Number(n) => n.as_f64().map(fmt_number).unwrap_or_default(),
        Value::Bool(b) => b.to_string(),
        Value::Array(a) => a
            .iter()
            .map(|x| value_text(x, options))
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(", "),
        Value::Object(o) => ["name", "text", "label", "url", "value"]
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
    fn option_ids_become_labels() {
        let mut opts = HashMap::new();
        collect_options(
            &json!({"id": "q1", "title": "Plan", "fields": [{"uuid": "f", "options": [
                {"id": "o1", "text": "Free"}, {"id": "o2", "text": "Pro"}]}]}),
            &mut opts,
        );
        assert_eq!(value_text(&json!(["o1", "o2"]), &opts), "Free, Pro");
        assert_eq!(value_text(&json!("Plain answer"), &opts), "Plain answer");
        assert_eq!(value_text(&json!(7.0), &opts), "7");
        assert_eq!(
            value_text(&json!([{"name": "cv.pdf", "url": "https://x"}]), &opts),
            "cv.pdf"
        );
    }
}
