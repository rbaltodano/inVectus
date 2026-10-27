//! Similarity → P(same thing) curve: weighted isotonic regression (pool adjacent violators) of a
//! 0/1 label on cosine similarity, interpolated linearly between block centres (SPEC §6.2).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};

/// Probabilities are kept inside this range: a few hundred pairs never justify certainty.
pub const P_MIN: f64 = 0.01;
pub const P_MAX: f64 = 0.99;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Curve {
    /// `(similarity, probability)` knots, strictly increasing in both.
    pub points: Vec<(f32, f64)>,
}

/// One labelled pair: similarity, whether it is the same thing, and its weight.
#[derive(Clone, Copy, Debug)]
pub struct Sample {
    pub similarity: f32,
    pub same: bool,
    pub weight: f64,
}

impl Curve {
    pub fn fit(samples: &[Sample]) -> Result<Self> {
        if !samples.iter().any(|s| s.same) || samples.iter().all(|s| s.same) {
            bail!("calibration needs both same-thing and different-thing pairs");
        }
        let mut s: Vec<Sample> = samples
            .iter()
            .copied()
            .filter(|s| s.similarity.is_finite() && s.weight > 0.0)
            .collect();
        s.sort_by(|a, b| a.similarity.total_cmp(&b.similarity));
        // Blocks of (weight, weighted label sum, weighted similarity sum).
        let mut blocks: Vec<(f64, f64, f64)> = Vec::new();
        for x in s {
            let y = if x.same { x.weight } else { 0.0 };
            blocks.push((x.weight, y, x.weight * f64::from(x.similarity)));
            // Merge while the previous block's mean is not below the new one's.
            while blocks.len() > 1 {
                let (w2, y2, s2) = blocks[blocks.len() - 1];
                let (w1, y1, s1) = blocks[blocks.len() - 2];
                if y1 / w1 < y2 / w2 {
                    break;
                }
                blocks.pop();
                *blocks.last_mut().unwrap() = (w1 + w2, y1 + y2, s1 + s2);
            }
        }
        let points = blocks
            .iter()
            .map(|(w, y, s)| ((s / w) as f32, (y / w).clamp(P_MIN, P_MAX)))
            .collect::<Vec<_>>();
        // Clamping can make knots at either end equal. Keep the last P_MIN knot and the first
        // P_MAX knot, so the curve stays flat beyond them and interpolates only in between.
        let mut out: Vec<(f32, f64)> = Vec::new();
        for p in points {
            match out.last_mut() {
                Some(l) if l.1 == p.1 => {
                    if p.1 <= P_MIN {
                        *l = p;
                    }
                }
                _ => out.push(p),
            }
        }
        Ok(Self { points: out })
    }

    /// P(same thing) at `similarity`.
    pub fn predict(&self, similarity: f32) -> f64 {
        let pts = &self.points;
        let Some(first) = pts.first() else {
            return P_MIN;
        };
        if similarity <= first.0 {
            return first.1;
        }
        for w in pts.windows(2) {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            if similarity <= x1 {
                let t = f64::from((similarity - x0) / (x1 - x0).max(f32::EPSILON));
                return y0 + t * (y1 - y0);
            }
        }
        pts.last().unwrap().1
    }

    /// Lowest similarity at which P(same) reaches `p`, if it ever does.
    pub fn similarity_at(&self, p: f64) -> Option<f32> {
        let pts = &self.points;
        if pts.first()?.1 >= p {
            return Some(pts[0].0);
        }
        pts.windows(2).find_map(|w| {
            let ((x0, y0), (x1, y1)) = (w[0], w[1]);
            (y1 >= p).then(|| x0 + ((p - y0) / (y1 - y0)) as f32 * (x1 - x0))
        })
    }
}

/// Area under the ROC curve: the chance a random positive scores above a random negative.
pub fn auc(positives: &[f32], negatives: &[f32]) -> f64 {
    if positives.is_empty() || negatives.is_empty() {
        return f64::NAN;
    }
    let mut wins = 0.0;
    for p in positives {
        for n in negatives {
            wins += if p > n {
                1.0
            } else if p == n {
                0.5
            } else {
                0.0
            };
        }
    }
    wins / (positives.len() * negatives.len()) as f64
}

/// Class-balanced Brier score of `curve` on held-out pairs (0 = perfect, 0.25 = uninformative).
pub fn balanced_brier(curve: &Curve, positives: &[f32], negatives: &[f32]) -> f64 {
    let mean = |xs: &[f32], f: &dyn Fn(f64) -> f64| {
        xs.iter().map(|&x| f(curve.predict(x))).sum::<f64>() / xs.len().max(1) as f64
    };
    0.5 * mean(positives, &|p| (1.0 - p).powi(2)) + 0.5 * mean(negatives, &|p| p * p)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(similarity: f32, same: bool) -> Sample {
        Sample {
            similarity,
            same,
            weight: 1.0,
        }
    }

    #[test]
    fn fit_is_monotone_and_pools_violators() {
        let c = Curve::fit(&[
            s(0.1, false),
            s(0.2, false),
            s(0.3, true),
            s(0.4, false), // violates order with 0.3: pooled
            s(0.6, true),
            s(0.7, true),
        ])
        .unwrap();
        assert!(
            c.points
                .windows(2)
                .all(|w| w[0].0 < w[1].0 && w[0].1 < w[1].1)
        );
        assert_eq!(c.points.first().unwrap().1, P_MIN);
        assert_eq!(c.points.last().unwrap().1, P_MAX);
        // The pooled 0.3/0.4 block has mean 0.5 at similarity 0.35.
        assert!(
            c.points
                .iter()
                .any(|(x, p)| (x - 0.35).abs() < 1e-6 && *p == 0.5),
            "{:?}",
            c.points
        );
        assert_eq!(c.points.len(), 3);
        assert!((c.predict(0.35) - 0.5).abs() < 1e-5);
        assert_eq!(c.predict(-1.0), P_MIN);
        assert_eq!(c.predict(1.0), P_MAX);
        let mut prev = 0.0;
        for i in 0..=100 {
            let p = c.predict(i as f32 / 100.0);
            assert!(p >= prev && (P_MIN..=P_MAX).contains(&p));
            prev = p;
        }
        let x = c.similarity_at(0.5).unwrap();
        assert!((x - 0.35).abs() < 1e-6, "{x}");
        assert!(c.similarity_at(0.999).is_none());
    }

    #[test]
    fn weights_balance_the_classes() {
        // One positive against three negatives at the same similarity: weighting the negatives
        // at 1/3 each gives an even 0.5.
        let mut v = vec![s(0.5, true)];
        for _ in 0..3 {
            v.push(Sample {
                weight: 1.0 / 3.0,
                ..s(0.5, false)
            });
        }
        v.push(s(0.0, false));
        v.push(s(0.9, true));
        let c = Curve::fit(&v).unwrap();
        assert!((c.predict(0.5) - 0.5).abs() < 1e-9, "{:?}", c.points);
    }

    #[test]
    fn needs_both_classes_and_scores_separation() {
        assert!(Curve::fit(&[s(0.5, true)]).is_err());
        assert_eq!(auc(&[0.9, 0.8], &[0.1, 0.2]), 1.0);
        assert_eq!(auc(&[0.5], &[0.5]), 0.5);
        let c = Curve::fit(&[s(0.1, false), s(0.9, true)]).unwrap();
        assert!(balanced_brier(&c, &[0.95], &[0.05]) < 0.001);
    }
}
