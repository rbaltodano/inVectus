//! Leiden community detection (Traag, Waltman & van Eck, 2019) maximising modularity with a
//! resolution parameter γ. Steps per level: fast local moving of nodes, refinement of each
//! community into well-connected sub-communities, and aggregation on the refined partition (the
//! unrefined partition seeds the next level). Refinement merges greedily (best gain) rather than
//! sampling with a temperature; node orders come from a seeded RNG, so runs are reproducible.

use std::collections::{HashMap, VecDeque};

/// Undirected weighted graph in CSR form. Each edge is stored in both directions; no self loops.
#[derive(Clone, Debug)]
pub struct Graph {
    offsets: Vec<usize>,
    targets: Vec<u32>,
    weights: Vec<f64>,
    /// Weighted degree of each node (for aggregated nodes: the sum over their members).
    strength: Vec<f64>,
    /// Sum of strengths (2m).
    total: f64,
}

impl Graph {
    /// Build from undirected edges; duplicates are summed, self loops and non-positive weights
    /// are dropped.
    pub fn from_edges(n: usize, edges: impl IntoIterator<Item = (u32, u32, f64)>) -> Self {
        let mut adj: Vec<HashMap<u32, f64>> = vec![HashMap::new(); n];
        for (a, b, w) in edges {
            if a == b || w <= 0.0 || !w.is_finite() {
                continue;
            }
            *adj[a as usize].entry(b).or_insert(0.0) += w;
            *adj[b as usize].entry(a).or_insert(0.0) += w;
        }
        let strength = adj.iter().map(|m| m.values().sum()).collect();
        Self::from_adjacency(adj, strength)
    }

    fn from_adjacency(adj: Vec<HashMap<u32, f64>>, strength: Vec<f64>) -> Self {
        let mut offsets = Vec::with_capacity(adj.len() + 1);
        let mut targets = Vec::new();
        let mut weights = Vec::new();
        offsets.push(0);
        for m in adj {
            let mut row: Vec<(u32, f64)> = m.into_iter().collect();
            row.sort_by_key(|e| e.0);
            for (t, w) in row {
                targets.push(t);
                weights.push(w);
            }
            offsets.push(targets.len());
        }
        let total = strength.iter().sum();
        Self {
            offsets,
            targets,
            weights,
            strength,
            total,
        }
    }

    pub fn nodes(&self) -> usize {
        self.strength.len()
    }

    fn neighbors(&self, v: usize) -> impl Iterator<Item = (usize, f64)> + '_ {
        let r = self.offsets[v]..self.offsets[v + 1];
        self.targets[r.clone()]
            .iter()
            .zip(&self.weights[r])
            .map(|(&t, &w)| (t as usize, w))
    }

    /// Modularity of `membership` at resolution `gamma`.
    pub fn modularity(&self, membership: &[u32], gamma: f64) -> f64 {
        if self.total == 0.0 {
            return 0.0;
        }
        let k = membership
            .iter()
            .map(|&c| c as usize + 1)
            .max()
            .unwrap_or(0);
        let mut inside = vec![0.0; k];
        let mut strength = vec![0.0; k];
        for v in 0..self.nodes() {
            let c = membership[v] as usize;
            strength[c] += self.strength[v];
            for (u, w) in self.neighbors(v) {
                if membership[u] as usize == c {
                    inside[c] += w;
                }
            }
        }
        (0..k)
            .map(|c| inside[c] / self.total - gamma * (strength[c] / self.total).powi(2))
            .sum()
    }
}

/// xorshift64*: small, seeded, deterministic.
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1)
    }
    pub fn next_u64(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    pub fn permutation(&mut self, n: usize) -> Vec<usize> {
        let mut p: Vec<usize> = (0..n).collect();
        for i in (1..n).rev() {
            let j = (self.next_u64() % (i as u64 + 1)) as usize;
            p.swap(i, j);
        }
        p
    }
}

#[derive(Clone, Debug)]
pub struct LeidenOptions {
    /// Higher values give more, smaller communities.
    pub resolution: f64,
    pub seed: u64,
    /// Full Leiden passes; each starts from the previous result.
    pub iterations: usize,
}

impl Default for LeidenOptions {
    fn default() -> Self {
        Self {
            resolution: 1.0,
            seed: 0,
            iterations: 2,
        }
    }
}

/// Community of each node, numbered by decreasing size (ties: lowest node first).
pub fn leiden(g: &Graph, opts: &LeidenOptions) -> Vec<u32> {
    let mut rng = Rng::new(opts.seed);
    let mut membership: Vec<u32> = (0..g.nodes() as u32).collect();
    if g.total <= 0.0 {
        return membership;
    }
    for _ in 0..opts.iterations.max(1) {
        membership = pass(g, &membership, opts.resolution, &mut rng);
    }
    relabel_by_size(&membership)
}

const EPS: f64 = 1e-12;

