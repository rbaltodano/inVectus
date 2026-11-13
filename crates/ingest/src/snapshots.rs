//! Pulled copies kept in a project (`<project>/sources/<kind>/<id>.csv` plus a `.json` record),
//! what uses them, and removing them safely (audit 2026-11-12, item 6).
//!
//! Retention: a pulled copy stays in the project until the person removes it or deletes the
//! project. It is not refreshed or expired automatically. Several collections can be imported
//! from one copy, so a copy is removed only when no collection still names it.

use anyhow::{Context, Result, bail};
use invectus_core::Project;
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Clone, Debug, Serialize)]
pub struct SnapshotInfo {
    /// `<kind>/<file>`, e.g. `notion/0123….csv`: the name to pass to [`forget`].
    pub name: String,
    pub path: PathBuf,
    /// What was pulled, from the `.json` record (e.g. "Notion database 0123…").
    pub origin: Option<String>,
    pub rows: Option<u64>,
    pub bytes: u64,
    pub pulled_at: Option<u64>,
    /// Collections imported from this copy.
    pub used_by: Vec<String>,
}

/// `<kind>/<file>` of a snapshot path, when it is a `.csv` directly in `sources/<kind>/`.
fn short_name(path: &Path) -> Option<String> {
    let file = path.file_name()?.to_str()?;
    let kind = path.parent()?.file_name()?.to_str()?;
    file.ends_with(".csv").then(|| format!("{kind}/{file}"))
}

/// Every pulled copy in `project`, with the collections that use it.
pub fn list(project: &Project) -> Result<Vec<SnapshotInfo>> {
    let mut users: Vec<(String, String)> = Vec::new(); // (snapshot short name, collection)
    for id in project.store.collection_ids()? {
        if let Some(c) = project.store.collection(&id)?
            && let Some(snap) = c.source["snapshot"].as_str()
            && let Some(name) = short_name(Path::new(snap))
        {
            users.push((name, id));
        }
    }
    let mut out = Vec::new();
    let sources = project.root.join("sources");
    let Ok(kinds) = std::fs::read_dir(&sources) else {
        return Ok(out);
    };
    for kind in kinds.flatten() {
        if !kind.file_type()?.is_dir() {
            continue;
        }
        for f in std::fs::read_dir(kind.path())?.flatten() {
            let path = f.path();
            let Some(name) = short_name(&path) else {
                continue;
            };
            if !f.file_type()?.is_file() {
                continue;
            }
            let record: Option<serde_json::Value> = std::fs::read(path.with_extension("json"))
                .ok()
                .and_then(|b| serde_json::from_slice(&b).ok());
            let pull = record.as_ref().map(|r| &r["pull"]);
            out.push(SnapshotInfo {
                used_by: users
                    .iter()
                    .filter(|(n, _)| *n == name)
                    .map(|(_, c)| c.clone())
                    .collect(),
                origin: pull.and_then(|p| p["origin"].as_str().map(String::from)),
                rows: pull.and_then(|p| p["rows"].as_u64()),
                pulled_at: pull.and_then(|p| p["pulled_at"].as_u64()),
                bytes: f.metadata()?.len(),
                name,
                path,
            });
        }
    }
    out.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(out)
}

/// Remove a pulled copy (`<kind>/<file>` from [`list`]) and its record. Refused while a
/// collection still uses it: the collection could no longer be updated from it.
pub fn forget(project: &Project, name: &str) -> Result<()> {
    let found = list(project)?
        .into_iter()
        .find(|s| s.name == name)
        .with_context(|| format!("no pulled copy named {name} in this project"))?;
    if !found.used_by.is_empty() {
        bail!(
            "{name} is used by {}: remove those collections first, or keep the copy",
            found.used_by.join(", ")
        );
    }
    // `list` only returns plain `.csv` files directly inside `sources/<kind>/`.
    std::fs::remove_file(&found.path)
        .with_context(|| format!("removing {}", found.path.display()))?;
    for extra in [
        found.path.with_extension("json"),
        found.path.with_extension("csv.part"),
    ] {
        let _ = std::fs::remove_file(extra);
    }
    if let Some(dir) = found.path.parent() {
        let _ = std::fs::remove_dir(dir); // only when now empty
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use invectus_core::store::CollectionRecord;

    #[test]
    fn a_shared_copy_survives_until_no_collection_uses_it() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::create(&dir.path().join("p")).unwrap();
        let snap = p.root.join("sources/notion/abc.csv");
        invectus_core::private::write(&snap, b"a\n1\n").unwrap();
        invectus_core::private::write(
            &snap.with_extension("json"),
            br#"{"pull":{"origin":"Notion database abc","rows":1,"pulled_at":5}}"#,
        )
        .unwrap();
        for c in ["one", "two"] {
            p.store
                .upsert_collection(&CollectionRecord {
                    collection_id: c.into(),
                    source: serde_json::json!({"kind": "notion", "database_id": "abc", "snapshot": snap}),
                    mapping: serde_json::json!({}),
                    chunking: serde_json::json!({}),
                })
                .unwrap();
        }
        let l = list(&p).unwrap();
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].name, "notion/abc.csv");
        assert_eq!(l[0].used_by, ["one", "two"]);
        assert_eq!(l[0].rows, Some(1));
        let e = forget(&p, "notion/abc.csv").unwrap_err().to_string();
        assert!(e.contains("one, two"), "{e}");
        assert!(snap.exists());
        // Names that are not listed copies are refused, including attempts to leave the folder.
        assert!(forget(&p, "../project.json").is_err());
        assert!(forget(&p, "notion/../../project.json").is_err());
        assert!(p.root.join("project.json").exists());
    }

    #[test]
    fn an_unused_copy_and_its_record_are_removed() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::create(&dir.path().join("p")).unwrap();
        let snap = p.root.join("sources/tally/f1.csv");
        invectus_core::private::write(&snap, b"a\n1\n").unwrap();
        invectus_core::private::write(&snap.with_extension("json"), b"{}").unwrap();
        forget(&p, "tally/f1.csv").unwrap();
        assert!(!snap.exists() && !snap.with_extension("json").exists());
        assert!(
            !p.root.join("sources/tally").exists(),
            "the empty folder goes too"
        );
    }
}
