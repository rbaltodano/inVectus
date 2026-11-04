//! Readable region names from a local language model (SPEC §6.4: "an optional LLM label cached
//! per region"). The model sees a region's distinctive terms and the titles of its most central
//! items, never full texts. Regions that already have a label are skipped, so the job resumes.

use anyhow::{Context, Result};
use invectus_core::job::ProgressFn;
use invectus_core::store::{MemberRecord, VectorTableRecord};
use invectus_core::{CancelToken, Progress, Project};
use invectus_llm::{GenOptions, TextGenerator};
use serde::Serialize;
use std::time::Instant;

pub const SYSTEM: &str = "You name clusters of documents. Reply with one descriptive name of 3 to 8 \
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

/// Titles of a region's ten most central items, as the namer sees them.
fn central_titles(project: &Project, members: &[MemberRecord], region: i32) -> Result<Vec<String>> {
    let mut central: Vec<_> = members
        .iter()
        .filter(|m| m.fine_id == region && !m.noise)
        .collect();
    central.sort_by(|a, b| b.centroid_sim.total_cmp(&a.centroid_sim));
    let ids: Vec<i64> = central.iter().take(10).map(|m| m.item_id).collect();
    let items = project.store.items_by_ids(&ids)?;
    Ok(ids
        .iter()
        .filter_map(|id| items.get(id).and_then(|i| i.title.clone()))
        .map(|t| format!("- {}", t.chars().take(200).collect::<String>()))
        .collect())
}

fn prompt(terms: &[String], titles: &[String]) -> String {
    format!(
        "Distinctive terms: {}\nRepresentative titles:\n{}\n\nName the shared topic.",
        terms.join(", "),
        titles.join("\n")
    )
}

/// One fine region as a namer sees it, with its current name and who wrote it.
#[derive(Clone, Debug, Serialize)]
pub struct RegionContext {
    pub region: i32,
    /// The c-TF-IDF label (always kept).
    pub label: String,
    pub terms: Vec<String>,
    pub size: i64,
    pub central_titles: Vec<String>,
    pub prompt: String,
    pub name: Option<String>,
    pub named_by: Option<String>,
}

/// Fine regions of the latest run with the evidence the local model sees (terms and central
/// titles, never full texts); with `unnamed_only`, only regions without a name.
pub fn region_context(
    project: &Project,
    table: &VectorTableRecord,
    unnamed_only: bool,
) -> Result<(String, Vec<RegionContext>)> {
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .context("no regions yet; run `invectus regions run` first")?;
    let members = project.store.region_members(&run.run_id)?;
    let regions = project
        .store
        .regions(&run.run_id)?
        .into_iter()
        .filter(|r| r.level == "fine" && (!unnamed_only || r.llm_label.is_none()))
        .map(|r| {
            let central_titles = central_titles(project, &members, r.region_id)?;
            Ok(RegionContext {
                region: r.region_id,
                prompt: prompt(&r.terms, &central_titles),
                label: r.label,
                terms: r.terms,
                size: r.size,
                central_titles,
                name: r.llm_label,
                named_by: r.llm_model,
            })
        })
        .collect::<Result<_>>()?;
    Ok((run.run_id, regions))
}

#[derive(Clone, Debug, Serialize)]
pub struct NamingReport {
    pub run_id: String,
    pub author: String,
    /// `(region, stored name)`.
    pub named: Vec<(i32, String)>,
    /// `(region, reason)`.
    pub rejected: Vec<(i32, String)>,
}

/// Store names written outside the engine (by an agent) for fine regions of the latest run,
/// beside the c-TF-IDF labels, recording `author` as the writer. Names are cleaned as the local
/// model's replies are.
pub fn set_names(
    project: &Project,
    table: &VectorTableRecord,
    names: &[(i32, String)],
    author: &str,
) -> Result<NamingReport> {
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .context("no regions yet; run `invectus regions run` first")?;
    let fine: Vec<i32> = project
        .store
        .regions(&run.run_id)?
        .into_iter()
        .filter(|r| r.level == "fine")
        .map(|r| r.region_id)
        .collect();
    let mut report = NamingReport {
        run_id: run.run_id.clone(),
        author: author.into(),
        named: Vec::new(),
        rejected: Vec::new(),
    };
    for (region, raw) in names {
        let name = clean(raw);
        if !fine.contains(region) {
            report
                .rejected
                .push((*region, "no such fine region in the latest regions".into()));
        } else if name.is_empty() {
            report.rejected.push((*region, "empty name".into()));
        } else {
            project
                .store
                .set_region_llm_label(&run.run_id, "fine", *region, &name, author)?;
            report.named.push((*region, name));
        }
    }
    Ok(report)
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
        let titles = central_titles(project, &members, r.region_id)?;
        let prompt = prompt(&r.terms, &titles);
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
