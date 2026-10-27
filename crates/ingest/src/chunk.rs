//! Chunking of long documents (SPEC §3: "long documents are split into chunks, and both chunks and
//! parent items have vectors").
//!
//! By default chunks are sized in characters, so import does not need a tokenizer. A policy can
//! instead size chunks in one model's tokens ([`TokenChunking`]), which keeps every chunk inside
//! that model's token cap. Either way the embedder enforces the cap (head truncation) and reports
//! truncation counts.

use anyhow::{Result, bail};
use invectus_core::TokenOffsets;
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkPolicy {
    /// Texts longer than this many characters are split; 0 disables chunking. Unused when
    /// `tokens` is set.
    pub max_chars: usize,
    /// Characters repeated at the start of the next chunk.
    pub overlap_chars: usize,
    /// Size chunks in a model's tokens instead of characters. Omitted from the stored policy when
    /// unset, so collections imported before this option keep their content hashes.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tokens: Option<TokenChunking>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenChunking {
    /// Registry key of the model whose tokenizer measures chunks.
    pub model_key: String,
    pub tokenizer_sha256: String,
    /// Texts with more tokens than this are split (excluding special tokens and prefixes).
    pub max_tokens: usize,
    /// Tokens repeated at the start of the next chunk.
    pub overlap_tokens: usize,
}

impl TokenChunking {
    /// 448 tokens leaves room for the title, a document prefix and special tokens inside a
    /// 512-token model cap.
    pub const DEFAULT_MAX_TOKENS: usize = 448;
    pub const DEFAULT_OVERLAP_TOKENS: usize = 48;
}

impl Default for ChunkPolicy {
    /// ~2,000 characters is roughly 400-500 English tokens, inside a 512-token model cap.
    fn default() -> Self {
        Self {
            max_chars: 2000,
            overlap_chars: 200,
            tokens: None,
        }
    }
}

