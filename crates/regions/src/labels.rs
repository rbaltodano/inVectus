//! Region labels from class-based TF-IDF (c-TF-IDF): a term scores high when it is frequent in a
//! region and rare across regions. No language model is involved.

use std::collections::HashMap;

const STOPWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "that",
    "this",
    "from",
    "were",
    "was",
    "are",
    "been",
    "have",
    "has",
    "had",
    "not",
    "but",
    "can",
    "may",
    "its",
    "their",
    "there",
    "these",
    "those",
    "which",
    "who",
    "whom",
    "than",
    "then",
    "into",
    "onto",
    "upon",
    "over",
    "under",
    "between",
    "after",
    "before",
    "during",
    "about",
    "also",
    "both",
    "each",
    "other",
    "such",
    "more",
    "most",
    "less",
    "least",
    "all",
    "any",
    "some",
    "our",
    "his",
    "her",
    "they",
    "them",
    "she",
    "him",
    "you",
    "your",
    "will",
    "would",
    "could",
    "should",
    "did",
    "does",
    "doing",
    "via",
    "per",
    "using",
    "used",
    "use",
    "based",
    "however",
    "whereas",
    "while",
    "within",
    "without",
    "among",
    "when",
    "where",
    "what",
    "how",
    "why",
    "one",
    "two",
    "three",
    "four",
    "five",
    "first",
    "second",
    "new",
    "results",
    "result",
    "conclusions",
    "conclusion",
    "methods",
    "method",
    "background",
    "objective",
    "objectives",
    "purpose",
    "aim",
    "aims",
    "study",
    "studies",
    "patients",
    "patient",
    "group",
    "groups",
    "years",
    "year",
    "significant",
    "significantly",
    "compared",
    "showed",
    "found",
    "total",
    "mean",
    "data",
    "analysis",
    "associated",
    "including",
    "included",
    "case",
    "cases",
    "report",
    "reported",
    "review",
    "high",
    "higher",
    "low",
    "lower",
    "after",
    "time",
    "only",
    "well",
    "being",
    "very",
    "most",
    "many",
    "much",
    "same",
    "different",
    "due",
    "who",
];

/// Lower-case alphabetic words of 3+ letters that are not stop words.
pub fn tokens(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split(|c: char| !c.is_alphanumeric() && c != '-')
        .map(|w| w.trim_matches('-').to_lowercase())
        .filter(|w| {
            w.chars().count() >= 3
                && w.chars().any(char::is_alphabetic)
                && !w.chars().all(|c| c.is_ascii_digit() || c == '-')
                && !STOPWORDS.contains(&w.as_str())
        })
}

/// Top `n` c-TF-IDF terms of each class. `docs` pairs each class id (0..classes) with its text.
pub fn ctfidf<'a>(
    docs: impl IntoIterator<Item = (usize, &'a str)>,
    classes: usize,
    n: usize,
) -> Vec<Vec<String>> {
    let mut counts: Vec<HashMap<String, f64>> = vec![HashMap::new(); classes];
    let mut totals = vec![0.0; classes];
    for (c, text) in docs {
        for t in tokens(text) {
            *counts[c].entry(t).or_insert(0.0) += 1.0;
            totals[c] += 1.0;
        }
    }
    let mut corpus: HashMap<&str, f64> = HashMap::new();
    for m in &counts {
        for (t, k) in m {
            *corpus.entry(t.as_str()).or_insert(0.0) += k;
        }
    }
    let avg = totals.iter().sum::<f64>() / classes.max(1) as f64;
    counts
        .iter()
        .zip(&totals)
        .map(|(m, &total)| {
            let mut scored: Vec<(&String, f64)> = m
                .iter()
                // Ignore terms used once in a region: they are noise, not a theme.
                .filter(|(_, k)| **k >= 2.0)
                .map(|(t, k)| {
                    (
                        t,
                        k / total.max(1.0) * (1.0 + avg / corpus[t.as_str()]).ln(),
                    )
                })
                .collect();
            scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(b.0)));
            scored.into_iter().take(n).map(|(t, _)| t.clone()).collect()
        })
        .collect()
}

/// A short label from the top terms.
pub fn label(terms: &[String]) -> String {
    if terms.is_empty() {
        return "(no distinctive terms)".into();
    }
    terms
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>()
        .join(" · ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distinctive_terms_win_over_shared_ones() {
        let docs = [
            (
                0,
                "Keratoconus crosslinking slows keratoconus progression in the cornea",
            ),
            (
                0,
                "Corneal crosslinking for keratoconus: outcomes in patients",
            ),
            (
                1,
                "Glaucoma trabeculectomy lowers intraocular pressure in glaucoma",
            ),
            (
                1,
                "Intraocular pressure after trabeculectomy for glaucoma patients",
            ),
        ];
        let t = ctfidf(docs, 2, 3);
        assert_eq!(t[0][0], "keratoconus");
        assert!(t[0].contains(&"crosslinking".to_string()));
        assert_eq!(t[1][0], "glaucoma");
        assert!(!t.concat().contains(&"patients".to_string()));
        assert_eq!(
            label(&t[1]),
            format!("{} · {} · {}", t[1][0], t[1][1], t[1][2])
        );
        assert_eq!(
            tokens("COVID-19 in 2020, β-blockers").collect::<Vec<_>>(),
            vec!["covid-19", "β-blockers"]
        );
    }
}
