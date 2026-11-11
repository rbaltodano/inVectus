//! Outside sources for verification (SPEC §6.6): PubMed (E-utilities), OpenAlex, Semantic
//! Scholar (Graph API, no key) and arXiv (Atom API; preprints, no review filter). Requests are
//! explicit, rate-limited, retried with backoff, refused offline, and cached in the project
//! (`cache/verify/`, 7 days), so repeating a verification needs no network.

use anyhow::{Context, Result, bail};
use invectus_core::Project;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const USER_AGENT: &str = concat!(
    "inVectus/",
    env!("CARGO_PKG_VERSION"),
    " (local research tool)"
);
const CACHE_DAYS: u64 = 7;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Source {
    PubMed,
    OpenAlex,
    SemanticScholar,
    Arxiv,
}

impl Source {
    pub fn name(self) -> &'static str {
        match self {
            Self::PubMed => "pubmed",
            Self::OpenAlex => "openalex",
            Self::SemanticScholar => "semanticscholar",
            Self::Arxiv => "arxiv",
        }
    }
    pub const ALL: [Source; 4] = [
        Self::PubMed,
        Self::OpenAlex,
        Self::SemanticScholar,
        Self::Arxiv,
    ];
    /// The query exactly as this source receives it.
    pub fn query(self, q: &KeywordQuery) -> String {
        match self {
            Self::PubMed => q.pubmed(),
            Self::OpenAlex => q.openalex(),
            Self::SemanticScholar => q.semantic_scholar(),
            Self::Arxiv => q.arxiv(),
        }
    }
    /// Minimum time between requests (PubMed allows 3 per second without a key).
    fn spacing(self) -> Duration {
        match self {
            Self::PubMed => Duration::from_millis(400),
            Self::OpenAlex => Duration::from_millis(150),
            // The unauthenticated pool is shared; one request a second avoids most 429s.
            Self::SemanticScholar => Duration::from_millis(1100),
            // arXiv asks for one request every three seconds.
            Self::Arxiv => Duration::from_millis(3000),
        }
    }
}

impl std::str::FromStr for Source {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self> {
        match s.to_ascii_lowercase().as_str() {
            "pubmed" => Ok(Self::PubMed),
            "openalex" => Ok(Self::OpenAlex),
            "semanticscholar" | "semantic-scholar" | "s2" => Ok(Self::SemanticScholar),
            "arxiv" => Ok(Self::Arxiv),
            other => bail!("unknown source {other:?} (pubmed, openalex, semanticscholar, arxiv)"),
        }
    }
}

/// One item an outside source returned.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ExternalHit {
    pub source: String,
    pub id: String,
    pub title: String,
    pub year: Option<i32>,
    pub url: Option<String>,
    /// Publication types (PubMed) or work type (OpenAlex).
    pub kinds: Vec<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SourceResult {
    pub source: String,
    /// The query exactly as sent.
    pub query: String,
    /// Total matches the source reported (only the first `hits` were fetched).
    pub total: Option<u64>,
    pub hits: Vec<ExternalHit>,
    pub cached: bool,
    pub fetched_at: u64,
}

/// Keyword search: OR within each group, AND between groups (SPEC §6.6: keyword search plus a
/// human read beats similarity alone).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct KeywordQuery {
    pub groups: Vec<Vec<String>>,
    /// Only reviews (coverage gaps ask whether a review exists).
    pub reviews_only: bool,
}

fn quote(term: &str) -> String {
    let t = term.replace('"', "");
    if t.contains([' ', '-']) {
        format!("\"{t}\"")
    } else {
        t
    }
}

impl KeywordQuery {
    fn boolean(&self, field: &str) -> String {
        self.groups
            .iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let terms: Vec<String> = g.iter().map(|t| format!("{}{field}", quote(t))).collect();
                format!("({})", terms.join(" OR "))
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }

    /// PubMed syntax: terms in title/abstract; reviews by publication type.
    pub fn pubmed(&self) -> String {
        let q = self.boolean("[tiab]");
        if self.reviews_only {
            format!("{q} AND (review[pt] OR systematic review[pt] OR meta-analysis[pt])")
        } else {
            q
        }
    }

    /// OpenAlex full-text search syntax (filters are separate parameters).
    pub fn openalex(&self) -> String {
        self.boolean("")
    }

