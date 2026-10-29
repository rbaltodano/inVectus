//! `invectus regions`: cluster a collection's vector space into fine and broad regions, label them,
//! measure their stability across seeds and lay them out on a 2-D map (SPEC §6.4–6.5).

use crate::hierarchy::ward;
use crate::knn::{Matrix, dot, knn, knn_ivf, recall};
use crate::labels::{ctfidf, label};
use crate::layout::{layout, trustworthiness};
use crate::leiden::{Graph, LeidenOptions, leiden};
use crate::stability::best_jaccard;
use anyhow::{Result, bail};
use invectus_core::job::ProgressFn;
use invectus_core::store::{JobRecord, MemberRecord, RegionRecord, RegionRun, VectorTableRecord};
use invectus_core::{CancelToken, Progress, Project};
use invectus_index::VectorStore;
use serde::Serialize;
use std::collections::HashMap;
use std::time::Instant;

/// Regions below this stability are shown but cannot produce ranked gaps (SPEC §6.4).
pub const STABLE_AT: f64 = 0.5;

#[derive(Clone, Debug, Serialize)]
pub struct RegionOptions {
    /// Neighbours per item in the kNN graph.
    pub k: usize,
    /// Leiden resolution: higher gives more, smaller fine regions. `None` searches for the
    /// resolution that gives about `target_regions` fine regions.
    pub resolution: Option<f64>,
    /// Fine regions to aim for when the resolution is automatic (default: √items, 8–1,000).
    pub target_regions: Option<usize>,
    /// Leiden runs with seeds 0..seeds; seed 0 is the reference, the rest measure stability.
    pub seeds: usize,
    /// Communities smaller than this are noise, attached to the nearest region and flagged.
    pub min_size: usize,
    /// Broad regions (default: √fine regions).
    pub broad: Option<usize>,
    /// Above this many items the kNN graph is approximate (inverted file, measured recall).
    pub exact_knn_max: usize,
}

