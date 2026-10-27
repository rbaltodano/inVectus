//! `invectus gaps scan`: coverage gaps, stale coverage and bridge gaps from the latest regions
//! (SPEC §5.1–5.3, §5.6–5.7). Every gap carries its score terms and a plain explanation.

use crate::momentum::poisson_slope;
use anyhow::{Context, Result, bail};
use invectus_core::store::{GapRecord, VectorTableRecord};
use invectus_core::{Project, Role};
use invectus_index::VectorStore;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

/// Regions below this stability are not ranked (SPEC §6.4).
pub const STABLE_AT: f64 = 0.5;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GapKind {
    Coverage,
    Stale,
    Bridge,
}

impl GapKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Coverage => "coverage",
            Self::Stale => "stale",
            Self::Bridge => "bridge",
        }
    }
}

impl std::str::FromStr for GapKind {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s {
            "coverage" => Ok(Self::Coverage),
            "stale" => Ok(Self::Stale),
            "bridge" => Ok(Self::Bridge),
            other => bail!("unknown gap type {other:?} (coverage, stale, bridge)"),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanOptions {
    /// Regions need at least this many demand items to be a coverage or stale gap.
    pub min_demand: usize,
    /// Years of history for momentum, ending at the collection's latest year.
    pub momentum_years: i32,
    /// Coverage newer than this many months before the latest date down-weights a gap.
    pub saturation_months: i32,
    /// Coverage gap when a region has no coverage or at least this many demand items per
    /// coverage item (raised to twice the collection's own ratio); otherwise a stale gap when
    /// this share of demand is newer than the coverage.
    pub coverage_ratio_at: f64,
    pub stale_share_at: f64,
    /// Bridge candidates: each region's nearest regions by centroid.
    pub bridge_neighbours: usize,
    /// Fine regions in the same broad region this similar are one topic split in two.
    pub sibling_similarity: f32,
    /// Bridges with more items between them than this ratio are not gaps.
    pub max_bridge_ratio: f64,
    pub weights: Weights,
    /// Treat this date (`YYYY-MM-DD`) as today; default: the system date.
    pub as_of: Option<String>,
}

/// Composite weights per z-scored term (SPEC §5.7). Stored with each scan.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Weights {
    pub demand_since: f64,
    pub coverage_ratio: f64,
    pub momentum: f64,
    pub stale_share: f64,
    pub bridge: f64,
    /// Subtracted (in z units) when recent coverage exists.
    pub saturation: f64,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            min_demand: 5,
            momentum_years: 6,
            saturation_months: 24,
            coverage_ratio_at: 10.0,
            stale_share_at: 0.5,
            bridge_neighbours: 3,
            sibling_similarity: 0.95,
            max_bridge_ratio: 0.2,
            weights: Weights {
                // Chosen by leave-one-cutoff-out tuning on the full gapmap benchmark
                // (PROGRESS.md, 2026-10-27): demand per coverage carries the signal; demand
                // since the last coverage added none on held-out cutoffs.
                demand_since: 0.0,
                coverage_ratio: 2.0,
                momentum: 0.5,
                stale_share: 1.0,
                bridge: 1.0,
                saturation: 1.0,
            },
            as_of: None,
        }
    }
}

/// One input of a gap's score.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Term {
    pub name: String,
    pub value: f64,
    /// Standardised across gaps of the same type.
    pub z: f64,
    pub weight: f64,
    pub contribution: f64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct YearCount {
    pub year: i32,
    pub demand: usize,
    pub coverage: usize,
}

