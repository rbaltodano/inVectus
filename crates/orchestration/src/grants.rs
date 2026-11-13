//! What files an AI agent may read (audit 2026-11-12, item 5).
//!
//! Trust boundary: an agent (Claude Code, Codex, a cloud model behind them) calls inVectus tools
//! over MCP, and what a tool returns can reach the agent's provider. So agent tools read files
//! only inside places the person granted:
//! - the projects folder (`~/inVectus Projects`, where agent-created projects go);
//! - the open project's own folder (its pulled snapshots);
//! - files and folders the person granted (Settings → Storage → Agent folders, or
//!   `invectus agents allow <path>`; kept per machine in
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

/// Files and folders the person granted to agents on this machine.
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
        "about": "Files and folders AI agents may read through inVectus. Change in Settings → Storage or with `invectus agents allow/revoke`.",
        "folders": folders.iter().map(|p| p.display().to_string()).collect::<Vec<_>>(),
    }))?;
    invectus_core::private::write(&file, &body)
}

/// Refuse places too broad to hand to an agent: the whole disk, the home folder, or any folder
/// that contains the home folder (`/Users`, a volume root). `real` must already be resolved.
pub fn check_grantable(real: &Path) -> Result<()> {
    let holds_home = dirs::home_dir()
        .and_then(|h| h.canonicalize().ok())
        .is_some_and(|h| h.starts_with(real));
    if real.parent().is_none() || holds_home {
        bail!(
            "{} holds your whole disk or home folder; choose the folder that holds the data",
            real.display()
        );
    }
    Ok(())
}

/// Grant agents read access to `path`, a file or a folder (resolved; must exist).
pub fn allow(path: &Path) -> Result<PathBuf> {
    let real = path
        .canonicalize()
        .with_context(|| format!("nothing at {}", path.display()))?;
    if !real.is_dir() && !real.is_file() {
        bail!("{} is not a file or folder", real.display());
    }
    check_grantable(&real)?;
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

// ---- Folder tree for the picker ------------------------------------------------------------

/// One entry in [`folder_tree`]: a folder (with what it holds, up to the limits) or a file.
#[derive(Debug, serde::Serialize)]
pub struct TreeNode {
    pub name: String,
    pub path: String,
    pub dir: bool,
    /// Visible entries inside a folder (all of them, listed or not).
    pub count: usize,
    pub children: Vec<TreeNode>,
    /// Entries inside that are not listed (over a limit or below the depth).
    pub more: usize,
}

/// At most this many entries per folder, and this many in a whole tree.
const PER_FOLDER: usize = 60;
const PER_TREE: usize = 600;

/// `root` and what it holds, `depth` levels down, for the agent-folder picker. Hidden entries
/// and symlinks are left out (a link is never followed), folders come before files, and the
/// listing stops at the limits above. Names and paths only; no file is opened.
pub fn folder_tree(root: &Path, depth: usize) -> Result<TreeNode> {
    let real = root
        .canonicalize()
        .with_context(|| format!("no folder at {}", root.display()))?;
    if !real.is_dir() {
        bail!("{} is not a folder", real.display());
    }
    let mut budget = PER_TREE;
    Ok(node(&real, true, depth, &mut budget))
}

fn node(path: &Path, dir: bool, depth: usize, budget: &mut usize) -> TreeNode {
    let name = path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    );
    let mut out = TreeNode {
        name,
        path: path.display().to_string(),
        dir,
        count: 0,
        children: Vec::new(),
        more: 0,
    };
    if !dir {
        return out;
    }
    let mut entries: Vec<(bool, String, PathBuf)> = std::fs::read_dir(path)
        .map(|rd| {
            rd.filter_map(|e| e.ok())
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().into_owned();
                    let kind = e.file_type().ok()?;
                    if name.starts_with('.') || kind.is_symlink() {
                        return None;
                    }
                    Some((kind.is_dir(), name, e.path()))
                })
                .collect()
        })
        .unwrap_or_default();
    entries.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase()))
    });
    out.count = entries.len();
    if depth == 0 {
        out.more = entries.len();
        return out;
    }
    for (is_dir, _, p) in entries.iter().take(PER_FOLDER) {
        if *budget == 0 {
            break;
        }
        *budget -= 1;
        out.children.push(node(p, *is_dir, depth - 1, budget));
    }
    out.more = out.count - out.children.len();
    out
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
    fn broad_places_are_refused_and_files_can_be_granted() {
        assert!(check_grantable(Path::new("/")).is_err());
        let home = dirs::home_dir().unwrap().canonicalize().unwrap();
        assert!(check_grantable(&home).is_err());
        assert!(check_grantable(home.parent().unwrap()).is_err());
        let dir = tempfile::tempdir().unwrap();
        assert!(check_grantable(&dir.path().canonicalize().unwrap()).is_ok());
        // A granted file is readable itself, and nothing beside it is.
        let f = dir.path().join("one.csv");
        std::fs::write(&f, "a\n1\n").unwrap();
        std::fs::write(dir.path().join("other.csv"), "a\n1\n").unwrap();
        let roots = vec![f.canonicalize().unwrap()];
        assert!(check_within(&f, &roots).is_ok());
        assert!(check_within(&dir.path().join("other.csv"), &roots).is_err());
    }

    #[test]
    fn the_tree_skips_hidden_entries_and_links_and_lists_folders_first() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("b-folder/inner")).unwrap();
        std::fs::write(root.join("a.csv"), "x").unwrap();
        std::fs::write(root.join(".secret"), "x").unwrap();
        std::fs::write(root.join("b-folder/inner/deep.txt"), "x").unwrap();
        std::os::unix::fs::symlink("/etc", root.join("link")).unwrap();
        let t = folder_tree(root, 2).unwrap();
        let names: Vec<_> = t.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["b-folder", "a.csv"]);
        assert_eq!(t.count, 2);
        let inner = &t.children[0].children[0];
        assert_eq!(inner.name, "inner");
        assert!(inner.children.is_empty() && inner.more == 1 && inner.count == 1);
        assert!(folder_tree(&root.join("a.csv"), 1).is_err());
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
