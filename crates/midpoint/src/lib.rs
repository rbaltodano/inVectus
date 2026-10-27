//! Midpoint and gap descriptions (SPEC §6.3): a language model describes the item that would sit
//! at a point of the vector space, and a round trip checks the description lands there and does
//! not already exist.
//!
//! Targets:
//! - **Between two regions** (bridge gaps): the point is the spherical interpolation (slerp) of
//!   the two centroids at `t`. The description is *on target* when its vector is closer to that
//!   point than to either centroid.
//! - **In one region** (coverage and stale gaps): the missing coverage item for the region's
//!   demand. The description is on target when its nearest fine region is that region.
//!
//! It is *novel* when an inVectus check of it (calibrated, exhaustive) is not `exists`. For a
//! combination of two regions only an item that reaches **both** regions can count as the same
//! thing: a description of A×B always sits close to strong papers about A alone. Up to `tries` descriptions are generated: the first deterministic, later ones
//! sampled. The model sees titles and roles only, never full texts.

use anyhow::{Context, Result, bail};
use invectus_core::store::{RegionRecord, VectorTableRecord};
use invectus_core::{InputKind, Project, Role, TextEmbedder};
use invectus_index::VectorStore;
use invectus_index::search::collapse;
use invectus_llm::{GenOptions, LlmIdentity, TextGenerator};
use invectus_unsearch::{CheckOptions, Verdict, check};
use serde::{Deserialize, Serialize};
use std::time::Instant;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Target {
    /// Between two fine regions at `t` (0 = region `a`, 1 = region `b`).
    Between { a: i32, b: i32, t: f32 },
    /// The missing coverage of one fine region.
    Region { region: i32 },
}

#[derive(Clone, Debug)]
pub struct DescribeOptions {
    /// What one item of this collection is, e.g. "research study" or "game".
    pub genre: String,
    /// What a coverage item is, e.g. "systematic review".
    pub coverage_genre: String,
    pub tries: usize,
    /// Titles shown to the model per side.
    pub context: usize,
}

