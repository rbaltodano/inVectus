//! `invectus eval tune` (SPEC §5.7, §9.1 step 4): choose gap-score weights from saved backtest
//! results with leave-one-cutoff-out, so every reported number is on a cutoff the weights never
//! saw. Scores are recomputed from the stored z-scored terms; the recent-coverage penalty keeps
//! its stored contribution.

use crate::backtest::{BacktestReport, RankScore, ScoredGap, rank_score};
use anyhow::{Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;

/// The tunable terms (names as the scan writes them).
pub const TERMS: [&str; 4] = [
    "demand since last coverage (log)",
    "demand per coverage (log)",
    "momentum",
    "share of demand after coverage",
];

/// Candidate weights for each term.
pub const GRID: [f64; 5] = [0.0, 0.25, 0.5, 1.0, 2.0];

#[derive(Clone, Debug, Serialize)]
pub struct Fold {
    /// The cutoff left out of tuning.
    pub cutoff: String,
    /// Weights chosen on the other cutoffs.
    pub weights: BTreeMap<String, f64>,
    pub train_auc: f64,
    /// The tuned score on the held-out cutoff.
    pub held_out: RankScore,
    /// The current default weights on the held-out cutoff.
    pub default: RankScore,
    /// Ranking by demand count on the held-out cutoff.
    pub raw_count: RankScore,
}

#[derive(Clone, Debug, Serialize)]
pub struct TuneReport {
    pub folds: Vec<Fold>,
    pub mean_tuned: RankScore,
    pub mean_default: RankScore,
    pub mean_raw_count: RankScore,
    /// Weights chosen on all cutoffs together (for inspection; not validated).
    pub all_cutoffs_weights: BTreeMap<String, f64>,
    pub notes: Vec<String>,
}

fn score(g: &ScoredGap, weights: &[f64; 4]) -> f64 {
    g.terms
        .iter()
        .map(|t| match TERMS.iter().position(|n| *n == t.name) {
            Some(i) => weights[i] * t.z,
            None => t.contribution,
        })
        .sum()
}

fn auc_of(gaps: &[ScoredGap], weights: &[f64; 4]) -> RankScore {
    let scores: Vec<f64> = gaps.iter().map(|g| score(g, weights)).collect();
    let labels: Vec<bool> = gaps.iter().map(|g| g.filled).collect();
    rank_score(&scores, &labels)
}

fn mean(xs: &[RankScore]) -> RankScore {
    let n = xs.len().max(1) as f64;
    let m = |f: &dyn Fn(&RankScore) -> f64| xs.iter().map(f).sum::<f64>() / n;
    RankScore {
        precision_at_10: m(&|x| x.precision_at_10),
        precision_at_25: m(&|x| x.precision_at_25),
        auc: m(&|x| x.auc),
    }
}

/// Best weights (by mean AUC) over `cutoffs`; ties keep the earlier grid point.
fn best(cutoffs: &[&Vec<ScoredGap>]) -> ([f64; 4], f64) {
    let mut best = ([1.0, 0.5, 0.5, 1.0], f64::NEG_INFINITY);
    for a in GRID {
        for b in GRID {
            for c in GRID {
                for d in GRID {
                    let w = [a, b, c, d];
                    if w.iter().all(|x| *x == 0.0) {
                        continue;
                    }
                    let auc = cutoffs.iter().map(|g| auc_of(g, &w).auc).sum::<f64>()
                        / cutoffs.len() as f64;
                    if auc > best.1 + 1e-12 {
                        best = (w, auc);
                    }
                }
            }
        }
    }
    best
}

fn named(w: &[f64; 4]) -> BTreeMap<String, f64> {
    TERMS
        .iter()
        .zip(w)
        .map(|(n, v)| (n.to_string(), *v))
        .collect()
}

pub fn tune(report: &BacktestReport) -> Result<TuneReport> {
    let usable: Vec<_> = report
        .cutoffs
        .iter()
        .filter(|c| {
            let filled = c.scored.iter().filter(|g| g.filled).count();
            filled > 0 && filled < c.scored.len()
        })
        .collect();
    if usable.len() < 2 {
        bail!(
            "tuning needs at least two cutoffs with both filled and unfilled gaps (and saved \
             gaps; rerun `invectus eval backtest --json` with this version)"
        );
    }
    let d = invectus_gaps::ScanOptions::default().weights;
    let default = [d.demand_since, d.coverage_ratio, d.momentum, d.stale_share];
    let mut folds = Vec::new();
    for (i, held) in usable.iter().enumerate() {
        let train: Vec<&Vec<ScoredGap>> = usable
            .iter()
            .enumerate()
            .filter(|(j, _)| *j != i)
            .map(|(_, c)| &c.scored)
            .collect();
        let (w, train_auc) = best(&train);
        let demand: Vec<f64> = held.scored.iter().map(|g| g.demand as f64).collect();
        let labels: Vec<bool> = held.scored.iter().map(|g| g.filled).collect();
        folds.push(Fold {
            cutoff: held.cutoff.clone(),
            weights: named(&w),
            train_auc,
            held_out: auc_of(&held.scored, &w),
            default: auc_of(&held.scored, &default),
            raw_count: rank_score(&demand, &labels),
        });
    }
    let all: Vec<&Vec<ScoredGap>> = usable.iter().map(|c| &c.scored).collect();
    let (w_all, _) = best(&all);
    Ok(TuneReport {
        mean_tuned: mean(&folds.iter().map(|f| f.held_out.clone()).collect::<Vec<_>>()),
        mean_default: mean(&folds.iter().map(|f| f.default.clone()).collect::<Vec<_>>()),
        mean_raw_count: mean(
            &folds
                .iter()
                .map(|f| f.raw_count.clone())
                .collect::<Vec<_>>(),
        ),
        folds,
        all_cutoffs_weights: named(&w_all),
        notes: vec![
            "weights are searched on a 5-value grid per term; each fold's numbers are on a \
             cutoff its weights never saw"
                .into(),
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::backtest::CutoffResult;
    use invectus_gaps::scan::Term;

    fn gap(ratio_z: f64, since_z: f64, filled: bool) -> ScoredGap {
        let t = |name: &str, z: f64| Term {
            name: name.into(),
            value: z,
            z,
            weight: 1.0,
            contribution: z,
        };
        ScoredGap {
            kind: "coverage".into(),
            demand: 10,
            region_size: 10,
            filled,
            terms: vec![t(TERMS[0], since_z), t(TERMS[1], ratio_z), t(TERMS[2], 0.0)],
        }
    }

    fn cutoff(name: &str) -> CutoffResult {
        // Filled gaps have a high ratio term and a low "since" term.
        let gaps: Vec<ScoredGap> = (0..20)
            .map(|i| {
                let filled = i % 2 == 0;
                let r = if filled { 1.0 } else { -1.0 } + i as f64 * 0.01;
                gap(r, -r, filled)
            })
            .collect();
        let zero = RankScore {
            precision_at_10: 0.0,
            precision_at_25: 0.0,
            auc: 0.5,
        };
        CutoffResult {
            cutoff: name.into(),
            horizon_end: String::new(),
            items: 0,
            regions: 0,
            gaps: gaps.len(),
            filled: 10,
            base_rate: 0.5,
            future_coverage: 0,
            model: zero.clone(),
            raw_count: zero.clone(),
            density: zero,
            bridges: 0,
            bridges_filled: 0,
            terms: vec![],
            scored: gaps,
        }
    }

    #[test]
    fn held_out_tuning_finds_the_informative_term() {
        let report = BacktestReport {
            collection: "c".into(),
            cutoffs: vec![cutoff("a"), cutoff("b"), cutoff("c")],
            mean_model: None,
            mean_raw_count: None,
            mean_density: None,
            mean_terms: vec![],
            notes: vec![],
        };
        let t = tune(&report).unwrap();
        assert_eq!(t.folds.len(), 3);
        for f in &t.folds {
            assert_eq!(f.held_out.auc, 1.0, "{:?}", f.weights);
            assert_eq!(f.weights[TERMS[0]], 0.0);
            assert!(f.weights[TERMS[1]] > 0.0);
        }
        let mut one = report.clone();
        one.cutoffs.truncate(1);
        assert!(tune(&one).is_err());
    }
}
