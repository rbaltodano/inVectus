//! Exact cosine k-nearest neighbours over unit vectors (brute force, parallel over rows).
//!
//! Exact search keeps region results reproducible and free of index recall effects. Cost is
//! O(n² · d): about 1 s for 5k × 384 on 8 cores; 100k would be minutes and is not yet measured.

use rayon::prelude::*;

/// A row-major matrix of `n` unit vectors of `dim` floats.
pub struct Matrix<'a> {
    pub data: &'a [f32],
    pub dim: usize,
}

impl Matrix<'_> {
    pub fn rows(&self) -> usize {
        self.data.len() / self.dim
    }
    pub fn row(&self, i: usize) -> &[f32] {
        &self.data[i * self.dim..(i + 1) * self.dim]
    }
}

pub fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

/// For each row, its `k` most similar other rows as `(index, cosine)`, most similar first (ties
/// by index).
pub fn knn(m: &Matrix<'_>, k: usize) -> Vec<Vec<(u32, f32)>> {
    let n = m.rows();
    let k = k.min(n.saturating_sub(1));
    (0..n)
        .into_par_iter()
        .map(|i| {
            let a = m.row(i);
            let mut best: Vec<(u32, f32)> = (0..n)
                .filter(|&j| j != i)
                .map(|j| (j as u32, dot(a, m.row(j))))
                .collect();
            let cmp = |x: &(u32, f32), y: &(u32, f32)| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0));
            if k < best.len() {
                best.select_nth_unstable_by(k, cmp);
                best.truncate(k);
            }
            best.sort_by(cmp);
            best
        })
        .collect()
}

fn top_k(mut v: Vec<(u32, f32)>, k: usize) -> Vec<(u32, f32)> {
    let cmp = |x: &(u32, f32), y: &(u32, f32)| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0));
    if k < v.len() {
        v.select_nth_unstable_by(k, cmp);
        v.truncate(k);
    }
    v.sort_by(cmp);
    v
}

/// Indices of the `p` centroids most similar to `v`.
fn nearest_lists(v: &[f32], centroids: &[Vec<f32>], p: usize) -> Vec<u32> {
    let sims: Vec<(u32, f32)> = centroids
        .iter()
        .enumerate()
        .map(|(c, x)| (c as u32, dot(v, x)))
        .collect();
    top_k(sims, p).into_iter().map(|x| x.0).collect()
}

/// Approximate kNN through an inverted file, for collections too large for [`knn`]: spherical
/// k-means splits the rows into `lists` groups (trained on an evenly spaced sample), and each
/// row is compared exactly with the members of its `nprobe` nearest groups. Deterministic.
pub fn knn_ivf(m: &Matrix<'_>, k: usize, lists: usize, nprobe: usize) -> Vec<Vec<(u32, f32)>> {
    let n = m.rows();
    let lists = lists.clamp(1, n.max(1));
    let k = k.min(n.saturating_sub(1));
    let sample: Vec<usize> = (0..n).step_by((n / (lists * 64)).max(1)).collect();
    let mut centroids: Vec<Vec<f32>> = (0..lists)
        .map(|c| m.row(sample[c * sample.len() / lists]).to_vec())
        .collect();
    for _ in 0..10 {
        let assign: Vec<u32> = sample
            .par_iter()
            .map(|&i| nearest_lists(m.row(i), &centroids, 1)[0])
            .collect();
        let mut sums = vec![vec![0.0f32; m.dim]; lists];
        for (&i, &c) in sample.iter().zip(&assign) {
            for (s, x) in sums[c as usize].iter_mut().zip(m.row(i)) {
                *s += x;
            }
        }
        for (c, s) in sums.into_iter().enumerate() {
            let norm = s.iter().map(|x| x * x).sum::<f32>().sqrt();
            // An empty group keeps its previous centroid.
            if norm > 0.0 {
                centroids[c] = s.into_iter().map(|x| x / norm).collect();
            }
        }
    }
    let probes: Vec<Vec<u32>> = (0..n)
        .into_par_iter()
        .map(|i| nearest_lists(m.row(i), &centroids, nprobe.max(1)))
        .collect();
    let mut members: Vec<Vec<u32>> = vec![Vec::new(); lists];
    for (i, p) in probes.iter().enumerate() {
        members[p[0] as usize].push(i as u32);
    }
    (0..n)
        .into_par_iter()
        .map(|i| {
            let a = m.row(i);
            let cands: Vec<(u32, f32)> = probes[i]
                .iter()
                .flat_map(|&l| members[l as usize].iter())
                .filter(|&&j| j as usize != i)
                .map(|&j| (j, dot(a, m.row(j as usize))))
                .collect();
            top_k(cands, k)
        })
        .collect()
}

