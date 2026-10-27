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

#[cfg(test)]
mod tests {
    use super::*;

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