    /// Semantic Scholar's bulk search takes `+` (AND), `|` (OR) and quoted phrases.
    pub fn semantic_scholar(&self) -> String {
        self.groups
            .iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let terms: Vec<String> = g.iter().map(|t| quote(t)).collect();
                format!("({})", terms.join(" | "))
            })
            .collect::<Vec<_>>()
            .join(" + ")
    }

    /// arXiv search_query: terms in title or abstract (`all:`), AND between groups.
    pub fn arxiv(&self) -> String {
        self.groups
            .iter()
            .filter(|g| !g.is_empty())
            .map(|g| {
                let terms: Vec<String> = g.iter().map(|t| format!("all:{}", quote(t))).collect();
                format!("({})", terms.join(" OR "))
            })
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

/// Parse an E-utilities esearch reply: (total count, PMIDs).
pub fn parse_pubmed_search(v: &serde_json::Value) -> Result<(u64, Vec<String>)> {
    let r = v
        .get("esearchresult")
        .context("no esearchresult in PubMed reply")?;
    let count = r["count"]
        .as_str()
        .and_then(|c| c.parse().ok())
        .unwrap_or(0);
    let ids = r["idlist"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|x| x.as_str().map(String::from))
                .collect()
        })
        .unwrap_or_default();
    Ok((count, ids))
}

/// Parse an E-utilities esummary reply into hits, in the order of `ids`.
pub fn parse_pubmed_summaries(v: &serde_json::Value, ids: &[String]) -> Vec<ExternalHit> {
    ids.iter()
        .filter_map(|id| {
            let d = &v["result"][id];
            let title = d["title"].as_str()?.trim().to_string();
            Some(ExternalHit {
                source: "pubmed".into(),
                id: id.clone(),
                year: d["pubdate"]
                    .as_str()
                    .and_then(|p| p.get(..4))
                    .and_then(|y| y.parse().ok()),
                url: Some(format!("https://pubmed.ncbi.nlm.nih.gov/{id}/")),
                kinds: d["pubtype"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                title,
            })
        })
        .collect()
}

/// Parse an OpenAlex `/works` reply: (total count, hits).
pub fn parse_openalex(v: &serde_json::Value) -> Result<(u64, Vec<ExternalHit>)> {
    let results = v["results"]
        .as_array()
        .context("no results in OpenAlex reply")?;
    let hits = results
        .iter()
        .filter_map(|w| {
            let title = w["display_name"]
                .as_str()
                .or(w["title"].as_str())?
                .trim()
                .to_string();
            let id = w["id"].as_str()?.to_string();
            Some(ExternalHit {
                source: "openalex".into(),
                url: w["doi"]
                    .as_str()
                    .map(String::from)
                    .or_else(|| Some(id.clone())),
                id,
                year: w["publication_year"].as_i64().map(|y| y as i32),
                kinds: w["type"]
                    .as_str()
                    .map(|t| vec![t.to_string()])
                    .unwrap_or_default(),
                title,
            })
        })
        .collect();
    Ok((v["meta"]["count"].as_u64().unwrap_or(0), hits))
}

/// Last request time per source, shared across threads.
static LAST: Mutex<[Option<Instant>; 4]> = Mutex::new([None; 4]);

fn get_json(source: Source, url: &str, params: &[(&str, &str)]) -> Result<serde_json::Value> {
    invectus_core::net::ensure_online(&format!("{} verification", source.name()))?;
    let mut last_err = None;
    for attempt in 0..3 {
        {
            let mut last = LAST.lock().unwrap();
            let slot = &mut last[source as usize];
            if let Some(t) = *slot {
                let wait = source.spacing().saturating_sub(t.elapsed());
                std::thread::sleep(wait);
            }
            *slot = Some(Instant::now());
        }
        let mut req = ureq::get(url).header("User-Agent", USER_AGENT);
        for (k, v) in params {
            req = req.query(*k, *v);
        }
        match req.call() {
            Ok(resp) => {
                let body = resp.into_body().read_to_string()?;
                return serde_json::from_str(&body)
                    .with_context(|| format!("{} returned invalid JSON", source.name()));
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_secs(1 << attempt));
            }
        }
    }
    Err(last_err.expect("at least one attempt")).with_context(|| format!("GET {url}"))
}