/// Share of exact top-`k` neighbours found by `approx`, over `sample` evenly spaced rows.
pub fn recall(m: &Matrix<'_>, approx: &[Vec<(u32, f32)>], k: usize, sample: usize) -> f64 {
    let n = m.rows();
    let rows: Vec<usize> = (0..n)
        .step_by((n / sample.max(1)).max(1))
        .take(sample)
        .collect();
    let (found, total) = rows
        .par_iter()
        .map(|&i| {
            let exact: Vec<(u32, f32)> = (0..n)
                .filter(|&j| j != i)
                .map(|j| (j as u32, dot(m.row(i), m.row(j))))
                .collect();
            let exact = top_k(exact, k);
            let got: std::collections::HashSet<u32> = approx[i].iter().map(|x| x.0).collect();
            (
                exact.iter().filter(|x| got.contains(&x.0)).count(),
                exact.len(),
            )
        })
        .reduce(|| (0, 0), |a, b| (a.0 + b.0, a.1 + b.1));
    found as f64 / total.max(1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ivf_recall_is_high_on_clustered_data() {
        // 2,000 points around 20 random directions in 16-D.
        let mut rng = crate::leiden::Rng::new(7);
        let mut unit = |n: usize| {
            let v: Vec<f32> = (0..n)
                .map(|_| (rng.next_u64() % 2000) as f32 / 1000.0 - 1.0)
                .collect();
            let s = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            v.into_iter().map(|x| x / s).collect::<Vec<f32>>()
        };
        let centres: Vec<Vec<f32>> = (0..20).map(|_| unit(16)).collect();
        let mut data = Vec::new();
        for i in 0..2000 {
            let noise = unit(16);
            let v: Vec<f32> = centres[i % 20]
                .iter()
                .zip(&noise)
                .map(|(c, e)| c + 0.3 * e)
                .collect();
            let s = v.iter().map(|x| x * x).sum::<f32>().sqrt();
            data.extend(v.into_iter().map(|x| x / s));
        }
        let m = Matrix {
            data: &data,
            dim: 16,
        };
        let approx = knn_ivf(&m, 10, 40, 8);
        assert!(approx.iter().all(|r| r.len() == 10));
        let r = recall(&m, &approx, 10, 200);
        assert!(r > 0.9, "{r}");
        assert_eq!(knn_ivf(&m, 10, 40, 8), approx, "deterministic");
        assert_eq!(recall(&m, &knn(&m, 10), 10, 50), 1.0);
    }

    #[test]
    fn finds_nearest_rows_excluding_self() {
        let s = std::f32::consts::FRAC_1_SQRT_2;
        let data = [1.0, 0.0, s, s, 0.0, 1.0, -1.0, 0.0];
        let m = Matrix {
            data: &data,
            dim: 2,
        };
        let nn = knn(&m, 2);
        assert_eq!(nn[0].iter().map(|x| x.0).collect::<Vec<_>>(), vec![1, 2]);
        assert_eq!(nn[2][0].0, 1);
        assert!((nn[0][0].1 - s).abs() < 1e-6);
        assert_eq!(knn(&m, 10)[0].len(), 3);
    }
}
