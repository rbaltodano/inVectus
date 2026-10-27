//! `invectus eval backtest` (SPEC §9.1): freeze the collection at a cutoff date, rerun regions
//! and the gap scan on that past, then look forward: a gap is **filled** when a coverage item
//! dated within the horizon reaches its region (the same reach rule the scan uses: among the
//! item's two nearest regions and at least as central as the region's 25th-percentile member).
//! Rankings are compared with random, raw-count (demand items) and density-only (region size)
//! baselines on the same gaps.
//!
//! Without an LLM there is no midpoint description, so "the same thing" is judged by region
//! reach rather than by distance to a described midpoint.

use anyhow::{Result, bail};
use invectus_core::store::{CollectionRecord, VectorTableRecord};
use invectus_core::{CancelToken, Project, Role};
use invectus_gaps::{GapKind, ScanOptions, scan};
use invectus_index::{VectorRow, VectorStore};
use invectus_regions::{RegionOptions, run_regions};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Clone, Debug)]
pub struct BacktestOptions {
    /// Cutoff dates (`YYYY-MM-DD`); items dated after a cutoff are dropped for that run.
    pub cutoffs: Vec<String>,
    /// Years after the cutoff in which new coverage counts as filling a gap.
    pub horizon_years: i32,
    pub regions: RegionOptions,
    pub scan: ScanOptions,
}

/// Precision at 10 and 25 and ROC-AUC of one ranking.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RankScore {
    pub precision_at_10: f64,
    pub precision_at_25: f64,
    pub auc: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CutoffResult {
    pub cutoff: String,
    pub horizon_end: String,
    pub items: usize,
    pub regions: usize,
    /// Coverage and stale gaps (the ranked set compared with baselines).
    pub gaps: usize,
    pub filled: usize,
    /// Share of gaps filled: the expected precision of a random ranking.
    pub base_rate: f64,
    pub future_coverage: usize,
    pub model: RankScore,
    pub raw_count: RankScore,
    pub density: RankScore,
    pub bridges: usize,
    pub bridges_filled: usize,
    /// Each score term used alone as a ranking (coverage gaps only; the ablation that shows
    /// which terms help).
    pub terms: Vec<(String, RankScore)>,
    /// Every coverage and stale gap with its score terms and outcome, so weights can be tuned
    /// without rerunning the pipeline (`invectus eval tune`).
    #[serde(default)]
    pub scored: Vec<ScoredGap>,
}

/// One ranked gap of a cutoff, as needed to re-score it with other weights.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScoredGap {
    pub kind: String,
    pub demand: usize,
    pub region_size: i64,
    pub filled: bool,
    pub terms: Vec<invectus_gaps::scan::Term>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BacktestReport {
    pub collection: String,
    pub cutoffs: Vec<CutoffResult>,
    /// Means over cutoffs with at least one filled and one unfilled gap.
    pub mean_model: Option<RankScore>,
    pub mean_raw_count: Option<RankScore>,
    pub mean_density: Option<RankScore>,
    pub mean_terms: Vec<(String, RankScore)>,
    pub notes: Vec<String>,
}

fn auc(scores: &[f64], labels: &[bool]) -> f64 {
    let (mut wins, mut pairs) = (0.0, 0.0);
    for (i, &si) in scores.iter().enumerate() {
        if !labels[i] {
            continue;
        }
        for (j, &sj) in scores.iter().enumerate() {
            if labels[j] {
                continue;
            }
            pairs += 1.0;
            wins += if si > sj {
                1.0
            } else if si == sj {
                0.5
            } else {
                0.0
            };
        }
    }
    if pairs == 0.0 { f64::NAN } else { wins / pairs }
}

/// Score a ranking given by `scores` (higher first; ties broken by input order).
pub fn rank_score(scores: &[f64], labels: &[bool]) -> RankScore {
    let mut order: Vec<usize> = (0..scores.len()).collect();
    order.sort_by(|&a, &b| scores[b].total_cmp(&scores[a]).then(a.cmp(&b)));
    let at = |k: usize| {
        let k = k.min(order.len());
        if k == 0 {
            return f64::NAN;
        }
        order[..k].iter().filter(|&&i| labels[i]).count() as f64 / k as f64
    };
    RankScore {
        precision_at_10: at(10),
        precision_at_25: at(25),
        auc: auc(scores, labels),
    }
}

fn mean(xs: &[&RankScore]) -> Option<RankScore> {
    if xs.is_empty() {
        return None;
    }
    let m = |f: &dyn Fn(&RankScore) -> f64| xs.iter().map(|x| f(x)).sum::<f64>() / xs.len() as f64;
    Some(RankScore {
        precision_at_10: m(&|x| x.precision_at_10),
        precision_at_25: m(&|x| x.precision_at_25),
        auc: m(&|x| x.auc),
    })
}

