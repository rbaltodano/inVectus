//! Items, roles and collections (SPEC §3).

use anyhow::{Result, bail};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    Demand,
    Coverage,
    Neutral,
}

impl Role {
    pub fn as_str(self) -> &'static str {
        match self {
            Role::Demand => "demand",
            Role::Coverage => "coverage",
            Role::Neutral => "neutral",
        }
    }
}

impl std::str::FromStr for Role {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "demand" => Ok(Role::Demand),
            "coverage" => Ok(Role::Coverage),
            "neutral" => Ok(Role::Neutral),
            other => bail!("unknown role {other:?} (expected demand, coverage or neutral)"),
        }
    }
}

/// One item as stored in `items`. Chunks are items with a `parent_id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Item {
    pub item_id: i64,
    pub collection_id: String,
    pub source_id: String,
    pub title: Option<String>,
    pub text: String,
    pub url: Option<String>,
    /// ISO `YYYY-MM-DD`.
    pub date: Option<String>,
    pub role: Role,
    pub meta: serde_json::Value,
    pub parent_id: Option<i64>,
    pub chunk_index: Option<i32>,
    /// Hash of the stored content (title, text, url, date, role, meta); detects changed rows.
    pub input_hash: String,
}

impl Item {
    pub fn content_hash(
        title: Option<&str>,
        text: &str,
        url: Option<&str>,
        date: Option<&str>,
        role: Role,
        meta: &serde_json::Value,
    ) -> String {
        let mut h = Sha256::new();
        for part in [
            title.unwrap_or("\u{1}"),
            text,
            url.unwrap_or("\u{1}"),
            date.unwrap_or("\u{1}"),
            role.as_str(),
        ] {
            h.update(part.as_bytes());
            h.update([0]);
        }
        h.update(meta.to_string().as_bytes());
        hex::encode(&h.finalize()[..16])
    }
}

/// Deterministic 63-bit item id from collection and source id, so re-imports are idempotent.
pub fn item_id(collection_id: &str, source_id: &str, chunk: Option<usize>) -> i64 {
    let mut h = Sha256::new();
    h.update(collection_id.as_bytes());
    h.update([0]);
    h.update(source_id.as_bytes());
    if let Some(c) = chunk {
        h.update(b"\0chunk\0");
        h.update(c.to_le_bytes());
    }
    let d = h.finalize();
    (u64::from_le_bytes(d[..8].try_into().unwrap()) >> 1) as i64
}

/// Collection names become table-name fragments, so restrict them.
pub fn validate_collection_name(name: &str) -> Result<()> {
    if name.is_empty()
        || name.len() > 64
        || !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("collection name {name:?} must be 1-64 characters of A-Z, a-z, 0-9, '_' or '-'");
    }
    Ok(())
}

/// Normalize a date string to `YYYY-MM-DD`. Accepts `YYYY`, `YYYY-MM`, `YYYY-MM-DD`, `YYYY/MM/DD`
/// and timestamps starting with a date. Partial dates take the first day of the period.
pub fn parse_date(raw: &str) -> Option<String> {
    let s = raw.trim();
    let head: String = s.chars().take(10).collect();
    let parts: Vec<&str> = head
        .split(['-', '/'])
        .take_while(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
        .collect();
    let num = |p: &str| p.parse::<u32>().ok();
    let (y, m, d) = match parts.as_slice() {
        [y] if y.len() == 4 && s.len() == 4 => (num(y)?, 1, 1),
        [y, m] if y.len() == 4 && m.len() <= 2 => (num(y)?, num(m)?, 1),
        [y, m, d, ..] if y.len() == 4 && m.len() <= 2 && d.len() <= 2 => {
            (num(y)?, num(m)?, num(d)?)
        }
        _ => return None,
    };
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days = [
        31,
        if leap { 29 } else { 28 },
        31,
        30,
        31,
        30,
        31,
        31,
        30,
        31,
        30,
        31,
    ];
    if !(1000..=9999).contains(&y) || !(1..=12).contains(&m) || d < 1 || d > days[m as usize - 1] {
        return None;
    }
    Some(format!("{y:04}-{m:02}-{d:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_normalize_or_reject() {
        assert_eq!(parse_date("2021").as_deref(), Some("2021-01-01"));
        assert_eq!(parse_date("2021-3").as_deref(), Some("2021-03-01"));
        assert_eq!(parse_date("2021/03/09").as_deref(), Some("2021-03-09"));
        assert_eq!(
            parse_date("2020-02-29 10:00:00").as_deref(),
            Some("2020-02-29")
        );
        for bad in [
            "",
            "21",
            "2021-13-01",
            "2019-02-29",
            "March 2021",
            "20210",
            "abcd",
        ] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn ids_are_stable_and_distinct() {
        assert_eq!(item_id("c", "1", None), item_id("c", "1", None));
        assert_ne!(item_id("c", "1", None), item_id("d", "1", None));
        assert_ne!(item_id("c", "1", None), item_id("c", "1", Some(0)));
        assert!(item_id("c", "1", None) >= 0);
    }

    #[test]
    fn roles_parse() {
        assert_eq!("Coverage".parse::<Role>().unwrap(), Role::Coverage);
        assert!("review".parse::<Role>().is_err());
    }

    #[test]
    fn collection_names_are_restricted() {
        assert!(validate_collection_name("papers_2024").is_ok());
        for bad in ["", "a b", "x/y", "é"] {
            assert!(validate_collection_name(bad).is_err());
        }
    }
}