/// A scored, explained claim that something is missing.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Gap {
    pub gap_id: i32,
    pub kind: GapKind,
    pub score: f64,
    pub terms: Vec<Term>,
    pub region_a: i32,
    pub region_b: Option<i32>,
    pub label: String,
    pub explanation: String,
    pub demand: usize,
    pub coverage: usize,
    pub demand_since: usize,
    pub last_coverage: Option<String>,
    pub momentum: Option<f64>,
    pub similarity: Option<f32>,
    pub bridge_items: Option<usize>,
    pub timeline: Vec<YearCount>,
    /// Items nearest the region centre(s); identify the gap across re-runs.
    pub core: Vec<i64>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ScanReport {
    pub run_id: String,
    pub collection: String,
    pub coverage: usize,
    pub stale: usize,
    pub bridge: usize,
    /// Regions skipped because they are unstable.
    pub unstable_regions: usize,
    pub latest_date: Option<String>,
    pub notes: Vec<String>,
    pub gaps: Vec<Gap>,
}

struct RegionStats {
    label: String,
    broad: Option<i32>,
    stable: bool,
    size: usize,
    p25: f32,
    centroid: Vec<f32>,
    demand_years: Vec<i32>,
    all_years: Vec<i32>,
    demand_dates: Vec<String>,
    covering: Vec<i64>,
    core: Vec<i64>,
}

/// Today's date (UTC) as `YYYY-MM-DD`.
fn today() -> String {
    let days = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs() / 86_400) as i64;
    // Civil date from days since 1970-01-01 (H. Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

