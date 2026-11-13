//! Per-machine app preferences kept in `<OS data dir>/inVectus/settings.json` (never in a
//! project, so a copied project changes nothing here).
//!
//! The projects folder is where new projects go, and it is always readable by agents
//! ([`crate::grants`]), so choosing one passes the same breadth check as an agent grant.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

fn file() -> Result<PathBuf> {
    Ok(invectus_core::paths::app_data_dir()
        .context("no OS data folder")?
        .join("settings.json"))
}

fn load() -> serde_json::Value {
    file()
        .ok()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_else(|| serde_json::json!({}))
}

static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `~/inVectus Projects`.
pub fn standard_projects_dir() -> PathBuf {
    dirs::home_dir()
        .map(|h| h.join("inVectus Projects"))
        .unwrap_or_default()
}

/// The chosen projects folder, or [`standard_projects_dir`].
pub fn projects_dir() -> PathBuf {
    load()["projects_dir"]
        .as_str()
        .filter(|s| !s.is_empty())
        .map_or_else(standard_projects_dir, PathBuf::from)
}

/// Choose the projects folder (`None` goes back to the standard one). The folder must exist;
/// it is resolved and refused when it holds the whole disk or home folder.
pub fn set_projects_dir(folder: Option<&Path>) -> Result<PathBuf> {
    let chosen = match folder {
        Some(f) => {
            let real = f
                .canonicalize()
                .with_context(|| format!("no folder at {}", f.display()))?;
            if !real.is_dir() {
                bail!("{} is not a folder", real.display());
            }
            crate::grants::check_grantable(&real)?;
            Some(real)
        }
        None => None,
    };
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut v = load();
    match &chosen {
        Some(p) => v["projects_dir"] = serde_json::json!(p.display().to_string()),
        None => {
            if let Some(o) = v.as_object_mut() {
                o.remove("projects_dir");
            }
        }
    }
    let f = file()?;
    invectus_core::private::write(&f, &serde_json::to_vec_pretty(&v)?)?;
    Ok(chosen.unwrap_or_else(standard_projects_dir))
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn the_projects_folder_can_be_chosen_and_reset() {
        // The process-wide test data folder (shared with the other orchestration tests).
        invectus_core::paths::use_data_dir_for_tests(
            std::env::temp_dir().join(format!("invectus-orch-test-{}", std::process::id())),
        );
        let chosen = tempfile::tempdir().unwrap();
        let real = set_projects_dir(Some(chosen.path())).unwrap();
        assert_eq!(real, chosen.path().canonicalize().unwrap());
        assert_eq!(projects_dir(), real);
        assert!(set_projects_dir(Some(Path::new("/"))).is_err());
        assert!(set_projects_dir(Some(&dirs::home_dir().unwrap())).is_err());
        assert!(set_projects_dir(Some(&chosen.path().join("missing"))).is_err());
        assert_eq!(projects_dir(), real, "a refused choice changes nothing");
        assert_eq!(set_projects_dir(None).unwrap(), standard_projects_dir());
        assert_eq!(projects_dir(), standard_projects_dir());
    }
}
