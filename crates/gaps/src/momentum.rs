//! Momentum (SPEC §5.6): the slope of yearly item counts from a Poisson regression,
//! log E[count] = a + b · year, fitted by iteratively reweighted least squares.

/// Yearly growth rate `b` over `window` years ending at `last` (inclusive); exp(b) − 1 is the
/// relative change per year. `None` when the window holds no items.
pub fn poisson_slope(years: &[i32], last: i32, window: i32) -> Option<f64> {
    let first = last - window + 1;
    let mut counts = vec![0.0f64; window.max(1) as usize];
    for &y in years {
        if (first..=last).contains(&y) {
            counts[(y - first) as usize] += 1.0;
        }
    }
    let total: f64 = counts.iter().sum();
    if total == 0.0 {
        return None;
    }
    // Centre x so the intercept and slope are nearly uncorrelated.
    let mid = (counts.len() as f64 - 1.0) / 2.0;
    let xs: Vec<f64> = (0..counts.len()).map(|i| i as f64 - mid).collect();
    let mut a = (total / counts.len() as f64).ln();
    let mut b = 0.0;
    for _ in 0..50 {
        // Weighted least squares on the working response z = η + (y − μ)/μ, weights μ.
        let (mut sw, mut swx, mut swxx, mut swz, mut swxz) = (0.0, 0.0, 0.0, 0.0, 0.0);
        for (&x, &y) in xs.iter().zip(&counts) {
            let eta = a + b * x;
            let mu = eta.exp().max(1e-9);
            let z = eta + (y - mu) / mu;
            sw += mu;
            swx += mu * x;
            swxx += mu * x * x;
            swz += mu * z;
            swxz += mu * x * z;
        }
        let det = sw * swxx - swx * swx;
        if det.abs() < 1e-12 {
            break;
        }
        let nb = (sw * swxz - swx * swz) / det;
        let na = (swz - nb * swx) / sw;
        // All counts in the first or last year drive b to ±∞; cap it.
        let nb = nb.clamp(-3.0, 3.0);
        let done = (nb - b).abs() < 1e-10 && (na - a).abs() < 1e-10;
        a = na;
        b = nb;
        if done {
            break;
        }
    }
    Some(b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_growth_and_decline() {
        // Doubling each year: 1, 2, 4, 8, 16 → b = ln 2.
        let mut years = Vec::new();
        for (i, n) in [1, 2, 4, 8, 16].iter().enumerate() {
            years.extend(std::iter::repeat_n(2020 + i as i32, *n));
        }
        let b = poisson_slope(&years, 2024, 5).unwrap();
        assert!((b - std::f64::consts::LN_2).abs() < 1e-6, "{b}");
        let flat: Vec<i32> = (2020..2025).flat_map(|y| [y, y, y]).collect();
        assert!(poisson_slope(&flat, 2024, 5).unwrap().abs() < 1e-9);
        let down: Vec<i32> = years.iter().map(|y| 4044 - y).collect();
        assert!(poisson_slope(&down, 2024, 5).unwrap() < -0.6);
        assert_eq!(poisson_slope(&[1990], 2024, 5), None);
        // Items only in the last year: capped, finite.
        assert_eq!(poisson_slope(&[2024, 2024], 2024, 5), Some(3.0));
    }
}