impl ChunkPolicy {
    /// Token-sized chunks for the tokenizer `t`.
    pub fn tokens(t: &dyn TokenOffsets, max_tokens: usize, overlap_tokens: usize) -> Self {
        Self {
            max_chars: 0,
            overlap_chars: 0,
            tokens: Some(TokenChunking {
                model_key: t.model_key().into(),
                tokenizer_sha256: t.tokenizer_sha256().into(),
                max_tokens,
                overlap_tokens,
            }),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if let Some(t) = &self.tokens {
            if t.max_tokens == 0 || t.overlap_tokens * 2 >= t.max_tokens {
                bail!("token chunks need a positive size and an overlap under half of it");
            }
        } else if self.max_chars != 0 && self.overlap_chars * 2 >= self.max_chars {
            bail!("chunk overlap must be less than half of the chunk size");
        }
        Ok(())
    }

    /// Check that `t` is the tokenizer this policy was defined with.
    pub fn check_tokenizer(&self, t: Option<&dyn TokenOffsets>) -> Result<()> {
        let Some(want) = &self.tokens else {
            return Ok(());
        };
        match t {
            None => bail!(
                "this collection is chunked by {} tokens; its tokenizer must be installed to import",
                want.model_key
            ),
            Some(t) if t.tokenizer_sha256() != want.tokenizer_sha256 => bail!(
                "tokenizer {} (SHA-256 {}) is not the one this collection is chunked with ({})",
                t.model_key(),
                t.tokenizer_sha256(),
                want.tokenizer_sha256
            ),
            Some(_) => Ok(()),
        }
    }

    /// Split `text` into chunks, or return an empty vector when it fits in one.
    pub fn split(&self, text: &str, tokenizer: Option<&dyn TokenOffsets>) -> Result<Vec<String>> {
        match &self.tokens {
            None => Ok(self.split_chars(text)),
            Some(t) => {
                self.check_tokenizer(tokenizer)?;
                let offsets = tokenizer.expect("checked above").offsets(text)?;
                Ok(split_tokens(text, &offsets, t.max_tokens, t.overlap_tokens))
            }
        }
    }

    /// Character-sized split.
    pub fn split_chars(&self, text: &str) -> Vec<String> {
        let chars: Vec<char> = text.chars().collect();
        if self.max_chars == 0 || chars.len() <= self.max_chars {
            return Vec::new();
        }
        let mut out = Vec::new();
        let mut start = 0;
        while start < chars.len() {
            let hard_end = (start + self.max_chars).min(chars.len());
            let end = if hard_end == chars.len() {
                hard_end
            } else {
                boundary(&chars, start + self.max_chars / 2, hard_end)
            };
            let piece: String = chars[start..end].iter().collect();
            let piece = piece.trim();
            if !piece.is_empty() {
                out.push(piece.to_string());
            }
            if end == chars.len() {
                break;
            }
            // Step back by the overlap, then forward to a word start so chunks begin cleanly.
            let mut next = end.saturating_sub(self.overlap_chars).max(start + 1);
            while next < end && !chars[next - 1].is_whitespace() {
                next += 1;
            }
            start = next;
        }
        out
    }
}

/// Best split point in `(lo, hi]`: paragraph break, then sentence end, then whitespace, else `hi`.
fn boundary(chars: &[char], lo: usize, hi: usize) -> usize {
    let window = lo..hi;
    let after =
        |pred: &dyn Fn(usize) -> bool| window.clone().rev().find(|&i| pred(i)).map(|i| i + 1);
    after(&|i| chars[i] == '\n' && i > 0 && chars[i - 1] == '\n')
        .or_else(|| {
            after(&|i| {
                matches!(chars[i], '.' | '!' | '?')
                    && chars.get(i + 1).is_some_and(|c| c.is_whitespace())
            })
        })
        .or_else(|| after(&|i| chars[i].is_whitespace()))
        .unwrap_or(hi)
}

/// Split by token count. `offsets` are the byte ranges of `text`'s tokens. Chunks end at a
/// paragraph break, then a sentence end, then a word boundary in their second half, and the next
/// chunk starts `overlap` tokens back at a word start.
fn split_tokens(text: &str, offsets: &[(usize, usize)], max: usize, overlap: usize) -> Vec<String> {
    let n = offsets.len();
    if n <= max {
        return Vec::new();
    }
    // Text between token i-1 and token i.
    let gap = |i: usize| text.get(offsets[i - 1].1..offsets[i].0).unwrap_or("");
    let mut out = Vec::new();
    let mut start = 0;
    while start < n {
        let hard_end = (start + max).min(n);
        let end = if hard_end == n {
            n
        } else {
            token_boundary(text, offsets, start + max / 2, hard_end)
        };
        let piece = text
            .get(offsets[start].0..offsets[end - 1].1)
            .unwrap_or("")
            .trim();
        if !piece.is_empty() {
            out.push(piece.to_string());
        }
        if end == n {
            break;
        }
        let mut next = end.saturating_sub(overlap).max(start + 1);
        while next < end && !gap(next).chars().any(char::is_whitespace) {
            next += 1;
        }
        start = next;
    }
    out
}

/// Best cut `e` in `(lo, hi]` (the chunk ends before token `e`; `hi` < token count).
fn token_boundary(text: &str, offsets: &[(usize, usize)], lo: usize, hi: usize) -> usize {
    let gap = |i: usize| text.get(offsets[i - 1].1..offsets[i].0).unwrap_or("");
    let ends_sentence = |i: usize| {
        text.get(offsets[i - 1].0..offsets[i - 1].1)
            .is_some_and(|t| t.ends_with(['.', '!', '?']))
    };
    let spaced = |i: usize| gap(i).chars().any(char::is_whitespace);
    let find = |pred: &dyn Fn(usize) -> bool| ((lo + 1)..=hi).rev().find(|&i| pred(i));
    find(&|i| gap(i).contains("\n\n"))
        .or_else(|| find(&|i| ends_sentence(i) && spaced(i)))
        .or_else(|| find(&spaced))
        .unwrap_or(hi)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Splits on whitespace and before punctuation, like a word-level tokenizer.
    struct Words;

    impl TokenOffsets for Words {
        fn model_key(&self) -> &str {
            "words"
        }
        fn tokenizer_sha256(&self) -> &str {
            "w1"
        }
        fn offsets(&self, text: &str) -> Result<Vec<(usize, usize)>> {
            let mut out = Vec::new();
            let mut start = None;
            for (i, c) in text.char_indices() {
                if c.is_whitespace() || matches!(c, '.' | ',') {
                    if let Some(s) = start.take() {
                        out.push((s, i));
                    }
                    if !c.is_whitespace() {
                        out.push((i, i + c.len_utf8()));
                    }
                } else if start.is_none() {
                    start = Some(i);
                }
            }
            if let Some(s) = start {
                out.push((s, text.len()));
            }
            Ok(out)
        }
    }

    #[test]
    fn token_chunks_respect_the_token_budget() {
        let p = ChunkPolicy::tokens(&Words, 30, 6);
        p.validate().unwrap();
        let sentence = "Axial length grew less with ortho-k lenses in children. ";
        let text = format!(
            "{}\n\nSecond part: {}",
            sentence.repeat(5),
            sentence.repeat(6)
        );
        let chunks = p.split(&text, Some(&Words)).unwrap();
        assert!(chunks.len() >= 3, "{chunks:?}");
        for c in &chunks {
            let n = Words.offsets(c).unwrap().len();
            assert!(n <= 30, "{n} tokens: {c:?}");
            assert!(!c.starts_with(' ') && !c.ends_with(' '));
        }
        // Prefers sentence ends; the paragraph break is used when it falls in the window.
        assert!(chunks[0].ends_with('.'), "{:?}", chunks[0]);
        for w in chunks.windows(2) {
            let head: String = w[1]
                .split_whitespace()
                .take(2)
                .collect::<Vec<_>>()
                .join(" ");
            assert!(w[0].contains(&head), "overlap: {:?} / {:?}", w[0], w[1]);
        }
        assert!(chunks.last().unwrap().ends_with("children."));
        // Text inside the budget is not chunked; non-ASCII text slices on char boundaries.
        assert!(p.split("short text", Some(&Words)).unwrap().is_empty());
        let accents = "Kératocône évolutif traité par crosslinking cornéen. ".repeat(10);
        let chunks = p.split(&accents, Some(&Words)).unwrap();
        assert!(chunks.len() >= 2 && chunks.iter().all(|c| c.ends_with('.')));
    }

    #[test]
    fn token_policy_requires_its_tokenizer() {
        let p = ChunkPolicy::tokens(&Words, 30, 6);
        let err = p.split("x", None).unwrap_err().to_string();
        assert!(err.contains("tokenizer must be installed"), "{err}");
        struct Other;
        impl TokenOffsets for Other {
            fn model_key(&self) -> &str {
                "other"
            }
            fn tokenizer_sha256(&self) -> &str {
                "o2"
            }
            fn offsets(&self, _: &str) -> Result<Vec<(usize, usize)>> {
                Ok(vec![])
            }
        }
        assert!(p.split("x", Some(&Other)).is_err());
        assert!(ChunkPolicy::tokens(&Words, 10, 5).validate().is_err());
    }

    #[test]
    fn char_policy_serializes_without_a_tokens_field() {
        // Keeps the content hashes of collections imported before token chunking existed.
        let v = serde_json::to_value(ChunkPolicy::default()).unwrap();
        assert_eq!(
            v,
            serde_json::json!({"max_chars": 2000, "overlap_chars": 200})
        );
        let back: ChunkPolicy = serde_json::from_value(v).unwrap();
        assert_eq!(back, ChunkPolicy::default());
    }

    #[test]
    fn short_text_is_not_chunked() {
        assert!(ChunkPolicy::default().split_chars("short").is_empty());
        let off = ChunkPolicy {
            max_chars: 0,
            overlap_chars: 0,
            tokens: None,
        };
        assert!(off.split_chars(&"x".repeat(10_000)).is_empty());
    }

    #[test]
    fn chunks_cover_text_with_bounded_size_and_overlap() {
        let p = ChunkPolicy {
            max_chars: 100,
            overlap_chars: 20,
            tokens: None,
        };
        let sentence = "The quick brown fox jumps over the lazy dog. ";
        let text = sentence.repeat(12);
        let chunks = p.split(&text, None).unwrap();
        assert!(chunks.len() >= 6, "{}", chunks.len());
        for c in &chunks {
            assert!(c.chars().count() <= 100);
            assert!(!c.starts_with(' '));
        }
        // Sentence boundaries are preferred when available.
        assert!(chunks[0].ends_with('.'), "{:?}", chunks[0]);
        // Consecutive chunks overlap: the next chunk starts inside the previous one's tail.
        for w in chunks.windows(2) {
            let head: String = w[1].chars().take(10).collect();
            assert!(w[0].contains(&head), "{:?} / {:?}", w[0], w[1]);
        }
        // Every word of the source appears in some chunk.
        assert!(chunks.last().unwrap().ends_with("dog."));
    }

    #[test]
    fn unbroken_text_is_hard_split() {
        let p = ChunkPolicy {
            max_chars: 50,
            overlap_chars: 0,
            tokens: None,
        };
        let chunks = p.split_chars(&"x".repeat(120));
        assert_eq!(
            chunks.iter().map(|c| c.len()).collect::<Vec<_>>(),
            vec![50, 50, 20]
        );
    }

    #[test]
    fn rejects_excessive_overlap() {
        assert!(
            ChunkPolicy {
                max_chars: 100,
                overlap_chars: 50,
                tokens: None,
            }
            .validate()
            .is_err()
        );
    }
}
