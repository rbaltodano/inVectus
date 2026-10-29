//! Project folder layout (SPEC §7.4) and `project.json`.

use crate::store::Store;
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub const PROJECT_FORMAT: u32 = 1;
pub const PROJECT_SUFFIX: &str = "invectus";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectConfig {
    pub format_version: u32,
    pub name: String,
    /// Connectors and cloud LLM calls are refused unless this is true (SPEC §7.7). P1 has no
    /// connectors; the flag is persisted so later milestones inherit the default.
    pub network_enabled: bool,
}

pub struct Project {
    pub root: PathBuf,
    pub config: ProjectConfig,
    pub store: Store,
}

impl Project {
    /// Create `<path>` (adding the `.invectus` suffix if missing) with the standard layout.
    pub fn create(path: &Path) -> Result<Self> {
        let root = with_suffix(path);
        if root.join("project.json").exists() {
            bail!("{} already contains a project", root.display());
        }
        for dir in ["vectors", "cache", "exports"] {
            std::fs::create_dir_all(root.join(dir))
                .with_context(|| format!("creating {}", root.join(dir).display()))?;
        }
        let name = root
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "project".into());
        let config = ProjectConfig {
            format_version: PROJECT_FORMAT,
            name,
            network_enabled: false,
        };
        write_json_atomic(&root.join("project.json"), &config)?;
        let store = Store::open(&root.join("project.duckdb"))?;
        Ok(Self {
            root,
            config,
            store,
        })
    }

    pub fn open(path: &Path) -> Result<Self> {
        let root = if path.join("project.json").exists() {
            path.to_path_buf()
        } else {
            with_suffix(path)
        };
        let cfg_path = root.join("project.json");
        let raw = std::fs::read(&cfg_path)
            .with_context(|| format!("no inVectus project at {}", root.display()))?;
        let config: ProjectConfig = serde_json::from_slice(&raw)
            .with_context(|| format!("reading {}", cfg_path.display()))?;
        if config.format_version != PROJECT_FORMAT {
            bail!(
                "project format {} is not supported (expected {PROJECT_FORMAT})",
                config.format_version
            );
        }
        let store = Store::open(&root.join("project.duckdb"))?;
        Ok(Self {
            root,
            config,
            store,
        })
    }

    /// Turn the project's network switch on or off and save `project.json`.
    pub fn set_network_enabled(&mut self, enabled: bool) -> Result<()> {
        self.config.network_enabled = enabled;
        write_json_atomic(&self.root.join("project.json"), &self.config)
    }

    pub fn vectors_dir(&self) -> PathBuf {
        self.root.join("vectors")
    }

    /// Warning text when the project sits in a cloud-synced folder (offloaded files break the
    /// databases; SPEC §7.4).
    pub fn storage_warning(&self) -> Option<String> {
        cloud_sync_warning(&self.root)
    }
}

fn with_suffix(path: &Path) -> PathBuf {
    if path.extension().is_some_and(|e| e == PROJECT_SUFFIX) {
        path.to_path_buf()
    } else {
        let mut s = path.as_os_str().to_owned();
        s.push(".");
        s.push(PROJECT_SUFFIX);
        PathBuf::from(s)
    }
}

pub fn cloud_sync_warning(path: &Path) -> Option<String> {
    let p = path.to_string_lossy();
    let markers = [
        ("Mobile Documents", "iCloud Drive"),
        ("Library/CloudStorage", "a cloud storage provider"),
        ("iCloud", "iCloud Drive"),
        ("OneDrive", "OneDrive"),
        ("Dropbox", "Dropbox"),
        ("Google Drive", "Google Drive"),
    ];
    // iCloud also syncs ~/Documents and ~/Desktop when "Desktop & Documents" is enabled.
    let home_synced = std::env::var_os("HOME").is_some_and(|h| {
        let h = PathBuf::from(h);
        cfg!(target_os = "macos")
            && (path.starts_with(h.join("Documents")) || path.starts_with(h.join("Desktop")))
    });
    markers
        .iter()
        .find(|(m, _)| p.contains(m))
        .map(|(_, svc)| *svc)
        .or(home_synced.then_some("iCloud Desktop & Documents (if enabled)"))
        .map(|svc| {
            format!(
                "{} may be synced by {svc}. Offloaded files can corrupt or stall project databases; \
                 prefer a local folder such as ~/inVectus Projects.",
                path.display()
            )
        })
}

pub fn write_json_atomic<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(value)?)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_then_open() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::create(&dir.path().join("demo")).unwrap();
        assert!(p.root.ends_with("demo.invectus"));
        assert!(!p.config.network_enabled, "network is off by default");
        for d in ["vectors", "cache", "exports"] {
            assert!(p.root.join(d).is_dir());
        }
        drop(p);
        let p = Project::open(&dir.path().join("demo")).unwrap();
        assert_eq!(p.config.name, "demo");
        assert!(Project::create(&dir.path().join("demo.invectus")).is_err());
    }

    #[test]
    fn warns_for_synced_folders() {
        assert!(cloud_sync_warning(Path::new("/x/Library/Mobile Documents/p")).is_some());
        assert!(cloud_sync_warning(Path::new("/x/Dropbox/p")).is_some());
        assert!(cloud_sync_warning(Path::new("/var/data/p")).is_none());
    }
}
