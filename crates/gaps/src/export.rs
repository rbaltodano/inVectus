//! Gap reports (SPEC §4 step 8): titles, links, scores, explanations and verdicts only. Source
//! text is never exported; much of it is copyrighted.

use crate::review::GapList;
use invectus_core::Project;
use std::fmt::Write;

fn csv_field(s: &str) -> String {
    if s.contains([',', '"', '\n']) {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

fn md_escape(s: &str) -> String {
    s.replace('|', "\\|").replace('\n', " ")
}

/// One row per gap: rank, type, score, label, verdict, note, explanation.
pub fn csv(list: &GapList) -> String {
    let mut out = String::from(
        "rank,type,score,label,verdict,note,explanation,outside_matches,outside_summary\n",
    );
    for g in &list.gaps {
        let (verdict, note) = g
            .verdict
            .as_ref()
            .map_or(("undecided", ""), |v| (v.verdict.as_str(), v.note.as_str()));
        let _ = writeln!(
            out,
            "{},{},{:.3},{},{},{},{},{},{}",
            g.gap.gap_id,
            g.gap.kind.as_str(),
            g.gap.score,
            csv_field(&g.gap.label),
            verdict,
            csv_field(note),
            csv_field(&g.gap.explanation),
            g.verification
                .as_ref()
                .map_or(String::new(), |v| v.matches.to_string()),
            csv_field(g.verification.as_ref().map_or("", |v| v.summary.as_str()))
        );
    }
    out
}

/// A readable report with the central items of each gap as titled links.
pub fn markdown(
    project: &Project,
    collection: &str,
    list: &GapList,
    top: usize,
) -> anyhow::Result<String> {
    let mut out = format!("# Gaps in {collection}\n\n");
    let _ = writeln!(
        out,
        "Regions run `{}`. Scores are relative within each gap type. Titles and links only; no \
         source text.\n",
        list.run_id
    );
    for g in list.gaps.iter().take(top) {
        let verdict = g.verdict.as_ref().map_or("undecided".to_string(), |v| {
            if v.note.is_empty() {
                v.verdict.clone()
            } else {
                format!("{} — {}", v.verdict, md_escape(&v.note))
            }
        });
        let _ = writeln!(
            out,
            "## {}. {} ({}, score {:.2})\n\n{}\n\nVerdict: {verdict}\n",
            g.gap.gap_id,
            md_escape(&g.gap.label),
            g.gap.kind.as_str(),
            g.gap.score,
            g.gap.explanation
        );
        if let Some(v) = &g.verification {
            let _ = writeln!(out, "Outside sources: {}\n", md_escape(&v.summary));
            for h in &v.closest {
                let year = h.year.map(|y| format!(" {y}")).unwrap_or_default();
                let title = md_escape(&h.title);
                let _ = match &h.url {
                    Some(u) => writeln!(out, "- outside: [{title}]({u}) ({}{year})", h.source),
                    None => writeln!(out, "- outside: {title} ({}{year})", h.source),
                };
            }
            out.push('\n');
        }
        let items = project.store.items_by_ids(&g.gap.core)?;
        for id in g.gap.core.iter().take(5) {
            if let Some(i) = items.get(id) {
                let title = md_escape(i.title.as_deref().unwrap_or(&i.source_id));
                let date = i
                    .date
                    .as_deref()
                    .map(|d| format!(" ({d})"))
                    .unwrap_or_default();
                match &i.url {
                    Some(u) => {
                        let _ = writeln!(out, "- [{title}]({u}){date} · {}", i.role.as_str());
                    }
                    None => {
                        let _ = writeln!(out, "- {title}{date} · {}", i.role.as_str());
                    }
                }
            }
        }
        out.push('\n');
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn escaping() {
        assert_eq!(csv_field("a,b"), "\"a,b\"");
        assert_eq!(csv_field("say \"hi\""), "\"say \"\"hi\"\"\"");
        assert_eq!(csv_field("plain"), "plain");
        assert_eq!(md_escape("a | b\nc"), "a \\| b c");
    }
}
