//! Readable region names from a local language model (SPEC §6.4: "an optional LLM label cached
//! per region"). The model sees a region's distinctive terms and the titles of its most central
//! items, never full texts. Regions that already have a label are skipped, so the job resumes.

use anyhow::{Context, Result};
use invectus_core::job::ProgressFn;
use invectus_core::store::VectorTableRecord;
use invectus_core::{CancelToken, Progress, Project};
use invectus_llm::{GenOptions, TextGenerator};
use serde::Serialize;
use std::time::Instant;

const SYSTEM: &str = "You name clusters of documents. Reply with one descriptive name of 3 to 8 \
words for the topic the documents share, and nothing else: no quotes, no explanation.";

#[derive(Clone, Debug, Serialize)]
pub struct LabelReport {
    pub run_id: String,
    pub model: String,
    pub labelled: usize,
    pub already_labelled: usize,
    pub seconds: f64,
    pub cancelled: bool,
    /// `(region, c-TF-IDF label, language-model label)` for each new label.
    pub labels: Vec<(i32, String, String)>,
}

/// Make the model's reply a single clean name.
pub fn clean(reply: &str) -> String {
    let line = reply
        .lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("");
    let line = line
        .trim_start_matches(|c: char| c == '#' || c == '-' || c == '*' || c.is_whitespace())
        .trim_start_matches("Name:")
        .trim();
    let line = line
        .trim_matches(|c: char| matches!(c, '"' | '\'' | '*' | '`' | '“' | '”'))
        .trim_end_matches('.')
        .trim();
    line.chars().take(80).collect()
}

/// Label the fine regions of the latest run (at most `limit` new labels).
pub fn label_regions(
    project: &Project,
    table: &VectorTableRecord,
    generator: &mut dyn TextGenerator,
    limit: Option<usize>,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
) -> Result<LabelReport> {
    let started = Instant::now();
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .context("no regions yet; run `invectus regions run` first")?;
    let regions: Vec<_> = project
        .store
        .regions(&run.run_id)?
        .into_iter()
        .filter(|r| r.level == "fine")
        .collect();
    let members = project.store.region_members(&run.run_id)?;
    let todo: Vec<_> = regions
        .iter()
        .filter(|r| r.llm_label.is_none())
        .take(limit.unwrap_or(usize::MAX))
        .collect();
    let mut report = LabelReport {
        run_id: run.run_id.clone(),
        model: generator.identity().model_key.clone(),
        labelled: 0,
        already_labelled: regions.iter().filter(|r| r.llm_label.is_some()).count(),
        seconds: 0.0,
        cancelled: false,
        labels: Vec::new(),
    };
    for (i, r) in todo.iter().enumerate() {
        if cancel.is_cancelled() {
            report.cancelled = true;
            break;
        }
        let mut central: Vec<_> = members
            .iter()
            .filter(|m| m.fine_id == r.region_id && !m.noise)
            .collect();
        central.sort_by(|a, b| b.centroid_sim.total_cmp(&a.centroid_sim));
        let ids: Vec<i64> = central.iter().take(10).map(|m| m.item_id).collect();
        let items = project.store.items_by_ids(&ids)?;
        let titles: Vec<String> = ids
            .iter()
            .filter_map(|id| items.get(id).and_then(|i| i.title.clone()))
            .map(|t| format!("- {}", t.chars().take(200).collect::<String>()))
            .collect();
        let prompt = format!(
            "Distinctive terms: {}\nRepresentative titles:\n{}\n\nName the shared topic.",
            r.terms.join(", "),
            titles.join("\n")
        );
        let out = generator.generate(
            Some(SYSTEM),
            &prompt,
            &GenOptions {
                max_tokens: 32,
                ..Default::default()
            },
        )?;
        let name = clean(&out.text);
        if !name.is_empty() {
            project.store.set_region_llm_label(
                &run.run_id,
                "fine",
                r.region_id,
                &name,
                &report.model,
            )?;
            report.labels.push((r.region_id, r.label.clone(), name));
            report.labelled += 1;
        }
        progress(&Progress {
            job_id: format!("label:{}", run.run_id),
            phase: "label".into(),
            done: i + 1,
            total: todo.len(),
        });
    }
    report.seconds = started.elapsed().as_secs_f64();
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replies_are_cleaned_to_one_name() {
        assert_eq!(
            clean("\"Corneal Crosslinking for Keratoconus.\""),
            "Corneal Crosslinking for Keratoconus"
        );
        assert_eq!(
            clean("\n\n**Glaucoma surgery outcomes**\nBecause the titles..."),
            "Glaucoma surgery outcomes"
        );
        assert_eq!(clean("Name: Retinal imaging"), "Retinal imaging");
        assert_eq!(clean(""), "");
        assert_eq!(clean(&"x".repeat(200)).len(), 80);
    }
}