fn pass(g: &Graph, init: &[u32], gamma: f64, rng: &mut Rng) -> Vec<u32> {
    let mut graph = g.clone();
    let mut part = init.to_vec();
    renumber(&mut part);
    // Original node -> node of the current aggregate graph.
    let mut node_of: Vec<u32> = (0..g.nodes() as u32).collect();
    for _level in 0..64 {
        move_nodes(&graph, &mut part, gamma, rng);
        let communities = renumber(&mut part);
        if communities == graph.nodes() {
            break;
        }
        let mut refined = refine(&graph, &part, gamma, rng);
        let mut groups = renumber(&mut refined);
        if groups == graph.nodes() {
            // Refinement merged nothing: aggregate on the moved partition so the level shrinks.
            refined = part.clone();
            groups = communities;
        }
        let (agg, agg_part) = aggregate(&graph, &refined, groups, &part);
        for x in &mut node_of {
            *x = refined[*x as usize];
        }
        graph = agg;
        part = agg_part;
    }
    node_of.iter().map(|&a| part[a as usize]).collect()
}

/// Renumber ids to 0..k in order of first appearance; returns k.
fn renumber(ids: &mut [u32]) -> usize {
    let mut map: HashMap<u32, u32> = HashMap::new();
    for x in ids.iter_mut() {
        let next = map.len() as u32;
        *x = *map.entry(*x).or_insert(next);
    }
    map.len()
}

fn relabel_by_size(ids: &[u32]) -> Vec<u32> {
    let mut stats: HashMap<u32, (usize, usize)> = HashMap::new();
    for (i, &c) in ids.iter().enumerate() {
        let e = stats.entry(c).or_insert((0, i));
        e.0 += 1;
    }
    let mut order: Vec<(u32, (usize, usize))> = stats.into_iter().collect();
    order.sort_by(|a, b| b.1.0.cmp(&a.1.0).then(a.1.1.cmp(&b.1.1)));
    let map: HashMap<u32, u32> = order
        .iter()
        .enumerate()
        .map(|(new, (old, _))| (*old, new as u32))
        .collect();
    ids.iter().map(|c| map[c]).collect()
}

/// Fast local moving: visit nodes from a queue, move each to the neighbouring community with the
/// best modularity gain (or a new community), and requeue neighbours that may now move.
fn move_nodes(g: &Graph, part: &mut [u32], gamma: f64, rng: &mut Rng) {
    let n = g.nodes();
    let mut comm_strength = vec![0.0; n];
    let mut comm_size = vec![0usize; n];
    for v in 0..n {
        comm_strength[part[v] as usize] += g.strength[v];
        comm_size[part[v] as usize] += 1;
    }
    let mut empty: Vec<u32> = (0..n as u32)
        .filter(|&c| comm_size[c as usize] == 0)
        .collect();
    let mut queue: VecDeque<usize> = rng.permutation(n).into();
    let mut queued = vec![true; n];
    let mut neigh_weight = vec![0.0; n];
    let mut neigh_comms: Vec<usize> = Vec::new();
    while let Some(v) = queue.pop_front() {
        queued[v] = false;
        let cv = part[v] as usize;
        let sv = g.strength[v];
        for (u, w) in g.neighbors(v) {
            let c = part[u] as usize;
            if neigh_weight[c] == 0.0 {
                neigh_comms.push(c);
            }
            neigh_weight[c] += w;
        }
        comm_strength[cv] -= sv;
        comm_size[cv] -= 1;
        let gain = |c: usize, w: f64| w - gamma * sv * comm_strength[c] / g.total;
        let mut best = cv;
        let mut best_gain = gain(cv, neigh_weight[cv]);
        for &c in &neigh_comms {
            let gc = gain(c, neigh_weight[c]);
            if gc > best_gain + EPS {
                best = c;
                best_gain = gc;
            }
        }
        if best_gain < -EPS {
            // Alone is better: reuse v's own community if now empty, else take an empty one.
            best = if comm_size[cv] == 0 {
                cv
            } else {
                empty.pop().map_or(cv, |c| c as usize)
            };
        }
        comm_strength[best] += sv;
        comm_size[best] += 1;
        if best != cv {
            if comm_size[cv] == 0 {
                empty.push(cv as u32);
            }
            part[v] = best as u32;
            for (u, _) in g.neighbors(v) {
                if !queued[u] && part[u] as usize != best {
                    queued[u] = true;
                    queue.push_back(u);
                }
            }
        }
        for &c in &neigh_comms {
            neigh_weight[c] = 0.0;
        }
        neigh_comms.clear();
    }
}