impl Default for DescribeOptions {
    fn default() -> Self {
        Self {
            genre: "item".into(),
            coverage_genre: "review or synthesis".into(),
            tries: 3,
            context: 6,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OnTarget {
    pub passed: bool,
    /// Between: similarity to the midpoint and to each centroid. Region: similarity to the
    /// gap's region and to the nearest region.
    pub to_target: f32,
    pub to_a: Option<f32>,
    pub to_b: Option<f32>,
    pub nearest_region: Option<i32>,
    pub nearest_region_similarity: Option<f32>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Novelty {
    pub passed: bool,
    pub verdict: String,
    pub p_exists: f64,
    /// The nearest item that counts as the same thing (for a combination: one that reaches
    /// both regions).
    pub nearest_title: Option<String>,
    pub nearest_similarity: Option<f32>,
    /// `single` (nearest item) or `combination` (nearest item reaching both regions).
    #[serde(default)]
    pub rule: String,
    /// For combinations: the nearest item overall when it reaches only one side, which the
    /// single-item rule would have counted.
    #[serde(default)]
    pub one_sided_nearest: Option<(String, f32)>,
}

/// One retrieved item for the combination rule.
#[derive(Clone, Debug)]
pub struct Candidate {
    pub title: String,
    /// Similarity to the description.
    pub similarity: f32,
    /// Similarity to each region's centroid.
    pub to_a: f32,
    pub to_b: f32,
}

/// Novelty of a description of a *combination* of regions A and B: only an item that reaches
/// both regions (at least as central as each region's 25th-percentile member, the scan's reach
/// rule) can be the same thing. Strong items about one side alone are reported, not counted.
/// `predict` maps a nearest similarity to P(exists) with the collection's calibration.
pub fn combination_novelty(
    candidates: &[Candidate],
    reach_a: f32,
    reach_b: f32,
    predict: &dyn Fn(f32) -> f64,
) -> Novelty {
    let both = candidates
        .iter()
        .filter(|c| c.to_a >= reach_a && c.to_b >= reach_b)
        .max_by(|x, y| x.similarity.total_cmp(&y.similarity));
    let nearest = candidates
        .iter()
        .max_by(|x, y| x.similarity.total_cmp(&y.similarity));
    let p = both.map_or(predict(-1.0), |c| predict(c.similarity));
    let verdict = Verdict::from_p(p);
    Novelty {
        passed: verdict != Verdict::Exists,
        verdict: verdict.as_str().into(),
        p_exists: p,
        nearest_title: both.map(|c| c.title.clone()),
        nearest_similarity: both.map(|c| c.similarity),
        rule: "combination".into(),
        one_sided_nearest: nearest
            .filter(|n| both.is_none_or(|b| !std::ptr::eq(*n, b)))
            .map(|n| (n.title.clone(), n.similarity)),
    }
}

/// The 25th-percentile centroid similarity of a region's core members.
fn reach(members: &[invectus_core::store::MemberRecord], region: i32) -> f32 {
    let mut v: Vec<f32> = members
        .iter()
        .filter(|m| m.fine_id == region && !m.noise)
        .map(|m| m.centroid_sim)
        .collect();
    v.sort_by(f32::total_cmp);
    v.get(v.len() / 4).copied().unwrap_or(f32::INFINITY)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Attempt {
    pub title: String,
    pub description: String,
    pub on_target: OnTarget,
    /// `None` when there is no calibration to check against.
    pub novelty: Option<Novelty>,
    pub seconds: f64,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct DescribeResult {
    pub target: Target,
    pub label: String,
    /// The first attempt that is on target and novel (or on target when novelty is unknown).
    pub accepted: Option<usize>,
    pub attempts: Vec<Attempt>,
    pub context_titles: Vec<String>,
    pub llm: LlmIdentity,
    pub embedding_model: String,
    pub notes: Vec<String>,
}

fn dot(a: &[f32], b: &[f32]) -> f32 {
    a.iter().zip(b).map(|(x, y)| x * y).sum()
}

fn normalize(v: &mut [f32]) {
    let n = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if n > 0.0 {
        v.iter_mut().for_each(|x| *x /= n);
    }
}

/// Spherical interpolation between unit vectors.
pub fn slerp(a: &[f32], b: &[f32], t: f32) -> Vec<f32> {
    let cos = dot(a, b).clamp(-1.0, 1.0);
    let omega = cos.acos();
    let mut v: Vec<f32> = if omega.abs() < 1e-5 {
        a.iter().zip(b).map(|(x, y)| x + t * (y - x)).collect()
    } else {
        let s = omega.sin();
        let (wa, wb) = (((1.0 - t) * omega).sin() / s, (t * omega).sin() / s);
        a.iter().zip(b).map(|(x, y)| wa * x + wb * y).collect()
    };
    normalize(&mut v);
    v
}

/// Split a reply into a title and a description.
pub fn parse_reply(reply: &str) -> (String, String) {
    let clean = |s: &str| {
        s.trim()
            .trim_matches(|c: char| matches!(c, '*' | '"' | '#'))
            .trim()
            .to_string()
    };
    let mut title = String::new();
    let mut body = Vec::new();
    for line in reply.lines() {
        let l = line.trim();
        if l.is_empty() {
            continue;
        }
        let lower = l.trim_start_matches(['*', '#', ' ']).to_lowercase();
        if title.is_empty() && lower.starts_with("title:") {
            title = clean(&l.trim_start_matches(['*', '#', ' '])["title:".len()..]);
        } else if lower.starts_with("description:") {
            body.push(clean(
                &l.trim_start_matches(['*', '#', ' '])["description:".len()..],
            ));
        } else if title.is_empty() {
            title = clean(l);
        } else {
            body.push(l.to_string());
        }
    }
    (title, body.join(" ").trim().to_string())
}

/// Titles (with role) of the `k` items nearest `v`.
fn nearest_titles(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    v: &[f32],
    k: usize,
    role: Option<Role>,
) -> Result<Vec<String>> {
    let allowed = role
        .map(|r| project.store.item_ids_with_role(&table.collection_id, r))
        .transpose()?;
    let (scored, _) = vectors.exhaustive(table, v, k * 4, allowed.as_ref())?;
    let ids: Vec<i64> = collapse(&scored, k).into_iter().map(|x| x.0).collect();
    let items = project.store.items_by_ids(&ids)?;
    Ok(ids
        .iter()
        .filter_map(|id| items.get(id))
        .map(|i| {
            format!(
                "- [{}] {}{}",
                i.role.as_str(),
                i.title
                    .as_deref()
                    .unwrap_or(&i.source_id)
                    .chars()
                    .take(200)
                    .collect::<String>(),
                i.date
                    .as_deref()
                    .and_then(|d| d.get(..4))
                    .map(|y| format!(" ({y})"))
                    .unwrap_or_default()
            )
        })
        .collect())
}

fn name(r: &RegionRecord) -> String {
    r.llm_label.clone().unwrap_or_else(|| r.label.clone())
}

#[allow(clippy::too_many_arguments)]
pub fn describe(
    project: &Project,
    vectors: &VectorStore,
    table: &VectorTableRecord,
    encoder: &mut dyn TextEmbedder,
    generator: &mut dyn TextGenerator,
    target: &Target,
    opts: &DescribeOptions,
) -> Result<DescribeResult> {
    if encoder.identity().space_id() != table.space_id {
        invectus_index::search::check_compatible(project, table, encoder)?;
    }
    let run = project
        .store
        .latest_region_run(&table.collection_id, &table.space_id)?
        .context("no regions yet; run `invectus regions run` first")?;
    let regions: Vec<RegionRecord> = project
        .store
        .regions(&run.run_id)?
        .into_iter()
        .filter(|r| r.level == "fine")
        .collect();
    let region = |id: i32| {
        regions
            .iter()
            .find(|r| r.region_id == id)
            .with_context(|| format!("no fine region {id} in the latest regions"))
    };
    let mut notes = Vec::new();
    let has_calibration = project
        .store
        .latest_calibration(&table.collection_id, &table.space_id)?
        .is_some();
    let members = project.store.region_members(&run.run_id)?;
    let curve: Option<invectus_unsearch::Curve> = if has_calibration {
        let cal = invectus_unsearch::verdict::calibration_for(project, table)?;
        Some(serde_json::from_value(cal.curve)?)
    } else {
        None
    };
    if !has_calibration {
        notes
            .push("no calibration: novelty was not checked (run `invectus calibrate`)".to_string());
    }

    // The point to describe, the prompt, and how to judge a description's position.
    let (point, label, prompt, ends) = match *target {
        Target::Between { a, b, t } => {
            if !(0.0..=1.0).contains(&t) {
                bail!("t must be between 0 and 1");
            }
            let (ra, rb) = (region(a)?, region(b)?);
            let m = slerp(&ra.centroid, &rb.centroid, t);
            let near_m = nearest_titles(project, vectors, table, &m, opts.context, None)?;
            let near_a = nearest_titles(project, vectors, table, &ra.centroid, opts.context, None)?;
            let near_b = nearest_titles(project, vectors, table, &rb.centroid, opts.context, None)?;
            let lean = match t {
                t if t < 0.4 => format!("closer to side A ({:.0}% of the way to B)", t * 100.0),
                t if t > 0.6 => format!("closer to side B ({:.0}% of the way to B)", t * 100.0),
                _ => "halfway between them".to_string(),
            };
            let prompt = format!(
                "Side A, \"{}\", includes:\n{}\n\nSide B, \"{}\", includes:\n{}\n\nThe items \
                 nearest the space between them are:\n{}\n\nDescribe one new {} that would sit \
                 {lean}: it should combine the two sides in a way none of the listed items does. \
                 Reply exactly as:\nTitle: <one line>\nDescription: <3 to 5 sentences>",
                name(ra),
                near_a.join("\n"),
                name(rb),
                near_b.join("\n"),
                near_m.join("\n"),
                opts.genre
            );
            let mut context = near_a;
            context.extend(near_b);
            context.extend(near_m);
            (
                m,
                format!("{} ↔ {}", name(ra), name(rb)),
                (prompt, context),
                Some((ra.centroid.clone(), rb.centroid.clone())),
            )
        }
        Target::Region { region: id } => {
            let r = region(id)?;
            let demand = nearest_titles(
                project,
                vectors,
                table,
                &r.centroid,
                opts.context,
                Some(Role::Demand),
            )?;
            let coverage = nearest_titles(
                project,
                vectors,
                table,
                &r.centroid,
                3,
                Some(Role::Coverage),
            )?;
            let prompt = format!(
                "These items in \"{}\" ask for or need an answer:\n{}\n\nThe nearest existing \
                 coverage items are:\n{}\n\nDescribe the one new {} that would cover this area, \
                 which the existing coverage items do not. Reply exactly as:\nTitle: <one \
                 line>\nDescription: <3 to 5 sentences>",
                name(r),
                demand.join("\n"),
                if coverage.is_empty() {
                    "(none)".to_string()
                } else {
                    coverage.join("\n")
                },
                opts.coverage_genre
            );
            let mut context = demand;
            context.extend(coverage);
            (r.centroid.clone(), name(r), (prompt, context), None)
        }
    };
    let (prompt, context_titles) = prompt;

    let mut attempts = Vec::new();
    let mut accepted = None;
    for i in 0..opts.tries.max(1) {
        let started = Instant::now();
        let reply = generator.generate(
            Some("You write concise, concrete, plausible descriptions. No preamble."),
            &prompt,
            &GenOptions {
                max_tokens: 320,
                temperature: if i == 0 { 0.0 } else { 0.8 },
                seed: i as u32,
            },
        )?;
        let (title, description) = parse_reply(&reply.text);
        if title.is_empty() && description.is_empty() {
            continue;
        }
        let doc = encoder
            .identity()
            .format_document(Some(&title), &description);
        let v = encoder.embed(&[doc], InputKind::Document)?.remove(0).vector;
        let (nearest_region, nearest_sim) = regions
            .iter()
            .map(|r| (r.region_id, dot(&v, &r.centroid)))
            .max_by(|x, y| x.1.total_cmp(&y.1))
            .unzip();
        let to_target = dot(&v, &point);
        let on_target = match (&ends, target) {
            (Some((a, b)), _) => {
                let (sa, sb) = (dot(&v, a), dot(&v, b));
                OnTarget {
                    passed: to_target > sa && to_target > sb,
                    to_target,
                    to_a: Some(sa),
                    to_b: Some(sb),
                    nearest_region,
                    nearest_region_similarity: nearest_sim,
                }
            }
            (None, Target::Region { region }) => OnTarget {
                passed: nearest_region == Some(*region),
                to_target,
                to_a: None,
                to_b: None,
                nearest_region,
                nearest_region_similarity: nearest_sim,
            },
            (None, _) => unreachable!("between targets always have ends"),
        };
        let novelty = if has_calibration {
            let c = check(
                project,
                vectors,
                table,
                encoder,
                &format!("{title}. {description}"),
                &CheckOptions {
                    k: 20,
                    role: match target {
                        Target::Region { .. } => Some(Role::Coverage),
                        Target::Between { .. } => None,
                    },
                    ..Default::default()
                },
            )?;
            match (&ends, target) {
                (Some((ca, cb)), Target::Between { a, b, .. }) => {
                    let ids: Vec<i64> = c.search.hits.iter().map(|h| h.item.item_id).collect();
                    let stored = vectors.vectors_for(table, &ids)?;
                    let candidates: Vec<Candidate> = c
                        .search
                        .hits
                        .iter()
                        .filter_map(|h| {
                            let v = stored.get(&h.item.item_id)?;
                            Some(Candidate {
                                title: h.item.title.clone().unwrap_or_default(),
                                similarity: h.similarity,
                                to_a: dot(v, ca),
                                to_b: dot(v, cb),
                            })
                        })
                        .collect();
                    let curve = &curve;
                    Some(combination_novelty(
                        &candidates,
                        reach(&members, *a),
                        reach(&members, *b),
                        &|s| curve.as_ref().map_or(0.0, |k| k.predict(s)),
                    ))
                }
                _ => Some(Novelty {
                    passed: c.verdict != Verdict::Exists,
                    verdict: c.verdict.as_str().into(),
                    p_exists: c.p_exists,
                    nearest_title: c.search.hits.first().and_then(|h| h.item.title.clone()),
                    nearest_similarity: c.nearest_similarity,
                    rule: "single".into(),
                    one_sided_nearest: None,
                }),
            }
        } else {
            None
        };
        let ok = on_target.passed && novelty.as_ref().is_none_or(|n| n.passed);
        attempts.push(Attempt {
            title,
            description,
            on_target,
            novelty,
            seconds: started.elapsed().as_secs_f64(),
        });
        if ok {
            accepted = Some(attempts.len() - 1);
            break;
        }
    }
    if accepted.is_none() {
        notes.push(format!(
            "no description passed the checks in {} attempt(s); the last ones are kept for review",
            attempts.len()
        ));
    }
    let result = DescribeResult {
        target: target.clone(),
        label,
        accepted,
        attempts,
        context_titles,
        llm: generator.identity().clone(),
        embedding_model: encoder.identity().model_key.clone(),
        notes,
    };
    project.store.save_midpoint(
        &table.collection_id,
        &table.space_id,
        &serde_json::to_value(target)?,
        &serde_json::to_value(&result)?,
    )?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combinations_need_an_item_on_both_sides() {
        let c = |title: &str, similarity: f32, to_a: f32, to_b: f32| Candidate {
            title: title.into(),
            similarity,
            to_a,
            to_b,
        };
        // P(exists) rises with similarity: 0.9 → 0.95, 0.7 → 0.1.
        let predict = |s: f32| {
            if s >= 0.85 {
                0.95
            } else if s >= 0.75 {
                0.5
            } else {
                0.1
            }
        };
        // A strong paper on side A alone is not the combination.
        let n = combination_novelty(
            &[c("A only", 0.9, 0.9, 0.5), c("Both", 0.7, 0.8, 0.8)],
            0.75,
            0.75,
            &predict,
        );
        assert!(n.passed && n.verdict == "open", "{n:?}");
        assert_eq!(n.nearest_title.as_deref(), Some("Both"));
        assert_eq!(n.one_sided_nearest, Some(("A only".into(), 0.9)));
        // A close paper that reaches both sides is.
        let n = combination_novelty(&[c("Both", 0.9, 0.8, 0.8)], 0.75, 0.75, &predict);
        assert!(!n.passed && n.verdict == "exists" && n.one_sided_nearest.is_none());
        // Nothing reaches both: the lowest P.
        let n = combination_novelty(&[c("A only", 0.95, 0.9, 0.1)], 0.75, 0.75, &predict);
        assert_eq!((n.p_exists, n.nearest_title), (0.1, None));
    }

    #[test]
    fn slerp_stays_on_the_sphere_between_the_ends() {
        let a = [1.0, 0.0];
        let b = [0.0, 1.0];
        let m = slerp(&a, &b, 0.5);
        let s = std::f32::consts::FRAC_1_SQRT_2;
        assert!((m[0] - s).abs() < 1e-6 && (m[1] - s).abs() < 1e-6);
        assert!((slerp(&a, &b, 0.0)[0] - 1.0).abs() < 1e-6);
        assert!((slerp(&a, &b, 1.0)[1] - 1.0).abs() < 1e-6);
        assert!((slerp(&a, &a, 0.3)[0] - 1.0).abs() < 1e-6, "identical ends");
    }

    #[test]
    fn replies_split_into_title_and_description() {
        let (t, d) = parse_reply(
            "**Title:** Crosslinking in thin corneas\n\nDescription: A trial of... It tests...",
        );
        assert_eq!(t, "Crosslinking in thin corneas");
        assert_eq!(d, "A trial of... It tests...");
        let (t, d) = parse_reply("A plain first line\nThen the body.\nMore body.");
        assert_eq!(
            (t.as_str(), d.as_str()),
            ("A plain first line", "Then the body. More body.")
        );
        assert_eq!(parse_reply(""), (String::new(), String::new()));
    }
}