fn add_years(date: &str, years: i32) -> Option<String> {
    let y: i32 = date.get(..4)?.parse().ok()?;
    Some(format!("{:04}{}", y + years, date.get(4..).unwrap_or("")))
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

pub fn backtest(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    opts: &BacktestOptions,
) -> Result<BacktestReport> {
    if opts.cutoffs.is_empty() {
        bail!("give at least one cutoff date");
    }
    let (ids, matrix) = vectors.parent_matrix(table)?;
    let dim = table.identity.dimensions;
    let items = project.store.item_facts(&table.collection_id)?;
    let row_of: HashMap<i64, usize> = ids.iter().enumerate().map(|(i, &id)| (id, i)).collect();
    let mut cutoffs = Vec::new();
    for cutoff in &opts.cutoffs {
        let horizon_end = add_years(cutoff, opts.horizon_years)
            .ok_or_else(|| anyhow::anyhow!("cutoff {cutoff:?} is not YYYY-MM-DD"))?;
        // The past: dated items up to the cutoff. Undated items are left out (their time is
        // unknown, so they could leak the future).
        let past: Vec<i64> = ids
            .iter()
            .copied()
            .filter(|id| {
                items[id]
                    .date
                    .as_deref()
                    .is_some_and(|d| d <= cutoff.as_str())
            })
            .collect();
        let dir = tempfile::tempdir()?;
        let mut temp = Project::create(&dir.path().join("backtest"))?;
        temp.store.upsert_collection(&CollectionRecord {
            collection_id: table.collection_id.clone(),
            source: serde_json::json!({"kind": "backtest", "cutoff": cutoff}),
            mapping: serde_json::json!({}),
            chunking: serde_json::json!({}),
        })?;
        // Full items (text for region labels) are copied in batches to bound memory.
        for part in past.chunks(20_000) {
            let full = project.store.items_by_ids(part)?;
            let batch: Vec<_> = part
                .iter()
                .filter_map(|id| full.get(id).map(|i| (i.clone(), vec![])))
                .collect();
            temp.store.write_items(&batch)?;
        }
        let tv = VectorStore::open(&temp)?;
        let ttable = tv.ensure_table(&temp, &table.collection_id, &table.identity, "embedded")?;
        let rows: Vec<VectorRow> = past
            .iter()
            .map(|id| {
                let i = row_of[id];
                VectorRow {
                    item_id: *id,
                    parent_id: None,
                    input_hash: "backtest".into(),
                    vector: matrix[i * dim..(i + 1) * dim].to_vec(),
                }
            })
            .collect();
        tv.add(&ttable, &rows)?;
        let regions = run_regions(
            &mut temp,
            &tv,
            &ttable,
            &opts.regions,
            &CancelToken::default(),
            &mut |_| {},
        )?;
        let report = scan(
            &mut temp,
            &tv,
            &ttable,
            &ScanOptions {
                as_of: Some(cutoff.clone()),
                ..opts.scan.clone()
            },
        )?;

        // Region reach for future coverage.
        let run = temp
            .store
            .latest_region_run(&table.collection_id, &table.space_id)?
            .expect("regions were just saved");
        let fine: Vec<_> = temp
            .store
            .regions(&run.run_id)?
            .into_iter()
            .filter(|r| r.level == "fine")
            .collect();
        let mut member_sims: HashMap<i32, Vec<f32>> = HashMap::new();
        for m in temp.store.region_members(&run.run_id)? {
            if !m.noise {
                member_sims
                    .entry(m.fine_id)
                    .or_default()
                    .push(m.centroid_sim);
            }
        }
        let p25: HashMap<i32, f32> = member_sims
            .into_iter()
            .map(|(r, mut v)| {
                v.sort_by(f32::total_cmp);
                (r, v[v.len() / 4])
            })
            .collect();
        let future: Vec<usize> = ids
            .iter()
            .enumerate()
            .filter(|(_, id)| {
                let it = &items[*id];
                it.role == Role::Coverage
                    && it
                        .date
                        .as_deref()
                        .is_some_and(|d| d > cutoff.as_str() && d <= horizon_end.as_str())
            })
            .map(|(i, _)| i)
            .collect();
        let mut reached: HashMap<i32, bool> = HashMap::new();
        let mut sims_to: Vec<HashMap<i32, f32>> = Vec::new();
        for &i in &future {
            let v = &matrix[i * dim..(i + 1) * dim];
            let sims: HashMap<i32, f32> = fine
                .iter()
                .map(|r| (r.region_id, dot(v, &r.centroid)))
                .collect();
            let mut order: Vec<(&i32, &f32)> = sims.iter().collect();
            order.sort_by(|a, b| b.1.total_cmp(a.1).then(a.0.cmp(b.0)));
            for (r, s) in order.into_iter().take(2) {
                if *s >= p25.get(r).copied().unwrap_or(f32::INFINITY) {
                    reached.insert(*r, true);
                }
            }
            sims_to.push(sims);
        }

        let region_gaps: Vec<_> = report
            .gaps
            .iter()
            .filter(|g| g.kind != GapKind::Bridge)
            .collect();
        let labels: Vec<bool> = region_gaps
            .iter()
            .map(|g| reached.contains_key(&g.region_a))
            .collect();
        let size_of: HashMap<i32, i64> = fine.iter().map(|r| (r.region_id, r.size)).collect();
        let filled = labels.iter().filter(|&&l| l).count();
        let bridges: Vec<_> = report
            .gaps
            .iter()
            .filter(|g| g.kind == GapKind::Bridge)
            .collect();
        let bridges_filled = bridges
            .iter()
            .filter(|g| {
                let (a, b) = (g.region_a, g.region_b.unwrap_or(g.region_a));
                sims_to.iter().any(|s| {
                    s[&a] >= p25.get(&a).copied().unwrap_or(f32::INFINITY)
                        && s[&b] >= p25.get(&b).copied().unwrap_or(f32::INFINITY)
                })
            })
            .count();
        let coverage_gaps: Vec<(usize, _)> = region_gaps
            .iter()
            .enumerate()
            .filter(|(_, g)| g.kind == GapKind::Coverage)
            .collect();
        let term_names: Vec<String> = coverage_gaps
            .first()
            .map(|(_, g)| g.terms.iter().map(|t| t.name.clone()).collect())
            .unwrap_or_default();
        let cov_labels: Vec<bool> = coverage_gaps.iter().map(|(i, _)| labels[*i]).collect();
        let terms: Vec<(String, RankScore)> = term_names
            .iter()
            .map(|name| {
                let values: Vec<f64> = coverage_gaps
                    .iter()
                    .map(|(_, g)| {
                        g.terms
                            .iter()
                            .find(|t| &t.name == name)
                            .map_or(f64::NEG_INFINITY, |t| t.value)
                    })
                    .collect();
                (name.clone(), rank_score(&values, &cov_labels))
            })
            .collect();
        cutoffs.push(CutoffResult {
            cutoff: cutoff.clone(),
            horizon_end,
            items: past.len(),
            regions: regions.fine,
            gaps: region_gaps.len(),
            filled,
            base_rate: filled as f64 / region_gaps.len().max(1) as f64,
            future_coverage: future.len(),
            model: rank_score(
                &region_gaps.iter().map(|g| g.score).collect::<Vec<_>>(),
                &labels,
            ),
            raw_count: rank_score(
                &region_gaps
                    .iter()
                    .map(|g| g.demand as f64)
                    .collect::<Vec<_>>(),
                &labels,
            ),
            density: rank_score(
                &region_gaps
                    .iter()
                    .map(|g| size_of.get(&g.region_a).copied().unwrap_or(0) as f64)
                    .collect::<Vec<_>>(),
                &labels,
            ),
            bridges: bridges.len(),
            bridges_filled,
            terms,
            scored: region_gaps
                .iter()
                .zip(&labels)
                .map(|(g, &filled)| ScoredGap {
                    kind: g.kind.as_str().into(),
                    demand: g.demand,
                    region_size: size_of.get(&g.region_a).copied().unwrap_or(0),
                    filled,
                    terms: g.terms.clone(),
                })
                .collect(),
        });
    }
    let informative: Vec<&CutoffResult> = cutoffs
        .iter()
        .filter(|c| c.filled > 0 && c.filled < c.gaps)
        .collect();
    let mut notes = vec![
        "filled = new coverage within the horizon reaches the gap's region (no LLM midpoint \
         descriptions yet)"
            .to_string(),
    ];
    if informative.len() < cutoffs.len() {
        notes.push(format!(
            "{} cutoff(s) had no filled or no unfilled gaps and are left out of the means",
            cutoffs.len() - informative.len()
        ));
    }
    Ok(BacktestReport {
        collection: table.collection_id.clone(),
        mean_model: mean(&informative.iter().map(|c| &c.model).collect::<Vec<_>>()),
        mean_raw_count: mean(&informative.iter().map(|c| &c.raw_count).collect::<Vec<_>>()),
        mean_density: mean(&informative.iter().map(|c| &c.density).collect::<Vec<_>>()),
        mean_terms: informative
            .first()
            .map(|c| {
                c.terms
                    .iter()
                    .map(|(name, _)| {
                        let scores: Vec<&RankScore> = informative
                            .iter()
                            .filter_map(|x| x.terms.iter().find(|t| &t.0 == name).map(|t| &t.1))
                            .collect();
                        (name.clone(), mean(&scores).expect("non-empty"))
                    })
                    .collect()
            })
            .unwrap_or_default(),
        cutoffs,
        notes,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranking_metrics() {
        let labels = [true, false, true, false];
        let r = rank_score(&[4.0, 3.0, 2.0, 1.0], &labels);
        assert_eq!(r.precision_at_10, 0.5);
        assert_eq!(r.auc, 0.75);
        assert_eq!(rank_score(&[1.0, 2.0, 3.0, 4.0], &labels).auc, 0.25);
        assert_eq!(rank_score(&[1.0; 4], &labels).auc, 0.5);
        assert!(rank_score(&[1.0], &[true]).auc.is_nan());
        assert_eq!(add_years("2020-12-31", 2).unwrap(), "2022-12-31");
    }
}