fn cache_path(project: &Project, source: Source, key: &str) -> PathBuf {
    let h = hex::encode(Sha256::digest(key.as_bytes()));
    project
        .root
        .join("cache")
        .join("verify")
        .join(source.name())
        .join(format!("{}.json", &h[..32]))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// Search one source (from the cache when a fresh answer exists).
pub fn search(
    project: &Project,
    source: Source,
    query: &KeywordQuery,
    max: usize,
) -> Result<SourceResult> {
    let q = source.query(query);
    let key = format!("{}|{q}|{}|{max}", source.name(), query.reviews_only);
    let path = cache_path(project, source, &key);
    if let Ok(bytes) = std::fs::read(&path)
        && let Ok(mut r) = serde_json::from_slice::<SourceResult>(&bytes)
        && now().saturating_sub(r.fetched_at) < CACHE_DAYS * 86_400
    {
        r.cached = true;
        return Ok(r);
    }
    let max_s = max.to_string();
    let result = match source {
        Source::PubMed => {
            let base = "https://eutils.ncbi.nlm.nih.gov/entrez/eutils";
            let found = get_json(
                source,
                &format!("{base}/esearch.fcgi"),
                &[
                    ("db", "pubmed"),
                    ("term", &q),
                    ("retmode", "json"),
                    ("retmax", &max_s),
                    ("sort", "relevance"),
                    ("tool", "invectus"),
                ],
            )?;
            let (total, ids) = parse_pubmed_search(&found)?;
            let hits = if ids.is_empty() {
                Vec::new()
            } else {
                let joined = ids.join(",");
                let summaries = get_json(
                    source,
                    &format!("{base}/esummary.fcgi"),
                    &[
                        ("db", "pubmed"),
                        ("id", &joined),
                        ("retmode", "json"),
                        ("tool", "invectus"),
                    ],
                )?;
                parse_pubmed_summaries(&summaries, &ids)
            };
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
        Source::SemanticScholar => {
            let mut params = vec![
                ("query", q.as_str()),
                ("fields", "title,year,externalIds,url,publicationTypes"),
            ];
            if query.reviews_only {
                params.push(("publicationTypes", "Review"));
            }
            let v = get_json(
                source,
                "https://api.semanticscholar.org/graph/v1/paper/search/bulk",
                &params,
            )?;
            let (total, mut hits) = parse_semantic_scholar(&v)?;
            hits.truncate(max);
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
        Source::Arxiv => {
            // arXiv has no review filter: a reviews-only check still lists preprints.
            let xml = get_text(
                source,
                "https://export.arxiv.org/api/query",
                &[
                    ("search_query", q.as_str()),
                    ("max_results", max_s.as_str()),
                    ("sortBy", "relevance"),
                ],
            )?;
            let (total, hits) = parse_arxiv(&xml)?;
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
        Source::OpenAlex => {
            let mut params = vec![("search", q.as_str()), ("per-page", max_s.as_str())];
            if query.reviews_only {
                params.push(("filter", "type:review"));
            }
            let v = get_json(source, "https://api.openalex.org/works", &params)?;
            let (total, hits) = parse_openalex(&v)?;
            SourceResult {
                source: source.name().into(),
                query: q,
                total: Some(total),
                hits,
                cached: false,
                fetched_at: now(),
            }
        }
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(&path, serde_json::to_vec(&result)?)?;
    Ok(result)
}

/// Parse a Semantic Scholar bulk-search reply: (total, hits). Reviews are marked by
/// `publicationTypes` ("Review").
pub fn parse_semantic_scholar(v: &serde_json::Value) -> Result<(u64, Vec<ExternalHit>)> {
    let data = v["data"]
        .as_array()
        .context("no data in Semantic Scholar reply")?;
    let hits = data
        .iter()
        .filter_map(|p| {
            let id = p["paperId"].as_str()?.to_string();
            let title = p["title"].as_str()?.trim().to_string();
            let doi = p["externalIds"]["DOI"].as_str();
            Some(ExternalHit {
                source: "semanticscholar".into(),
                url: doi
                    .map(|d| format!("https://doi.org/{d}"))
                    .or_else(|| p["url"].as_str().map(String::from))
                    .or_else(|| Some(format!("https://www.semanticscholar.org/paper/{id}"))),
                year: p["year"].as_i64().map(|y| y as i32),
                kinds: p["publicationTypes"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|t| t.as_str().map(String::from))
                            .collect()
                    })
                    .unwrap_or_default(),
                id,
                title,
            })
        })
        .collect();
    Ok((v["total"].as_u64().unwrap_or(0), hits))
}

/// Parse an arXiv Atom feed: (total, hits). A small reader for the fixed feed shape, not a
/// general XML parser.
pub fn parse_arxiv(xml: &str) -> Result<(u64, Vec<ExternalHit>)> {
    if !xml.contains("<feed") {
        bail!("arXiv did not return a feed");
    }
    fn tag<'a>(s: &'a str, name: &str) -> Option<&'a str> {
        let open = s.find(&format!("<{name}"))?;
        let start = open + s[open..].find('>')? + 1;
        let end = start + s[start..].find(&format!("</{name}>"))?;
        Some(&s[start..end])
    }
    fn unescape(s: &str) -> String {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .replace("&lt;", "<")
            .replace("&gt;", ">")
            .replace("&quot;", "\"")
            .replace("&apos;", "'")
            .replace("&amp;", "&")
    }
    let total = tag(xml, "opensearch:totalResults")
        .and_then(|t| t.trim().parse().ok())
        .unwrap_or(0);
    let hits = xml
        .split("<entry>")
        .skip(1)
        .filter_map(|e| {
            let url = tag(e, "id")?.trim().to_string();
            let id = url.rsplit("/abs/").next().unwrap_or(&url).to_string();
            Some(ExternalHit {
                source: "arxiv".into(),
                title: unescape(tag(e, "title")?),
                year: tag(e, "published").and_then(|p| p.get(..4)?.parse().ok()),
                kinds: vec!["preprint".into()],
                url: Some(url),
                id,
            })
        })
        .collect();
    Ok((total, hits))
}

fn get_text(source: Source, url: &str, params: &[(&str, &str)]) -> Result<String> {
    get_text_with(source, url, params, &[])
}

fn get_text_with(
    source: Source,
    url: &str,
    params: &[(&str, &str)],
    headers: &[(&str, &str)],
) -> Result<String> {
    invectus_core::net::ensure_online(&format!("{} verification", source.name()))?;
    let mut last_err = None;
    for attempt in 0..3 {
        {
            let mut last = LAST.lock().unwrap();
            let slot = &mut last[source as usize];
            if let Some(t) = *slot {
                std::thread::sleep(source.spacing().saturating_sub(t.elapsed()));
            }
            *slot = Some(Instant::now());
        }
        let mut req = ureq::get(url).header("User-Agent", USER_AGENT);
        for (k, v) in params {
            req = req.query(*k, *v);
        }
        for (k, v) in headers {
            req = req.header(*k, *v);
        }
        match req.call() {
            Ok(resp) => return Ok(resp.into_body().read_to_string()?),
            Err(ureq::Error::StatusCode(code @ (401 | 403))) => {
                bail!("{} refused the API key (HTTP {code})", source.name())
            }
            Err(e) => {
                last_err = Some(e);
                std::thread::sleep(Duration::from_secs(2 << attempt));
            }
        }
    }
    Err(last_err.expect("at least one attempt")).with_context(|| format!("GET {url}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queries_use_each_sources_syntax() {
        let q = KeywordQuery {
            groups: vec![
                vec!["corneal".into(), "cross-linking".into()],
                vec!["choroidal thickness".into()],
            ],
            reviews_only: true,
        };
        assert_eq!(
            q.pubmed(),
            "(corneal[tiab] OR \"cross-linking\"[tiab]) AND (\"choroidal thickness\"[tiab]) \
             AND (review[pt] OR systematic review[pt] OR meta-analysis[pt])"
        );
        assert_eq!(
            q.openalex(),
            "(corneal OR \"cross-linking\") AND (\"choroidal thickness\")"
        );
        assert_eq!(
            q.semantic_scholar(),
            "(corneal | \"cross-linking\") + (\"choroidal thickness\")"
        );
        assert_eq!(
            q.arxiv(),
            "(all:corneal OR all:\"cross-linking\") AND (all:\"choroidal thickness\")"
        );
    }

    #[test]
    fn pubmed_and_openalex_replies_parse() {
        let search =
            serde_json::json!({"esearchresult": {"count": "42", "idlist": ["111", "222"]}});
        assert_eq!(
            parse_pubmed_search(&search).unwrap(),
            (42, vec!["111".to_string(), "222".to_string()])
        );
        let summary = serde_json::json!({"result": {
            "uids": ["111", "222"],
            "111": {"title": "A review of things.", "pubdate": "2021 Mar 3",
                    "pubtype": ["Journal Article", "Review"]},
            "222": {"title": "Another", "pubdate": ""}
        }});
        let hits = parse_pubmed_summaries(&summary, &["111".into(), "222".into(), "333".into()]);
        assert_eq!(hits.len(), 2);
        assert_eq!(hits[0].year, Some(2021));
        assert_eq!(hits[0].kinds, vec!["Journal Article", "Review"]);
        assert_eq!(
            hits[0].url.as_deref(),
            Some("https://pubmed.ncbi.nlm.nih.gov/111/")
        );
        assert_eq!(hits[1].year, None);
        let oa = serde_json::json!({"meta": {"count": 7}, "results": [
            {"id": "https://openalex.org/W1", "display_name": "Work one", "publication_year": 2019,
             "doi": "https://doi.org/10.1/x", "type": "review"},
            {"id": "https://openalex.org/W2", "title": "Work two"}
        ]});
        let (n, hits) = parse_openalex(&oa).unwrap();
        assert_eq!(n, 7);
        assert_eq!(hits[0].url.as_deref(), Some("https://doi.org/10.1/x"));
        assert_eq!(hits[1].url.as_deref(), Some("https://openalex.org/W2"));
        assert!(parse_openalex(&serde_json::json!({})).is_err());
        let s2 = serde_json::json!({"total": 12, "data": [
            {"paperId": "abc", "title": "A survey", "year": 2022,
             "externalIds": {"DOI": "10.2/y"}, "publicationTypes": ["Review"]},
            {"paperId": "def", "title": "No doi", "year": null, "externalIds": {}},
            {"paperId": "ghi"}
        ]});
        let (n, hits) = parse_semantic_scholar(&s2).unwrap();
        assert_eq!((n, hits.len()), (12, 2));
        assert_eq!(hits[0].url.as_deref(), Some("https://doi.org/10.2/y"));
        assert_eq!(hits[0].kinds, vec!["Review"]);
        assert_eq!(
            hits[1].url.as_deref(),
            Some("https://www.semanticscholar.org/paper/def")
        );
        let feed = r#"<?xml version="1.0"?><feed xmlns="http://www.w3.org/2005/Atom">
          <opensearch:totalResults xmlns:opensearch="http://a9.com/-/spec/opensearch/1.1/">5</opensearch:totalResults>
          <entry><id>http://arxiv.org/abs/2101.00001v2</id><published>2021-01-01T00:00:00Z</published>
          <title>Corneal   cross-linking
            &amp; thickness</title></entry>
          <entry><id>http://arxiv.org/abs/2202.00002v1</id><title>Second</title></entry>
        </feed>"#;
        let (n, hits) = parse_arxiv(feed).unwrap();
        assert_eq!((n, hits.len()), (5, 2));
        assert_eq!(hits[0].title, "Corneal cross-linking & thickness");
        assert_eq!(hits[0].id, "2101.00001v2");
        assert_eq!(hits[0].year, Some(2021));
        assert_eq!(hits[1].year, None);
        assert!(parse_arxiv("<html>").is_err());
    }

    #[test]
    fn offline_mode_refuses_uncached_searches_but_serves_the_cache() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::create(&dir.path().join("t")).unwrap();
        let q = KeywordQuery {
            groups: vec![vec!["x".into()]],
            reviews_only: false,
        };
        // SAFETY: tests in this crate do not read the variable concurrently.
        unsafe { std::env::set_var("INVECTUS_OFFLINE", "1") };
        let err = search(&p, Source::OpenAlex, &q, 5).unwrap_err().to_string();
        assert!(err.contains("offline"), "{err}");
        let cached = SourceResult {
            source: "openalex".into(),
            query: q.openalex(),
            total: Some(1),
            hits: vec![],
            cached: false,
            fetched_at: now(),
        };
        let path = cache_path(
            &p,
            Source::OpenAlex,
            &format!("openalex|{}|false|5", q.openalex()),
        );
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, serde_json::to_vec(&cached).unwrap()).unwrap();
        let r = search(&p, Source::OpenAlex, &q, 5).unwrap();
        assert!(r.cached && r.total == Some(1));
        unsafe { std::env::remove_var("INVECTUS_OFFLINE") };
    }
}
