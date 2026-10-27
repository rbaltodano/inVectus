//! Broad regions: Ward agglomeration of fine-region centroids weighted by size (gapmap's
//! approach), so every fine region nests in exactly one broad region (SPEC §6.4).

/// Group `centroids` (with member counts `sizes`) into `k` clusters; returns the cluster of each
/// input, numbered by first appearance.
pub fn ward(centroids: &[Vec<f32>], sizes: &[usize], k: usize) -> Vec<u32> {
    let m = centroids.len();
    let k = k.clamp(1, m.max(1));
    let mut active: Vec<bool> = vec![true; m];
    let mut n: Vec<f64> = sizes.iter().map(|&s| s.max(1) as f64).collect();
    let mut cluster: Vec<usize> = (0..m).collect();
    // Ward merge cost: n_i n_j / (n_i + n_j) · ||c_i − c_j||².
    let mut d = vec![vec![0.0f64; m]; m];
    for i in 0..m {
        for j in i + 1..m {
            let sq: f64 = centroids[i]
                .iter()
                .zip(&centroids[j])
                .map(|(a, b)| f64::from(a - b).powi(2))
                .sum();
            let cost = n[i] * n[j] / (n[i] + n[j]) * sq;
            d[i][j] = cost;
            d[j][i] = cost;
        }
    }
    let mut remaining = m;
    while remaining > k {
        let mut best = (usize::MAX, usize::MAX, f64::INFINITY);
        for i in (0..m).filter(|&i| active[i]) {
            for j in (i + 1..m).filter(|&j| active[j]) {
                if d[i][j] < best.2 {
                    best = (i, j, d[i][j]);
                }
            }
        }
        let (i, j, dij) = best;
        // Lance–Williams update for Ward, merging j into i.
        for x in (0..m).filter(|&x| active[x] && x != i && x != j) {
            let nt = n[i] + n[j] + n[x];
            let v = ((n[i] + n[x]) * d[i][x] + (n[j] + n[x]) * d[j][x] - n[x] * dij) / nt;
            d[i][x] = v;
            d[x][i] = v;
        }
        n[i] += n[j];
        active[j] = false;
        for c in cluster.iter_mut() {
            if *c == j {
                *c = i;
            }
        }
        remaining -= 1;
    }
    let mut ids: Vec<u32> = cluster.iter().map(|&c| c as u32).collect();
    let mut map = std::collections::HashMap::new();
    for x in ids.iter_mut() {
        let next = map.len() as u32;
        *x = *map.entry(*x).or_insert(next);
    }
    ids
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_the_closest_centroids_first() {
        let c = vec![
            vec![1.0, 0.0],
            vec![0.99, 0.14],
            vec![0.0, 1.0],
            vec![0.14, 0.99],
            vec![-1.0, 0.0],
        ];
        let g = ward(&c, &[10, 10, 10, 10, 10], 3);
        assert_eq!(g[0], g[1]);
        assert_eq!(g[2], g[3]);
        assert!(g[4] != g[0] && g[4] != g[2]);
        assert_eq!(ward(&c, &[1; 5], 1), vec![0; 5]);
        assert_eq!(ward(&c, &[1; 5], 9), vec![0, 1, 2, 3, 4]);
    }
}
