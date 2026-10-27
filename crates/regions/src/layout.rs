//! 2-D map coordinates for display only (SPEC §6.5); never used to decide regions or gaps.
//!
//! P0 found the tested Rust UMAP's neighbourhood preservation too poor, so this is the fallback the
//! spike recommended: classical MDS places the fine-region centroids, circles sized by region are
//! pushed apart until they stop overlapping, and each region's items are laid out inside its
//! circle by a local PCA of their vectors. Everything is deterministic. Trustworthiness@k reports
//! how well 2-D neighbourhoods reflect the original space.

use crate::knn::{Matrix, dot};

#[derive(Clone, Debug)]
pub struct Layout {
    pub centers: Vec<(f64, f64)>,
    pub radii: Vec<f64>,
    /// One point per matrix row.
    pub points: Vec<(f32, f32)>,
}

/// `region[i]` is the fine region (0..centroids.len()) of row `i`.
pub fn layout(m: &Matrix<'_>, region: &[usize], centroids: &[Vec<f32>]) -> Layout {
    let k = centroids.len();
    let mut sizes = vec![0usize; k];
    for &r in region {
        sizes[r] += 1;
    }
    let radii: Vec<f64> = sizes.iter().map(|&s| (s.max(1) as f64).sqrt()).collect();
    let mut centers = mds2(centroids);
    scale_and_separate(&mut centers, &radii);
    let mut points = vec![(0.0f32, 0.0f32); m.rows()];
    for r in 0..k {
        let rows: Vec<usize> = (0..m.rows()).filter(|&i| region[i] == r).collect();
        let local = local_pca(m, &rows, &centroids[r]);
        // Map the 95th-percentile radius to 90% of the circle; clamp the rest inside it.
        let mut radial: Vec<f64> = local.iter().map(|p| p.0.hypot(p.1)).collect();
        radial.sort_by(f64::total_cmp);
        let p95 = radial
            .get((radial.len() as f64 * 0.95) as usize)
            .copied()
            .unwrap_or(0.0)
            .max(1e-9);
        let s = 0.9 * radii[r] / p95;
        for (&i, &(a, b)) in rows.iter().zip(&local) {
            let (mut x, mut y) = (a * s, b * s);
            let d = x.hypot(y);
            if d > radii[r] {
                x *= radii[r] / d;
                y *= radii[r] / d;
            }
            points[i] = ((centers[r].0 + x) as f32, (centers[r].1 + y) as f32);
        }
    }
    Layout {
        centers,
        radii,
        points,
    }
}

/// Classical MDS of unit vectors to 2-D (squared distance = 2 − 2·cos).
fn mds2(c: &[Vec<f32>]) -> Vec<(f64, f64)> {
    let m = c.len();
    if m < 2 {
        return vec![(0.0, 0.0); m];
    }
    let mut b = vec![vec![0.0f64; m]; m];
    for i in 0..m {
        for j in 0..m {
            b[i][j] = -0.5 * (2.0 - 2.0 * f64::from(dot(&c[i], &c[j])));
        }
    }
    // Double centring.
    let row: Vec<f64> = b.iter().map(|r| r.iter().sum::<f64>() / m as f64).collect();
    let all = row.iter().sum::<f64>() / m as f64;
    for i in 0..m {
        for j in 0..m {
            b[i][j] = b[i][j] - row[i] - row[j] + all;
        }
    }
    let mut axes: Vec<(Vec<f64>, f64)> = Vec::new();
    for axis in 0..2 {
        let mut v: Vec<f64> = (0..m)
            .map(|i| 1.0 + ((i * 7 + axis * 3) % 11) as f64)
            .collect();
        let mut lambda = 0.0;
        for _ in 0..200 {
            let mut w: Vec<f64> = (0..m)
                .map(|i| (0..m).map(|j| b[i][j] * v[j]).sum())
                .collect();
            for (u, _) in &axes {
                let p: f64 = w.iter().zip(u).map(|(a, b)| a * b).sum();
                for (x, y) in w.iter_mut().zip(u) {
                    *x -= p * y;
                }
            }
            let norm = w.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm < 1e-12 {
                break;
            }
            lambda = norm;
            v = w.into_iter().map(|x| x / norm).collect();
        }
        axes.push((v, lambda));
    }
    (0..m)
        .map(|i| {
            (
                axes[0].0[i] * axes[0].1.sqrt(),
                axes[1].0[i] * axes[1].1.sqrt(),
            )
        })
        .collect()
}