/// Split each community into well-connected sub-communities: singleton nodes merge greedily into
/// sub-communities of the same community when both are well connected and the gain is ≥ 0.
fn refine(g: &Graph, part: &[u32], gamma: f64, rng: &mut Rng) -> Vec<u32> {
    let n = g.nodes();
    let mut comm_strength = vec![0.0; n];
    for v in 0..n {
        comm_strength[part[v] as usize] += g.strength[v];
    }
    let mut refined: Vec<u32> = (0..n as u32).collect();
    let mut sub_strength = g.strength.clone();
    let mut sub_size = vec![1usize; n];
    // Weight from each sub-community to the rest of its community.
    let mut external: Vec<f64> = (0..n)
        .map(|v| {
            g.neighbors(v)
                .filter(|(u, _)| part[*u] == part[v])
                .map(|(_, w)| w)
                .sum()
        })
        .collect();
    let mut neigh_weight = vec![0.0; n];
    let mut neigh_subs: Vec<usize> = Vec::new();
    for v in rng.permutation(n) {
        let rv = refined[v] as usize;
        if sub_size[rv] != 1 {
            continue;
        }
        let c = part[v] as usize;
        let sv = g.strength[v];
        let sc = comm_strength[c];
        if external[rv] < gamma * sv * (sc - sv) / g.total - EPS {
            continue;
        }
        for (u, w) in g.neighbors(v) {
            if part[u] as usize != c {
                continue;
            }
            let t = refined[u] as usize;
            if t == rv {
                continue;
            }
            if neigh_weight[t] == 0.0 {
                neigh_subs.push(t);
            }
            neigh_weight[t] += w;
        }
        let mut best: Option<(usize, f64)> = None;
        for &t in &neigh_subs {
            let st = sub_strength[t];
            let well_connected = external[t] >= gamma * st * (sc - st) / g.total - EPS;
            let gain = neigh_weight[t] - gamma * sv * st / g.total;
            if well_connected && gain >= -EPS && best.is_none_or(|(_, b)| gain > b + EPS) {
                best = Some((t, gain));
            }
        }
        if let Some((t, _)) = best {
            external[t] += external[rv] - 2.0 * neigh_weight[t];
            sub_strength[t] += sv;
            sub_size[t] += 1;
            sub_size[rv] = 0;
            refined[v] = t as u32;
        }
        for &t in &neigh_subs {
            neigh_weight[t] = 0.0;
        }
        neigh_subs.clear();
    }
    refined
}

/// One node per group; edge weights and strengths summed. Returns the graph and each new node's
/// community from `part`.
fn aggregate(g: &Graph, groups: &[u32], k: usize, part: &[u32]) -> (Graph, Vec<u32>) {
    let mut adj: Vec<HashMap<u32, f64>> = vec![HashMap::new(); k];
    let mut strength = vec![0.0; k];
    let mut new_part = vec![0u32; k];
    for v in 0..g.nodes() {
        let a = groups[v] as usize;
        strength[a] += g.strength[v];
        new_part[a] = part[v];
        for (u, w) in g.neighbors(v) {
            let b = groups[u];
            if b as usize != a {
                *adj[a].entry(b).or_insert(0.0) += w;
            }
        }
    }
    (Graph::from_adjacency(adj, strength), new_part)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `k` cliques of size `s`, joined in a ring by single weak edges.
    fn ring_of_cliques(k: usize, s: usize) -> Graph {
        let mut edges = Vec::new();
        for c in 0..k {
            let base = (c * s) as u32;
            for i in 0..s as u32 {
                for j in i + 1..s as u32 {
                    edges.push((base + i, base + j, 1.0));
                }
            }
            let next = (((c + 1) % k) * s) as u32;
            edges.push((base, next, 0.1));
        }
        Graph::from_edges(k * s, edges)
    }

    #[test]
    fn finds_planted_cliques_for_every_seed() {
        let g = ring_of_cliques(12, 8);
        for seed in 0..5 {
            let m = leiden(
                &g,
                &LeidenOptions {
                    seed,
                    ..Default::default()
                },
            );
            for c in 0..12 {
                let ids: Vec<u32> = m[c * 8..(c + 1) * 8].to_vec();
                assert!(ids.iter().all(|&x| x == ids[0]), "seed {seed}: {m:?}");
            }
            let distinct: std::collections::HashSet<_> = m.iter().collect();
            assert_eq!(distinct.len(), 12, "seed {seed}");
            assert!(g.modularity(&m, 1.0) > 0.85);
        }
    }

    #[test]
    fn resolution_controls_granularity_and_runs_are_reproducible() {
        let g = ring_of_cliques(12, 8);
        let coarse = leiden(
            &g,
            &LeidenOptions {
                resolution: 0.02,
                ..Default::default()
            },
        );
        let k = |m: &[u32]| m.iter().collect::<std::collections::HashSet<_>>().len();
        assert!(k(&coarse) < 12, "{}", k(&coarse));
        let a = leiden(&g, &LeidenOptions::default());
        assert_eq!(a, leiden(&g, &LeidenOptions::default()));
        // Communities are numbered by size.
        let sizes: Vec<usize> = (0..k(&coarse) as u32)
            .map(|c| coarse.iter().filter(|&&x| x == c).count())
            .collect();
        assert!(sizes.windows(2).all(|w| w[0] >= w[1]));
    }

    #[test]
    fn handles_isolated_nodes_and_empty_graphs() {
        let g = Graph::from_edges(4, [(0, 1, 1.0), (2, 2, 5.0)]);
        let m = leiden(&g, &LeidenOptions::default());
        assert_eq!(m[0], m[1]);
        assert!(m[2] != m[0] && m[3] != m[0] && m[2] != m[3]);
        assert!(leiden(&Graph::from_edges(0, []), &LeidenOptions::default()).is_empty());
    }
}