impl Default for RegionOptions {
    fn default() -> Self {
        Self {
            k: 30,
            resolution: None,
            target_regions: None,
            seeds: 5,
            min_size: 10,
            broad: None,
            exact_knn_max: 40_000,
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct RegionReport {
    pub run_id: String,
    pub collection: String,
    pub space_id: String,
    pub items: usize,
    pub edges: usize,
    /// `exact` or `ivf (lists L, probes P)`.
    pub knn: String,
    /// Recall@k of the approximate graph against exact neighbours on 200 sampled items.
    pub knn_recall: Option<f64>,
    pub fine: usize,
    pub broad: usize,
    pub noise: usize,
    pub sizes_min_median_max: (usize, usize, usize),
    /// The Leiden resolution used (given, or found for the target).
    pub resolution: f64,
    pub target_regions: Option<usize>,
    pub modularity: f64,
    /// Fine regions with stability ≥ 0.5.
    pub stable: usize,
    pub mean_stability: f64,
    pub trustworthiness_at_15: f64,
    pub seconds_knn: f64,
    pub seconds_cluster: f64,
    pub seconds_total: f64,
    pub cancelled: bool,
    pub regions: Vec<RegionRecord>,
}

pub fn job_id(collection_id: &str, space_id: &str) -> String {
    format!("regions:{collection_id}:{}", &space_id[..12])
}

pub fn run_regions(
    project: &mut Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    opts: &RegionOptions,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
) -> Result<RegionReport> {
    let job_id = job_id(&table.collection_id, &table.space_id);
    let mut job = JobRecord {
        job_id: job_id.clone(),
        kind: "regions".into(),
        params: serde_json::json!({"collection": table.collection_id,
                                   "space_id": table.space_id, "options": opts}),
        state: "running".into(),
        progress: 0.0,
        checkpoint: serde_json::json!({}),
        updated_at: None,
    };
    project.store.save_job(&job)?;
    let result = run_inner(project, vectors, table, opts, cancel, progress, &job_id);
    job.state = match &result {
        Ok(r) if r.cancelled => "cancelled".into(),
        Ok(_) => {
            job.progress = 1.0;
            "completed".into()
        }
        Err(_) => "failed".into(),
    };
    job.checkpoint = serde_json::json!({"error": result.as_ref().err().map(|e| format!("{e:#}"))});
    project.store.save_job(&job)?;
    result
}

fn run_inner(
    project: &mut Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    opts: &RegionOptions,
    cancel: &CancelToken,
    progress: ProgressFn<'_>,
    job_id: &str,
) -> Result<RegionReport> {
    let started = Instant::now();
    vectors.check(table)?;
    let (ids, data) = vectors.parent_matrix(table)?;
    let n = ids.len();
    if n < opts.min_size.max(2) * 2 {
        bail!(
            "{n} items are too few to find regions (need at least {})",
            opts.min_size.max(2) * 2
        );
    }
    let m = Matrix {
        data: &data,
        dim: table.identity.dimensions,
    };
    let steps = opts.seeds.max(1) + 3;
    let mut done = 0;
    let mut tick = |phase: &str| {
        done += 1;
        progress(&Progress {
            job_id: job_id.into(),
            phase: phase.into(),
            done,
            total: steps,
        });
    };
    let cancelled = |report_regions: Vec<RegionRecord>| RegionReport {
        run_id: String::new(),
        collection: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        items: n,
        edges: 0,
        knn: String::new(),
        knn_recall: None,
        fine: 0,
        broad: 0,
        noise: 0,
        sizes_min_median_max: (0, 0, 0),
        resolution: f64::NAN,
        target_regions: None,
        modularity: f64::NAN,
        stable: 0,
        mean_stability: f64::NAN,
        trustworthiness_at_15: f64::NAN,
        seconds_knn: 0.0,
        seconds_cluster: 0.0,
        seconds_total: started.elapsed().as_secs_f64(),
        cancelled: true,
        regions: report_regions,
    };

    // kNN graph: symmetric union, weight = cosine similarity (non-positive dropped).
    let t = Instant::now();
    let (nn, knn_method, knn_recall) = if n <= opts.exact_knn_max {
        (knn(&m, opts.k), "exact".to_string(), None)
    } else {
        let lists = ((n as f64).sqrt().round() as usize).max(16);
        let nn = knn_ivf(&m, opts.k, lists, 8);
        let r = recall(&m, &nn, opts.k, 200);
        (nn, format!("ivf (lists {lists}, probes 8)"), Some(r))
    };
    let mut edges: HashMap<(u32, u32), f64> = HashMap::new();
    for (i, row) in nn.iter().enumerate() {
        for &(j, s) in row {
            let key = ((i as u32).min(j), (i as u32).max(j));
            let w = edges.entry(key).or_insert(0.0);
            *w = w.max(f64::from(s));
        }
    }
    let edge_count = edges.len();
    let graph = Graph::from_edges(n, edges.into_iter().map(|((a, b), w)| (a, b, w)));
    let seconds_knn = t.elapsed().as_secs_f64();
    tick("knn");
    if cancel.is_cancelled() {
        return Ok(cancelled(vec![]));
    }

    let t = Instant::now();
    let target = opts
        .target_regions
        .unwrap_or_else(|| ((n as f64).sqrt().round() as usize).clamp(8, 1000));
    let resolution = match opts.resolution {
        Some(r) => r,
        None => find_resolution(&graph, target, opts.min_size),
    };
    let mut runs = Vec::new();
    for seed in 0..opts.seeds.max(1) as u64 {
        runs.push(leiden(
            &graph,
            &LeidenOptions {
                resolution,
                seed,
                ..Default::default()
            },
        ));
        tick("cluster");
        if cancel.is_cancelled() {
            return Ok(cancelled(vec![]));
        }
    }
    let seconds_cluster = t.elapsed().as_secs_f64();
    let reference = &runs[0];
    let modularity = graph.modularity(reference, resolution);

    // Fine regions: reference communities of at least `min_size` (already numbered by size).
    let mut community_size: HashMap<u32, usize> = HashMap::new();
    for &c in reference {
        *community_size.entry(c).or_insert(0) += 1;
    }
    let fine = (0..community_size.len() as u32)
        .take_while(|c| community_size[c] >= opts.min_size)
        .count();
    if fine == 0 {
        bail!(
            "no community reached {} items; lower --min-size or --resolution",
            opts.min_size
        );
    }
    let core: Vec<Option<u32>> = reference
        .iter()
        .map(|&c| ((c as usize) < fine).then_some(c))
        .collect();
    let mut centroids = vec![vec![0.0f32; m.dim]; fine];
    for (i, c) in core.iter().enumerate() {
        if let Some(c) = c {
            for (a, b) in centroids[*c as usize].iter_mut().zip(m.row(i)) {
                *a += b;
            }
        }
    }
    for c in &mut centroids {
        normalize(c);
    }
    // Noise items join their most similar region, flagged.
    let assign: Vec<usize> = (0..n)
        .map(|i| match core[i] {
            Some(c) => c as usize,
            None => argmax(&centroids, m.row(i)),
        })
        .collect();
    let mut sizes = vec![0usize; fine];
    for c in core.iter().flatten() {
        sizes[*c as usize] += 1;
    }
    let stability = best_jaccard(&core, &runs[1..], fine);
    let stability: Vec<Option<f64>> = if runs.len() > 1 {
        stability.into_iter().map(Some).collect()
    } else {
        vec![None; fine]
    };

    // Broad regions nest fine ones.
    let n_broad = opts
        .broad
        .unwrap_or_else(|| (fine as f64).sqrt().round() as usize)
        .clamp(1, fine);
    let broad_of = ward(&centroids, &sizes, n_broad);
    let n_broad = broad_of.iter().map(|&b| b as usize + 1).max().unwrap_or(1);
    let mut broad_centroids = vec![vec![0.0f32; m.dim]; n_broad];
    let mut broad_sizes = vec![0usize; n_broad];
    for f in 0..fine {
        let b = broad_of[f] as usize;
        broad_sizes[b] += sizes[f];
        for (a, x) in broad_centroids[b].iter_mut().zip(&centroids[f]) {
            *a += x * sizes[f] as f32;
        }
    }
    for c in &mut broad_centroids {
        normalize(c);
    }

    // Labels from member texts (core members only).
    let texts = project.store.parent_texts(&table.collection_id)?;
    let doc = |i: usize| {
        texts
            .get(&ids[i])
            .map(|(t, x)| format!("{t} {t} {x}"))
            .unwrap_or_default()
    };
    let docs: Vec<(usize, usize, String)> = (0..n)
        .filter_map(|i| core[i].map(|c| (c as usize, broad_of[c as usize] as usize, doc(i))))
        .collect();
    let fine_terms = ctfidf(docs.iter().map(|d| (d.0, d.2.as_str())), fine, 8);
    let broad_terms = ctfidf(docs.iter().map(|d| (d.1, d.2.as_str())), n_broad, 8);
    tick("label");
    if cancel.is_cancelled() {
        return Ok(cancelled(vec![]));
    }

    let map = layout(&m, &assign, &centroids);
    let trust = trustworthiness(&m, &map.points, 15, 1000);
    // Broad circles enclose their fine regions' circles (display only).
    let mut regions: Vec<RegionRecord> = (0..fine)
        .map(|f| RegionRecord {
            region_id: f as i32,
            level: "fine".into(),
            parent_id: Some(broad_of[f] as i32),
            label: label(&fine_terms[f]),
            terms: fine_terms[f].clone(),
            size: sizes[f] as i64,
            stability: stability[f],
            centroid: centroids[f].clone(),
            x: map.centers[f].0,
            y: map.centers[f].1,
            radius: map.radii[f],
            llm_label: None,
        })
        .collect();
    for b in 0..n_broad {
        let members: Vec<usize> = (0..fine).filter(|&f| broad_of[f] as usize == b).collect();
        let w: f64 = members
            .iter()
            .map(|&f| sizes[f] as f64)
            .sum::<f64>()
            .max(1.0);
        let cx = members
            .iter()
            .map(|&f| map.centers[f].0 * sizes[f] as f64)
            .sum::<f64>()
            / w;
        let cy = members
            .iter()
            .map(|&f| map.centers[f].1 * sizes[f] as f64)
            .sum::<f64>()
            / w;
        let radius = members
            .iter()
            .map(|&f| (map.centers[f].0 - cx).hypot(map.centers[f].1 - cy) + map.radii[f])
            .fold(0.0, f64::max);
        regions.push(RegionRecord {
            region_id: b as i32,
            level: "broad".into(),
            parent_id: None,
            label: label(&broad_terms[b]),
            terms: broad_terms[b].clone(),
            size: broad_sizes[b] as i64,
            stability: None,
            centroid: broad_centroids[b].clone(),
            x: cx,
            y: cy,
            radius,
            llm_label: None,
        });
    }
    let members: Vec<MemberRecord> = (0..n)
        .map(|i| MemberRecord {
            item_id: ids[i],
            fine_id: assign[i] as i32,
            broad_id: broad_of[assign[i]] as i32,
            noise: core[i].is_none(),
            centroid_sim: dot(m.row(i), &centroids[assign[i]]),
            x: map.points[i].0,
            y: map.points[i].1,
        })
        .collect();
    tick("layout");

    let noise = core.iter().filter(|c| c.is_none()).count();
    let mut sorted = sizes.clone();
    sorted.sort_unstable();
    let stab: Vec<f64> = stability.iter().flatten().copied().collect();
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)?
        .as_millis();
    let mut report = RegionReport {
        run_id: format!("{job_id}:{millis}"),
        collection: table.collection_id.clone(),
        space_id: table.space_id.clone(),
        items: n,
        edges: edge_count,
        knn: knn_method,
        knn_recall,
        fine,
        broad: n_broad,
        noise,
        sizes_min_median_max: (
            sorted[0],
            sorted[sorted.len() / 2],
            sorted[sorted.len() - 1],
        ),
        resolution,
        target_regions: opts.resolution.is_none().then_some(target),
        modularity,
        stable: stab.iter().filter(|&&s| s >= STABLE_AT).count(),
        mean_stability: if stab.is_empty() {
            f64::NAN
        } else {
            stab.iter().sum::<f64>() / stab.len() as f64
        },
        trustworthiness_at_15: trust,
        seconds_knn,
        seconds_cluster,
        seconds_total: 0.0,
        cancelled: false,
        regions: Vec::new(),
    };
    report.seconds_total = started.elapsed().as_secs_f64();
    let mut stats = serde_json::to_value(&report)?;
    stats.as_object_mut().unwrap().remove("regions");
    project.store.save_region_run(
        &RegionRun {
            run_id: report.run_id.clone(),
            collection_id: table.collection_id.clone(),
            space_id: table.space_id.clone(),
            params: serde_json::to_value(opts)?,
            stats,
            created_at: None,
        },
        &regions,
        &members,
    )?;
    report.regions = regions;
    Ok(report)
}

/// Bisect the resolution (log scale, 0.05–200) until seed 0 gives about `target` communities of
/// at least `min_size` items. The count is not monotone in the resolution: past a point,
/// communities shrink below `min_size`; the search keeps the best count it saw.
fn find_resolution(graph: &Graph, target: usize, min_size: usize) -> f64 {
    let count = |r: f64| {
        let m = leiden(
            graph,
            &LeidenOptions {
                resolution: r,
                ..Default::default()
            },
        );
        let mut sizes: HashMap<u32, usize> = HashMap::new();
        for c in m {
            *sizes.entry(c).or_insert(0) += 1;
        }
        sizes.values().filter(|&&s| s >= min_size).count()
    };
    let (mut lo, mut hi) = (0.05f64.ln(), 200f64.ln());
    let mut best = (usize::MAX, 1.0);
    for _ in 0..14 {
        let mid = (lo + hi) / 2.0;
        let c = count(mid.exp());
        let miss = c.abs_diff(target);
        if miss < best.0 {
            best = (miss, mid.exp());
        }
        if c == target {
            break;
        }
        if c < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    best.1
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        for x in v {
            *x /= n;
        }
    }
}

fn argmax(centroids: &[Vec<f32>], v: &[f32]) -> usize {
    centroids
        .iter()
        .enumerate()
        .max_by(|a, b| dot(a.1, v).total_cmp(&dot(b.1, v)).then(b.0.cmp(&a.0)))
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// A region near a query, for "your idea sits between A and B".
#[derive(Clone, Debug, Serialize)]
pub struct NearRegion {
    pub region_id: i32,
    pub level: String,
    pub label: String,
    pub similarity: f32,
    pub size: i64,
    pub stability: Option<f64>,
}

/// The `n` regions of `level` whose centroids are most similar to `q`.
pub fn nearest_regions(
    regions: &[RegionRecord],
    q: &[f32],
    level: &str,
    n: usize,
) -> Vec<NearRegion> {
    let mut v: Vec<NearRegion> = regions
        .iter()
        .filter(|r| r.level == level && r.centroid.len() == q.len())
        .map(|r| NearRegion {
            region_id: r.region_id,
            level: r.level.clone(),
            label: r.label.clone(),
            similarity: dot(&r.centroid, q),
            size: r.size,
            stability: r.stability,
        })
        .collect();
    v.sort_by(|a, b| {
        b.similarity
            .total_cmp(&a.similarity)
            .then(a.region_id.cmp(&b.region_id))
    });
    v.truncate(n);
    v
}