/// Scale centres so neighbouring circles roughly touch, then push overlapping circles apart.
fn scale_and_separate(centers: &mut [(f64, f64)], radii: &[f64]) {
    let m = centers.len();
    if m < 2 {
        return;
    }
    let dist = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).hypot(a.1 - b.1);
    let mut ratios: Vec<f64> = (0..m)
        .filter_map(|i| {
            (0..m)
                .filter(|&j| j != i)
                .map(|j| (dist(centers[i], centers[j]), j))
                .filter(|(d, _)| *d > 1e-9)
                .min_by(|a, b| a.0.total_cmp(&b.0))
                .map(|(d, j)| (radii[i] + radii[j]) / d)
        })
        .collect();
    ratios.sort_by(f64::total_cmp);
    let s = ratios.get(ratios.len() / 2).copied().unwrap_or(1.0);
    for c in centers.iter_mut() {
        c.0 *= s;
        c.1 *= s;
    }
    for _ in 0..200 {
        let mut moved = false;
        for i in 0..m {
            for j in i + 1..m {
                let (dx, dy) = (centers[j].0 - centers[i].0, centers[j].1 - centers[i].1);
                let d = dx.hypot(dy);
                let need = (radii[i] + radii[j]) * 1.05;
                if d < need {
                    // Coincident centres separate along a fixed per-pair direction.
                    let (ux, uy) = if d > 1e-9 {
                        (dx / d, dy / d)
                    } else {
                        let a = (i * 31 + j * 17) as f64;
                        (a.cos(), a.sin())
                    };
                    let push = (need - d) / 2.0;
                    centers[i].0 -= ux * push;
                    centers[i].1 -= uy * push;
                    centers[j].0 += ux * push;
                    centers[j].1 += uy * push;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
}

/// First two principal components of `rows` around `center`, as 2-D coordinates.
fn local_pca(m: &Matrix<'_>, rows: &[usize], center: &[f32]) -> Vec<(f64, f64)> {
    if rows.len() < 3 {
        return rows
            .iter()
            .enumerate()
            .map(|(i, _)| (i as f64, 0.0))
            .collect();
    }
    let dim = m.dim;
    let centred: Vec<Vec<f64>> = rows
        .iter()
        .map(|&i| {
            m.row(i)
                .iter()
                .zip(center)
                .map(|(a, b)| f64::from(a - b))
                .collect()
        })
        .collect();
    let mut comps: Vec<Vec<f64>> = Vec::new();
    for c in 0..2 {
        let mut v: Vec<f64> = (0..dim)
            .map(|d| 1.0 + ((d * 13 + c * 5) % 7) as f64)
            .collect();
        for _ in 0..40 {
            // v ← Xᵀ(X v), orthogonal to earlier components.
            let xv: Vec<f64> = centred
                .iter()
                .map(|x| x.iter().zip(&v).map(|(a, b)| a * b).sum())
                .collect();
            let mut w = vec![0.0; dim];
            for (x, s) in centred.iter().zip(&xv) {
                for (wd, xd) in w.iter_mut().zip(x) {
                    *wd += xd * s;
                }
            }
            for u in &comps {
                let p: f64 = w.iter().zip(u).map(|(a, b)| a * b).sum();
                for (x, y) in w.iter_mut().zip(u) {
                    *x -= p * y;
                }
            }
            let norm = w.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm < 1e-12 {
                break;
            }
            v = w.into_iter().map(|x| x / norm).collect();
        }
        comps.push(v);
    }
    centred
        .iter()
        .map(|x| {
            let p = |u: &Vec<f64>| x.iter().zip(u).map(|(a, b)| a * b).sum::<f64>();
            (p(&comps[0]), p(&comps[1]))
        })
        .collect()
}

/// Trustworthiness@k (Venna & Kaski) of 2-D `points` against the rows of `m`, over `sample` rows
/// (all rows when fewer). 1.0 means no false neighbours appear in 2-D.
pub fn trustworthiness(m: &Matrix<'_>, points: &[(f32, f32)], k: usize, sample: usize) -> f64 {
    let n_all = m.rows();
    let step = (n_all / sample.max(1)).max(1);
    let idx: Vec<usize> = (0..n_all).step_by(step).take(sample).collect();
    let n = idx.len();
    if n <= 2 * k + 1 {
        return f64::NAN;
    }
    let mut penalty = 0.0;
    for (a, &i) in idx.iter().enumerate() {
        let mut high: Vec<(usize, f32)> = idx
            .iter()
            .enumerate()
            .filter(|&(b, _)| b != a)
            .map(|(b, &j)| (b, dot(m.row(i), m.row(j))))
            .collect();
        high.sort_by(|x, y| y.1.total_cmp(&x.1).then(x.0.cmp(&y.0)));
        let mut rank = vec![0usize; n];
        for (r, (b, _)) in high.iter().enumerate() {
            rank[*b] = r + 1;
        }
        let mut low: Vec<(usize, f32)> = idx
            .iter()
            .enumerate()
            .filter(|&(b, _)| b != a)
            .map(|(b, &j)| {
                let (p, q) = (points[i], points[j]);
                (b, (p.0 - q.0).hypot(p.1 - q.1))
            })
            .collect();
        low.sort_by(|x, y| x.1.total_cmp(&y.1).then(x.0.cmp(&y.0)));
        for (b, _) in low.iter().take(k) {
            if rank[*b] > k {
                penalty += (rank[*b] - k) as f64;
            }
        }
    }
    let (n, k) = (n as f64, k as f64);
    1.0 - 2.0 / (n * k * (2.0 * n - 3.0 * k - 1.0)) * penalty
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn regions_get_separate_circles_and_points_stay_inside() {
        // Two tight groups of 20 unit vectors around orthogonal directions.
        let mut data = Vec::new();
        let mut region = Vec::new();
        for g in 0..2 {
            for i in 0..20 {
                let e = 0.05 * (i as f32 / 20.0 - 0.5);
                let v = if g == 0 { [1.0, e, 0.0] } else { [0.0, e, 1.0] };
                let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
                data.extend(v.map(|x| x / n));
                region.push(g);
            }
        }
        let m = Matrix {
            data: &data,
            dim: 3,
        };
        let centroids = vec![vec![1.0, 0.0, 0.0], vec![0.0, 0.0, 1.0]];
        let l = layout(&m, &region, &centroids);
        let (a, b) = (l.centers[0], l.centers[1]);
        assert!((a.0 - b.0).hypot(a.1 - b.1) >= (l.radii[0] + l.radii[1]) * 0.99);
        for (i, p) in l.points.iter().enumerate() {
            let c = l.centers[region[i]];
            let d = (f64::from(p.0) - c.0).hypot(f64::from(p.1) - c.1);
            assert!(d <= l.radii[region[i]] + 1e-6 && p.0.is_finite());
        }
        let t = trustworthiness(&m, &l.points, 5, 1000);
        assert!(t > 0.9, "{t}");
        assert_eq!(
            layout(&m, &region, &centroids).points,
            l.points,
            "deterministic"
        );
    }
}
