//! What files an AI agent may read (audit 2026-11-12, item 5).
//!
//! Trust boundary: an agent (Claude Code, Codex, a cloud model behind them) calls inVectus tools
//! over MCP, and what a tool returns can reach the agent's provider. So agent tools read files
//! only inside places the person granted:
//! - the projects folder (`~/inVectus Projects`, where agent-created projects go);
//! - the open project's own folder (its pulled snapshots);
//! - folders the person granted with `invectus agents allow <folder>` (kept per machine in
//!   `<OS data dir>/inVectus/agent-access.json`, never in a project, so a copied project cannot
//!   grant itself anything).
//!
//! Every path is resolved first (`..`, `~`, and symlinks, including the final file), then
//! compared with the resolved grants, so a link inside a granted folder that points outside
//! it is refused. Remote sources (links and `notion:` style specs) are not files and are not
//! affected. The person's own imports in the app (chosen in a file dialog) are not limited.

use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The per-machine grants file.
pub fn grants_file() -> Result<PathBuf> {
    let base = invectus_core::paths::app_data_dir().context("no OS data folder")?;
    Ok(base.join("agent-access.json"))
}

/// Folders the person granted to agents on this machine.
pub fn granted_folders() -> Vec<PathBuf> {
    grants_file()
        .ok()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice::<serde_json::Value>(&b).ok())
        .and_then(|v| {
            v["folders"].as_array().map(|a| {
                a.iter()
                    .filter_map(|p| p.as_str().map(PathBuf::from))
                    .collect()
            })
        })
        .unwrap_or_default()
}

/// Read-modify-write of the grants file is serialised within the process.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn save(folders: &[PathBuf]) -> Result<()> {
    let file = grants_file()?;
    let dir = file.parent().context("grants file has no folder")?;
    invectus_core::private::create_dir_all(dir)?;
    let body = serde_json::to_vec_pretty(&serde_json::json!({
        "about": "Folders AI agents may read through inVectus. Change with `invectus agents allow/revoke`.",
        "folders": folders.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    }))?;
    invectus_core::private::write(&file, &body)
}

/// Grant agents read access to `folder` (resolved; must exist and be a folder).
pub fn allow(folder: &Path) -> Result<PathBuf> {
    let real = folder
        .canonicalize()
        .with_context(|| format!("no folder at {}", folder.display()))?;
    if !real.is_dir() {
        bail!("{} is not a folder", real.display());
    }
    if real.parent().is_none() || dirs::home_dir().is_some_and(|h| h == real) {
        bail!(
            "granting the whole disk or home folder is refused; choose the folder that holds the data"
        );
    }
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut folders = granted_folders();
    if !folders.contains(&real) {
        folders.push(real.clone());
    }
    save(&folders)?;
    Ok(real)
}

/// Remove a grant; `false` when it was not granted.
pub fn revoke(folder: &Path) -> Result<bool> {
    let real = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());
    let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let mut folders = granted_folders();
    let before = folders.len();
    folders.retain(|f| f != &real && f != folder);
    save(&folders)?;
    Ok(folders.len() != before)
}

/// `path` resolved, if it lies inside one of `roots` (each resolved too). Refused otherwise,
/// including when the path does not exist (nothing to read) or escapes through a symlink.
pub fn check_within(path: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let real = path
        .canonicalize()
        .with_context(|| format!("no file or folder at {}", path.display()))?;
    let inside = roots
        .iter()
        .filter_map(|r| r.canonicalize().ok())
        .any(|r| real.starts_with(&r));
    if !inside {
        bail!(
            "agents may not read {}: it is outside the folders granted to agents. The person can \
             grant its folder with `invectus agents allow <folder>`, or import it in the app",
            path.display()
        );
    }
    Ok(real)
}

/// Where a new or opened project may be: inside the projects folder or a granted folder. The
/// project itself may not exist yet, so its nearest existing ancestor is resolved.
pub fn check_project_location(path: &Path, roots: &[PathBuf]) -> Result<()> {
    if path
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        bail!("agents may not use `..` in a project path");
    }
    let mut existing = path;
    while !existing.exists() {
        existing = existing
            .parent()
            .context("no existing folder above this path")?;
    }
    check_within(existing, roots).map(|_| ())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn paths_outside_grants_and_symlink_escapes_are_refused() {
        let dir = tempfile::tempdir().unwrap();
        let granted = dir.path().join("granted");
        let outside = dir.path().join("outside");
        std::fs::create_dir_all(&granted).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        std::fs::write(granted.join("ok.csv"), "a\n1\n").unwrap();
        std::fs::write(outside.join("secret.csv"), "a\n1\n").unwrap();
        std::os::unix::fs::symlink(outside.join("secret.csv"), granted.join("link.csv")).unwrap();
        std::os::unix::fs::symlink(&outside, granted.join("linkdir")).unwrap();
        let roots = vec![granted.clone()];

        assert!(check_within(&granted.join("ok.csv"), &roots).is_ok());
        for bad in [
            outside.join("secret.csv"),
            granted.join("link.csv"),
            granted.join("linkdir"),
            granted.join("linkdir/secret.csv"),
            granted.join("../outside/secret.csv"),
            PathBuf::from("/etc/hosts"),
        ] {
            let e = check_within(&bad, &roots).unwrap_err().to_string();
            assert!(
                e.contains("outside the folders granted") || e.contains("no file"),
                "{bad:?}: {e}"
            );
        }
        // A granted folder that is itself a symlink resolves to its target.
        let alias = dir.path().join("alias");
        std::os::unix::fs::symlink(&granted, &alias).unwrap();
        assert!(check_within(&granted.join("ok.csv"), &[alias]).is_ok());
    }

    #[test]
    fn projects_stay_inside_the_projects_folder() {
        let dir = tempfile::tempdir().unwrap();
        let projects = dir.path().join("projects");
        std::fs::create_dir_all(&projects).unwrap();
        let roots = vec![projects.clone()];
        assert!(check_project_location(&projects.join("new.invectus"), &roots).is_ok());
        assert!(check_project_location(&projects.join("a/b/new.invectus"), &roots).is_ok());
        assert!(check_project_location(&dir.path().join("elsewhere.invectus"), &roots).is_err());
        assert!(check_project_location(&projects.join("../x.invectus"), &roots).is_err());
    }
}