fn year(date: &str) -> Option<i32> {
    date.get(..4)?.parse().ok()
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn months_before(date: &str, months: i32) -> Option<String> {
    let y: i32 = date.get(..4)?.parse().ok()?;
    let m: i32 = date.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    let total = y * 12 + (m - 1) - months;
    Some(format!(
        "{:04}-{:02}{}",
        total.div_euclid(12),
        total.rem_euclid(12) + 1,
        date.get(7..).unwrap_or("")
    ))
}

fn zscores(xs: &[f64]) -> Vec<f64> {
    let n = xs.len() as f64;
    if xs.is_empty() {
        return Vec::new();
    }
    let mean = xs.iter().sum::<f64>() / n;
    let sd = (xs.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n).sqrt();
    xs.iter()
        .map(|x| if sd > 1e-12 { (x - mean) / sd } else { 0.0 })
        .collect()
}

fn pct(b: f64) -> String {
    format!("{:+.0}%/yr", (b.exp() - 1.0) * 100.0)
}

pub fn scan(
    project: &mut Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    opts: &ScanOptions,
) -> Result<ScanReport> {
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .with_context(|| {
            format!(
                "collection {:?} has no regions; run `invectus regions run -c {}` first",
                table.collection_id, table.collection_id
            )
        })?;
    let regions = project.store.regions(&run.run_id)?;
    let members = project.store.region_members(&run.run_id)?;
    let items = project.store.item_facts(&table.collection_id)?;
    let fine: Vec<_> = regions.iter().filter(|r| r.level == "fine").collect();
    let index: HashMap<i32, usize> = fine
        .iter()
        .enumerate()
        .map(|(i, r)| (r.region_id, i))
        .collect();
    // Dates after today are placeholders (e.g. issue dates of articles not yet printed).
    let today = opts.as_of.clone().unwrap_or_else(today);
    let future = items
        .values()
        .filter(|i| i.date.as_ref().is_some_and(|d| *d > today))
        .count();
    let latest = items
        .values()
        .filter_map(|i| i.date.clone())
        .filter(|d| *d <= today)
        .max();
    // Momentum counts whole years: a latest date before December leaves its year incomplete.
    let last_year = latest.as_deref().and_then(|d| {
        let y = year(d)?;
        Some(if d < format!("{y}-12-01").as_str() {
            y - 1
        } else {
            y
        })
    });
    let mut notes = Vec::new();
    if future > 0 {
        notes.push(format!(
            "{future} item(s) dated after {today} were ignored for dates and momentum"
        ));
    }
    if latest.is_none() {
        notes.push(
            "no item has a date: coverage age, stale coverage and momentum are unavailable".into(),
        );
    }
    let roles: HashSet<Role> = items.values().map(|i| i.role).collect();
    if !roles.contains(&Role::Demand) || !roles.contains(&Role::Coverage) {
        notes.push(
            "coverage and stale gaps need both demand and coverage items; map roles at import"
                .into(),
        );
    }
    notes.push(
        "query voids need an imported search log and density holes are exploratory; neither is \
         scanned (SPEC §5.4–5.5)"
            .into(),
    );

    let mut stats: Vec<RegionStats> = fine
        .iter()
        .map(|r| RegionStats {
            label: r.label.clone(),
            broad: r.parent_id,
            stable: r.stability.is_none_or(|s| s >= STABLE_AT),
            size: r.size as usize,
            p25: 0.0,
            centroid: r.centroid.clone(),
            demand_years: Vec::new(),
            all_years: Vec::new(),
            demand_dates: Vec::new(),
            covering: Vec::new(),
            core: Vec::new(),
        })
        .collect();
    // Per region: member similarities (core members only; flagged noise is excluded).
    let mut sims: Vec<Vec<(f32, i64)>> = vec![Vec::new(); fine.len()];
    for m in members.iter().filter(|m| !m.noise) {
        let Some(&r) = index.get(&m.fine_id) else {
            continue;
        };
        sims[r].push((m.centroid_sim, m.item_id));
        if let Some(it) = items.get(&m.item_id) {
            let y = it.date.as_deref().and_then(year);
            if let Some(y) = y {
                stats[r].all_years.push(y);
            }
            if it.role == Role::Demand {
                if let Some(y) = y {
                    stats[r].demand_years.push(y);
                }
                if let Some(d) = &it.date {
                    stats[r].demand_dates.push(d.clone());
                }
            }
        }
    }
    for (r, s) in sims.iter_mut().enumerate() {
        s.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        stats[r].core = s.iter().take(20).map(|x| x.1).collect();
        let mut v: Vec<f32> = s.iter().map(|x| x.0).collect();
        v.sort_by(f32::total_cmp);
        stats[r].p25 = v.get(v.len() / 4).copied().unwrap_or(1.0);
    }
    let mut demand_count = vec![0usize; fine.len()];
    for m in members.iter().filter(|m| !m.noise) {
        if let Some(&r) = index.get(&m.fine_id)
            && items
                .get(&m.item_id)
                .is_some_and(|i| i.role == Role::Demand)
        {
            demand_count[r] += 1;
        }
    }

    // Item similarity to every fine centroid (for coverage reach and bridges).
    let (vids, matrix) = vectors.parent_matrix(table)?;
    let dim = table.identity.dimensions;
    let row = |i: usize| &matrix[i * dim..(i + 1) * dim];
    let centroid_sims: Vec<Vec<f32>> = (0..vids.len())
        .into_par_iter()
        .map(|i| stats.iter().map(|s| dot(row(i), &s.centroid)).collect())
        .collect();
    // A coverage item covers its two nearest regions where it is at least as central as the
    // region's 25th-percentile member (gapmap: reviews often sit in a neighbouring cluster).
    for (i, id) in vids.iter().enumerate() {
        if items.get(id).is_none_or(|it| it.role != Role::Coverage) {
            continue;
        }
        let mut order: Vec<usize> = (0..stats.len()).collect();
        order.sort_by(|&a, &b| centroid_sims[i][b].total_cmp(&centroid_sims[i][a]));
        for &r in order.iter().take(2) {
            if centroid_sims[i][r] >= stats[r].p25 {
                stats[r].covering.push(*id);
            }
        }
    }

    // "Few coverage" is relative to the field: at least twice the collection's own demand per
    // coverage item (and never below `coverage_ratio_at`).
    let demand_total = items.values().filter(|i| i.role == Role::Demand).count();
    let coverage_total = items.values().filter(|i| i.role == Role::Coverage).count();
    let collection_ratio = demand_total as f64 / (1.0 + coverage_total as f64);
    let coverage_gap_at = opts.coverage_ratio_at.max(2.0 * collection_ratio);
    let mut gaps: Vec<Gap> = Vec::new();
    let mut unstable = 0;
    let saturation_from = latest
        .as_deref()
        .and_then(|d| months_before(d, opts.saturation_months));
    // Coverage and stale gaps.
    let mut coverage_raw: Vec<(Gap, [f64; 3], bool)> = Vec::new();
    let mut stale_raw: Vec<(Gap, [f64; 3], bool)> = Vec::new();
    for (r, s) in stats.iter().enumerate() {
        if !s.stable {
            unstable += 1;
            continue;
        }
        let demand = demand_count[r];
        if demand < opts.min_demand {
            continue;
        }
        let cov_dates: Vec<String> = s
            .covering
            .iter()
            .filter_map(|id| items.get(id).and_then(|i| i.date.clone()))
            .collect();
        let last_cov = cov_dates.iter().max().cloned();
        let coverage = s.covering.len();
        let since = match &last_cov {
            Some(d) => s.demand_dates.iter().filter(|x| *x > d).count(),
            None => demand,
        };
        let momentum = last_year.and_then(|y| poisson_slope(&s.all_years, y, opts.momentum_years));
        let recent = match &saturation_from {
            Some(from) => cov_dates.iter().filter(|d| *d >= from).count(),
            None => 0,
        };
        let ratio = demand as f64 / (1.0 + coverage as f64);
        let mut timeline: BTreeMap<i32, YearCount> = BTreeMap::new();
        for &y in &s.demand_years {
            let e = timeline.entry(y).or_default();
            e.year = y;
            e.demand += 1;
        }
        for d in &cov_dates {
            if let Some(y) = year(d) {
                let e = timeline.entry(y).or_default();
                e.year = y;
                e.coverage += 1;
            }
        }
        let base = Gap {
            gap_id: 0,
            kind: GapKind::Coverage,
            score: 0.0,
            terms: Vec::new(),
            region_a: fine[r].region_id,
            region_b: None,
            label: s.label.clone(),
            explanation: String::new(),
            demand,
            coverage,
            demand_since: since,
            last_coverage: last_cov.clone(),
            momentum,
            similarity: None,
            bridge_items: None,
            timeline: timeline.into_values().collect(),
            core: s.core.clone(),
        };
        let trend = momentum
            .map(|b| format!(", activity {}", pct(b)))
            .unwrap_or_default();
        let recent_note = if recent > 0 {
            format!(
                "; {recent} coverage item(s) in the last {} months lower the score",
                opts.saturation_months
            )
        } else {
            String::new()
        };
        if coverage == 0 || ratio >= coverage_gap_at {
            let what = if coverage == 0 {
                "no coverage item reaches this region".to_string()
            } else {
                format!("only {coverage} coverage item(s) for {demand} demand items")
            };
            let mut g = base;
            g.explanation = format!("{demand} demand items and {what}{trend}{recent_note}.");
            coverage_raw.push((
                g,
                [
                    (1.0 + since as f64).ln(),
                    ratio.ln(),
                    momentum.unwrap_or(0.0),
                ],
                recent > 0,
            ));
        } else if let Some(last) = &last_cov {
            let share = since as f64 / demand as f64;
            if share >= opts.stale_share_at {
                let mut g = base;
                g.kind = GapKind::Stale;
                g.explanation = format!(
                    "{since} of {demand} demand items ({:.0}%) are newer than the latest coverage ({last}){trend}{recent_note}.",
                    share * 100.0
                );
                stale_raw.push((
                    g,
                    [share, (1.0 + since as f64).ln(), momentum.unwrap_or(0.0)],
                    recent > 0,
                ));
            }
        }
    }
    let w = &opts.weights;
    for (raw, names, weights) in [
        (
            coverage_raw,
            [
                "demand since last coverage (log)",
                "demand per coverage (log)",
                "momentum",
            ],
            [w.demand_since, w.coverage_ratio, w.momentum],
        ),
        (
            stale_raw,
            [
                "share of demand after coverage",
                "demand since last coverage (log)",
                "momentum",
            ],
            [w.stale_share, w.demand_since, w.momentum],
        ),
    ] {
        let zs: Vec<Vec<f64>> = (0..3)
            .map(|t| zscores(&raw.iter().map(|x| x.1[t]).collect::<Vec<_>>()))
            .collect();
        for (i, (mut g, values, saturated)) in raw.into_iter().enumerate() {
            g.terms = (0..3)
                .map(|t| Term {
                    name: names[t].into(),
                    value: values[t],
                    z: zs[t][i],
                    weight: weights[t],
                    contribution: zs[t][i] * weights[t],
                })
                .collect();
            if saturated {
                g.terms.push(Term {
                    name: "recent coverage".into(),
                    value: 1.0,
                    z: 1.0,
                    weight: -w.saturation,
                    contribution: -w.saturation,
                });
            }
            g.score = g.terms.iter().map(|t| t.contribution).sum();
            gaps.push(g);
        }
    }

    // Bridge gaps between close, active, stable regions.
    let active = |r: usize| {
        last_year
            .and_then(|y| poisson_slope(&stats[r].all_years, y, opts.momentum_years))
            .is_none_or(|b| b >= 0.0)
    };
    let mut pairs: HashSet<(usize, usize)> = HashSet::new();
    for a in 0..stats.len() {
        let mut near: Vec<(usize, f32)> = (0..stats.len())
            .filter(|&b| b != a)
            .map(|b| (b, dot(&stats[a].centroid, &stats[b].centroid)))
            .collect();
        near.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
        for (b, _) in near.into_iter().take(opts.bridge_neighbours) {
            pairs.insert((a.min(b), a.max(b)));
        }
    }
    let mut pairs: Vec<(usize, usize)> = pairs.into_iter().collect();
    pairs.sort_unstable();
    // "Close" is relative to this collection: at least the median similarity between a region
    // and its nearest other region. Unrelated topics with nothing between them are not bridges.
    let mut nearest: Vec<f32> = (0..stats.len())
        .filter_map(|a| {
            (0..stats.len())
                .filter(|&b| b != a)
                .map(|b| dot(&stats[a].centroid, &stats[b].centroid))
                .max_by(f32::total_cmp)
        })
        .collect();
    nearest.sort_by(f32::total_cmp);
    let close_at = nearest.get(nearest.len() / 2).copied().unwrap_or(1.0);
    let mut bridge_raw: Vec<(Gap, f64)> = Vec::new();
    for (a, b) in pairs {
        let (sa, sb) = (&stats[a], &stats[b]);
        if !sa.stable || !sb.stable || !active(a) || !active(b) {
            continue;
        }
        let sim = dot(&sa.centroid, &sb.centroid);
        if sim < close_at || (sa.broad == sb.broad && sim >= opts.sibling_similarity) {
            continue;
        }
        let between = centroid_sims
            .iter()
            .filter(|s| s[a] >= sa.p25 && s[b] >= sb.p25)
            .count();
        let ratio = between as f64 / ((sa.size * sb.size) as f64).sqrt().max(1.0);
        if ratio > opts.max_bridge_ratio {
            continue;
        }
        let ma = last_year.and_then(|y| poisson_slope(&sa.all_years, y, opts.momentum_years));
        let mb = last_year.and_then(|y| poisson_slope(&sb.all_years, y, opts.momentum_years));
        let score = f64::from(sim)
            * (1.0 - ratio)
            * (1.0 + ma.unwrap_or(0.0).max(0.0) + mb.unwrap_or(0.0).max(0.0));
        let trend = match (ma, mb) {
            (Some(x), Some(y)) => format!(" (activity {} and {})", pct(x), pct(y)),
            _ => String::new(),
        };
        let mut core: Vec<i64> = sa.core.iter().take(10).copied().collect();
        core.extend(sb.core.iter().take(10));
        bridge_raw.push((
            Gap {
                gap_id: 0,
                kind: GapKind::Bridge,
                score: 0.0,
                terms: Vec::new(),
                region_a: fine[a].region_id,
                region_b: Some(fine[b].region_id),
                label: format!("{} ↔ {}", sa.label, sb.label),
                explanation: format!(
                    "\"{}\" and \"{}\" are close (centroid similarity {sim:.3}){trend}, but only \
                     {between} item(s) sit between them (ratio {ratio:.3}).",
                    sa.label, sb.label
                ),
                demand: 0,
                coverage: 0,
                demand_since: 0,
                last_coverage: None,
                momentum: match (ma, mb) {
                    (Some(x), Some(y)) => Some(x.max(0.0) + y.max(0.0)),
                    _ => None,
                },
                similarity: Some(sim),
                bridge_items: Some(between),
                timeline: Vec::new(),
                core,
            },
            score,
        ));
    }
    let zs = zscores(&bridge_raw.iter().map(|x| x.1).collect::<Vec<_>>());
    for (i, (mut g, raw)) in bridge_raw.into_iter().enumerate() {
        g.terms = vec![Term {
            name: "similarity × (1 − bridge ratio) × (1 + momentum)".into(),
            value: raw,
            z: zs[i],
            weight: w.bridge,
            contribution: zs[i] * w.bridge,
        }];
        g.score = zs[i] * w.bridge;
        gaps.push(g);
    }

    gaps.sort_by(|a, b| {
        b.score
            .total_cmp(&a.score)
            .then(a.region_a.cmp(&b.region_a))
    });
    for (i, g) in gaps.iter_mut().enumerate() {
        g.gap_id = i as i32 + 1;
    }
    let count = |k: GapKind| gaps.iter().filter(|g| g.kind == k).count();
    let report = ScanReport {
        run_id: run.run_id.clone(),
        collection: table.collection_id.clone(),
        coverage: count(GapKind::Coverage),
        stale: count(GapKind::Stale),
        bridge: count(GapKind::Bridge),
        unstable_regions: unstable,
        latest_date: latest,
        notes,
        gaps,
    };
    let records: Vec<GapRecord> = report
        .gaps
        .iter()
        .map(|g| {
            Ok(GapRecord {
                gap_id: g.gap_id,
                kind: g.kind.as_str().into(),
                score: g.score,
                data: serde_json::to_value(g)?,
            })
        })
        .collect::<Result<_>>()?;
    project.store.save_gaps(
        &run.run_id,
        &serde_json::to_value(opts)?,
        &serde_json::json!({"notes": report.notes, "latest_date": report.latest_date,
                            "unstable_regions": report.unstable_regions}),
        &records,
    )?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_scores() {
        let t = today();
        assert_eq!(t.len(), 10);
        assert!(t.as_str() > "2025-01-01" && t.as_bytes()[4] == b'-');
        assert_eq!(months_before("2024-03-15", 24).unwrap(), "2022-03-15");
        assert_eq!(months_before("2024-01-01", 1).unwrap(), "2023-12-01");
        assert_eq!(year("2019-07-01"), Some(2019));
        let z = zscores(&[1.0, 2.0, 3.0]);
        assert!((z[0] + z[2]).abs() < 1e-12 && z[1] == 0.0);
        assert_eq!(zscores(&[5.0, 5.0]), vec![0.0, 0.0]);
        assert_eq!(pct(0.0), "+0%/yr");
        assert_eq!("bridge".parse::<GapKind>().unwrap(), GapKind::Bridge);
        assert!("x".parse::<GapKind>().is_err());
    }
}
