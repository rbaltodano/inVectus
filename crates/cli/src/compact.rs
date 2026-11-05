//! Compact tool results for agents. Every result is re-read on each later turn of an agent's
//! conversation, so `invectus mcp` drops what an agent rarely needs: timings, hashes, full
//! texts, the 20-id cores and year-by-year timelines in gap lists, job history. Verdicts,
//! calibration quality, scope notes and evidence titles always stay. `detail: "full"` on any
//! call returns the complete result.

use serde_json::{Map, Value, json};

/// Characters of an item's text kept in a compact hit.
const TEXT_CHARS: usize = 200;
/// Hits of the underlying search kept inside a compact `check_idea`.
const CHECK_HITS: usize = 3;
/// Finished jobs kept in a compact project status.
const RECENT_JOBS: usize = 2;

/// The compact form of a tool's result; tools not listed here are returned as they are.
pub fn result(tool: &str, v: Value) -> Value {
    match tool {
        "search" => search(v, usize::MAX),
        "check_idea" => check_idea(v),
        "list_gaps" | "scan_gaps" => map_key(v, "gaps", |gaps| each(gaps, gap_summary)),
        "gap_detail" => gap_detail(v),
        "suggest_decision" => suggest_decision(v),
        "open_project" | "project_status" => status(v),
        "submit_description" => drop(v, &["context_titles", "llm"]),
        _ => v,
    }
}

fn drop(mut v: Value, keys: &[&str]) -> Value {
    if let Some(o) = v.as_object_mut() {
        for k in keys {
            o.remove(*k);
        }
    }
    v
}

fn map_key(mut v: Value, key: &str, f: impl FnOnce(Value) -> Value) -> Value {
    if let Some(o) = v.as_object_mut()
        && let Some(x) = o.remove(key)
    {
        o.insert(key.into(), f(x));
    }
    v
}

fn each(v: Value, f: impl Fn(Value) -> Value) -> Value {
    match v {
        Value::Array(a) => Value::Array(a.into_iter().map(f).collect()),
        other => other,
    }
}

/// Item ids are 64-bit; as JSON numbers they lose digits in JavaScript clients, so compact
/// results carry them as strings (the form `submit_paraphrases` takes).
fn id_string(v: &mut Value, key: &str) {
    if let Some(n) = v.get(key).and_then(Value::as_i64) {
        v[key] = Value::String(n.to_string());
    }
}

fn truncate(s: &str, n: usize) -> String {
    match s.char_indices().nth(n) {
        Some((i, _)) => format!("{}…", s[..i].trim_end()),
        None => s.to_string(),
    }
}

/// A hit: rank, similarity and the item's id, title, date, role, link and a text excerpt.
fn hit(mut h: Value) -> Value {
    let item = h["item"].take();
    let mut slim = Map::new();
    for k in ["item_id", "title", "date", "role", "url"] {
        if let Some(x) = item.get(k).filter(|x| !x.is_null()) {
            slim.insert(k.into(), x.clone());
        }
    }
    if let Some(t) = item["text"].as_str() {
        slim.insert("text".into(), truncate(t, TEXT_CHARS).into());
    }
    let mut slim = Value::Object(slim);
    id_string(&mut slim, "item_id");
    h["item"] = slim;
    if h["via_chunk"].is_null() {
        drop(h, &["via_chunk"])
    } else {
        h
    }
}

fn search(v: Value, hits: usize) -> Value {
    let v = drop(v, &["embed_ms", "retrieve_ms", "space_id", "model_key"]);
    map_key(v, "hits", |h| match h {
        Value::Array(a) => Value::Array(a.into_iter().take(hits).map(hit).collect()),
        other => other,
    })
}

fn check_idea(v: Value) -> Value {
    map_key(v, "search", |s| search(s, CHECK_HITS))
}

/// One line of figures per gap; cores, timelines and outside-source matches stay in
/// `gap_detail`.
fn gap_summary(g: Value) -> Value {
    let mut g = drop(g, &["core", "timeline", "bridge_items"]);
    if let Some(verification) = g.get_mut("verification").filter(|v| v.is_object()) {
        *verification = drop(verification.take(), &["closest"]);
    }
    if let Some(s) = g.get_mut("suggestion").filter(|s| s.is_object()) {
        *s = drop(s.take(), &["core", "collection_id", "space_id"]);
    }
    g
}

fn gap_detail(v: Value) -> Value {
    let mut v = drop(v, &["core"]);
    if let Some(items) = v.get_mut("core_items").and_then(Value::as_array_mut) {
        for i in items {
            id_string(i, "item_id");
        }
    }
    if let Some(s) = v.get_mut("suggestion").filter(|s| s.is_object()) {
        *s = drop(s.take(), &["core", "collection_id", "space_id"]);
    }
    // Years with neither demand nor coverage say nothing.
    map_key(v, "timeline", |t| match t {
        Value::Array(a) => Value::Array(
            a.into_iter()
                .filter(|y| y["demand"].as_i64() != Some(0) || y["coverage"].as_i64() != Some(0))
                .collect(),
        ),
        other => other,
    })
}

/// What was recorded, not the whole gap again.
fn suggest_decision(v: Value) -> Value {
    let mut out = drop(
        v,
        &[
            "bridge_items",
            "core",
            "core_items",
            "coverage",
            "demand",
            "demand_since",
            "explanation",
            "last_coverage",
            "momentum",
            "region_a",
            "region_b",
            "score",
            "similarity",
            "terms",
            "timeline",
            "verification",
        ],
    );
    if let Some(s) = out.get_mut("suggestion").filter(|s| s.is_object()) {
        *s = drop(s.take(), &["core", "collection_id", "space_id"]);
    }
    out
}

