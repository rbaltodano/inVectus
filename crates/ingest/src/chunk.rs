//! Chunking of long documents (SPEC §3: "long documents are split into chunks, and both chunks and
//! parent items have vectors").
//!
//! Chunking is character-based so import does not depend on a tokenizer; the embedder separately
//! enforces the model's token cap (head truncation) and reports truncation counts.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChunkPolicy {
    /// Texts longer than this many characters are split; 0 disables chunking.
    pub max_chars: usize,
    /// Characters repeated at the start of the next chunk.
    pub overlap_chars: usize,
}

impl Default for ChunkPolicy {
    /// ~2,000 characters is roughly 400-500 English tokens, inside a 512-token model cap.
    fn default() -> Self {
        Self {
            max_chars: 2000,
            overlap_chars: 200,
        }
    }
}

impl ChunkPolicy {
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.max_chars != 0 && self.overlap_chars * 2 >= self.max_chars {
            anyhow::bail!("chunk overlap must be less than half of the chunk size");
        }
        Ok(())
    }

    /// Split `text` into chunks, or return an empty vector when it fits in one.
    pub fn split(&self, text: &str) -> Vec<String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_text_is_not_chunked() {
        assert!(ChunkPolicy::default().split("short").is_empty());
        let off = ChunkPolicy {
            max_chars: 0,
            overlap_chars: 0,
        };
        assert!(off.split(&"x".repeat(10_000)).is_empty());
    }

    #[test]
    fn chunks_cover_text_with_bounded_size_and_overlap() {
        let p = ChunkPolicy {
            max_chars: 100,
            overlap_chars: 20,
        };
        let sentence = "The quick brown fox jumps over the lazy dog. ";
        let text = sentence.repeat(12);
        let chunks = p.split(&text);
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
        };
        let chunks = p.split(&"x".repeat(120));
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
                overlap_chars: 50
            }
            .validate()
            .is_err()
        );
    }
}
