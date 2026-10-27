//! Region stability across seeds: for each region of a reference partition, the best Jaccard
//! overlap with any community of another run, averaged over runs (SPEC §6.4).

use std::collections::HashMap;

/// Mean best-Jaccard of each reference region (ids `0..k`) against the communities of each run in
/// `others`. Nodes outside every reference region (`None`) count only toward the other runs'
/// community sizes.
pub fn best_jaccard(reference: &[Option<u32>], others: &[Vec<u32>], k: usize) -> Vec<f64> {
    let mut sums = vec![0.0; k];
    let ref_size = {
        let mut s = vec![0usize; k];
        for r in reference.iter().flatten() {
            s[*r as usize] += 1;
        }
        s
    };
    for other in others {
        let mut other_size: HashMap<u32, usize> = HashMap::new();
        let mut overlap: HashMap<(u32, u32), usize> = HashMap::new();
        for (i, &o) in other.iter().enumerate() {
            *other_size.entry(o).or_insert(0) += 1;
            if let Some(r) = reference[i] {
                *overlap.entry((r, o)).or_insert(0) += 1;
            }
        }
        let mut best = vec![0.0f64; k];
        for (&(r, o), &n) in &overlap {
            let union = ref_size[r as usize] + other_size[&o] - n;
            let j = n as f64 / union as f64;
            if j > best[r as usize] {
                best[r as usize] = j;
            }
        }
        for (s, b) in sums.iter_mut().zip(best) {
            *s += b;
        }
    }
    let runs = others.len().max(1) as f64;
    sums.into_iter().map(|s| s / runs).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identical_runs_are_fully_stable_and_splits_halve() {
        let reference = vec![Some(0), Some(0), Some(0), Some(0), Some(1), Some(1), None];
        let same = vec![5, 5, 5, 5, 7, 7, 9];
        let split = vec![1, 1, 2, 2, 3, 3, 3];
        let s = best_jaccard(&reference, std::slice::from_ref(&same), 2);
        assert_eq!(s, vec![1.0, 1.0]);
        let s = best_jaccard(&reference, &[same, split], 2);
        // Region 0 splits in two halves in the second run: (1.0 + 0.5) / 2.
        assert!((s[0] - 0.75).abs() < 1e-12);
        // Region 1 = {4, 5} vs {4, 5, 6}: 2/3.
        assert!((s[1] - (1.0 + 2.0 / 3.0) / 2.0).abs() < 1e-12);
    }
}