/// Readiness and the running job; the latest finished jobs only, without their parameters.
fn status(v: Value) -> Value {
    let v = map_key(v, "activity", |a| match a {
        Value::Array(jobs) => Value::Array(
            jobs.into_iter()
                .take(RECENT_JOBS)
                .map(|j| {
                    json!({
                        "job_id": j["job_id"], "kind": j["kind"], "state": j["state"],
                        "updated_at": j["updated_at"], "error": j["checkpoint"]["error"],
                    })
                })
                .collect(),
        ),
        other => other,
    });
    let v = map_key(v, "collections", |c| {
        each(c, |c| drop(c, &["source", "updated_at"]))
    });
    map_key(v, "tables", |t| {
        each(t, |mut t| {
            if let Some(r) = t.get_mut("regions").filter(|r| r.is_object()) {
                let stats = r["stats"].take();
                let r = r.as_object_mut().expect("checked above");
                r.remove("stats");
                for k in ["fine", "broad", "mean_stability"] {
                    if let Some(x) = stats.get(k) {
                        r.insert(k.into(), x.clone());
                    }
                }
            }
            drop(t, &["ann_partitions", "runtime", "indexed_at"])
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit_json(id: i64, text: &str) -> Value {
        json!({ "rank": 1, "similarity": 0.8, "via_chunk": null, "item": {
            "item_id": id, "title": "T", "date": "2020-01-01", "role": "demand", "url": null,
            "text": text, "input_hash": "abc", "meta": {"x": 1}, "source_id": "s",
            "collection_id": "c", "chunk_index": null, "parent_id": null } })
    }

    #[test]
    fn check_idea_keeps_the_verdict_and_a_few_slim_hits() {
        let hits: Vec<Value> = (0..10).map(|i| hit_json(i, &"word ".repeat(100))).collect();
        let v = json!({ "verdict": "open", "p_exists": 0.01, "scope": "in this collection only",
            "calibration": { "quality": "good" },
            "search": { "exhaustive": true, "embed_ms": 3.0, "hits": hits } });
        let c = result("check_idea", v);
        assert_eq!(c["verdict"], "open");
        assert_eq!(c["calibration"]["quality"], "good");
        assert!(
            c["scope"]
                .as_str()
                .unwrap()
                .contains("this collection only")
        );
        assert_eq!(c["search"]["exhaustive"], true);
        assert!(c["search"].get("embed_ms").is_none());
        let hits = c["search"]["hits"].as_array().unwrap();
        assert_eq!(hits.len(), CHECK_HITS);
        let item = &hits[0]["item"];
        assert_eq!(item["item_id"], "0");
        assert!(item.get("input_hash").is_none() && item.get("url").is_none());
        assert!(item["text"].as_str().unwrap().chars().count() <= TEXT_CHARS + 1);
    }

    #[test]
    fn big_ids_survive_as_strings() {
        let c = result(
            "search",
            json!({ "hits": [hit_json(3456100418089264742, "x")] }),
        );
        assert_eq!(c["hits"][0]["item"]["item_id"], "3456100418089264742");
    }

    #[test]
    fn gap_lists_drop_cores_and_timelines_and_details_skip_empty_years() {
        let gap = json!({ "gap_id": 1, "label": "a", "score": 3.6, "core": [1, 2],
            "timeline": [{ "year": 2000, "demand": 0, "coverage": 0 },
                         { "year": 2001, "demand": 2, "coverage": 0 }],
            "verification": { "summary": "none match", "closest": [{}], "matches": 0 },
            "core_items": [{ "item_id": 5, "title": "x" }], "suggestion": null });
        let l = result("list_gaps", json!({ "gaps": [gap.clone()], "scope": "s" }));
        let g = &l["gaps"][0];
        assert!(g.get("core").is_none() && g.get("timeline").is_none());
        assert_eq!(g["verification"]["summary"], "none match");
        assert_eq!(g["verification"]["matches"], 0);
        assert!(g["verification"].get("closest").is_none());
        assert_eq!(l["scope"], "s");
        let d = result("gap_detail", gap);
        assert_eq!(d["timeline"].as_array().unwrap().len(), 1);
        assert_eq!(d["core_items"][0]["item_id"], "5");
        assert_eq!(d["verification"]["matches"], 0);
    }

    #[test]
    fn suggest_decision_returns_what_was_recorded() {
        let v = json!({ "gap_id": 2, "label": "a", "verdict": null, "timeline": [], "core": [1],
            "scope": "in this collection", "agent_model_preference": { "requested_model": "x" },
            "suggestion": { "author": "agent:codex/gpt-6", "verdict": "confirmed_gap",
                "note": "n", "core": [1], "space_id": "s" } });
        let c = result("suggest_decision", v);
        assert!(c["verdict"].is_null() && c.get("timeline").is_none());
        assert_eq!(c["suggestion"]["author"], "agent:codex/gpt-6");
        assert!(c["suggestion"].get("core").is_none());
        assert_eq!(c["scope"], "in this collection");
        assert_eq!(c["agent_model_preference"]["requested_model"], "x");
    }

    #[test]
    fn other_tools_are_unchanged() {
        let v = json!({ "path": "/x", "core": [1] });
        assert_eq!(result("export_gaps", v.clone()), v);
    }
}
