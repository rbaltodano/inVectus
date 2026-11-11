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
    /// Requested model for future MCP agent work; the agent client controls execution.
    #[serde(default)]
    pub preferred_agent_model: Option<String>,
    /// Who does this project's writing work: an agent client id (`claude-code`, `codex`, …) or
    /// `on-device` for the local language model. `None`: not chosen yet.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    /// Outside sources gap checks use (`pubmed`, `openalex`, `semanticscholar`, `arxiv`);
    /// `None`: PubMed and OpenAlex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub verify_sources: Option<Vec<String>>,
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
            preferred_agent_model: None,
            agent: None,
            verify_sources: None,
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
        let config = read_config(&root)?;
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

    /// Choose the outside sources gap checks use (`None`: the default pair) and save it.
    pub fn set_verify_sources(&mut self, sources: Option<Vec<String>>) -> Result<()> {
        if sources.as_ref().is_some_and(|s| s.is_empty()) {
            bail!("choose at least one outside source");
        }
        self.config.verify_sources = sources;
        write_json_atomic(&self.root.join("project.json"), &self.config)
    }

    /// Save a request for future agent work without changing recorded authorship.
    pub fn set_preferred_agent_model(&mut self, model: Option<&str>) -> Result<()> {
        if let Some(model) = model
            && (model.trim().is_empty() || model.len() > 128 || model.chars().any(char::is_control))
        {
            bail!(
                "model name must be nonempty, at most 128 bytes, and contain no control characters"
            );
        }
        let mut config = read_config(&self.root)?;
        config.preferred_agent_model = model.map(|s| s.trim().to_owned());
        write_json_atomic(&self.root.join("project.json"), &config)?;
        self.config = config;
        Ok(())
    }

    /// Choose who does this project's writing work (an agent client id, or `on-device`).
    pub fn set_agent(&mut self, agent: Option<&str>) -> Result<()> {
        if let Some(agent) = agent
            && (agent.is_empty()
                || agent.len() > 64
                || !agent
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
        {
            bail!("agent must be a client id such as `claude-code`, or `on-device`");
        }
        let mut config = read_config(&self.root)?;
        config.agent = agent.map(str::to_owned);
        write_json_atomic(&self.root.join("project.json"), &config)?;
        self.config = config;
        Ok(())
    }

    /// Read current metadata, including preferences changed by the desktop app.
    pub fn preferred_agent_model(&self) -> Result<Option<String>> {
        Ok(read_config(&self.root)?.preferred_agent_model)
    }

    /// Who does this project's writing work, as last saved (an agent client id or `on-device`).
    pub fn agent(&self) -> Result<Option<String>> {
        Ok(read_config(&self.root)?.agent)
    }

    /// Change the display name without moving the project or touching its database.
    pub fn rename(&mut self, name: &str) -> Result<()> {
        let mut config = self.config.clone();
        config.name = valid_name(name)?;
        write_json_atomic(&self.root.join("project.json"), &config)?;
        self.config = config;
        Ok(())
    }

    /// Rename a closed project using only its metadata; no database connection is needed.
    pub fn rename_at(path: &Path, name: &str) -> Result<String> {
        let name = valid_name(name)?;
        let root = if path.join("project.json").exists() {
            path.to_path_buf()
        } else {
            with_suffix(path)
        };
        let mut config = read_config(&root)?;
        config.name = name.clone();
        write_json_atomic(&root.join("project.json"), &config)?;
        Ok(name)
    }

    /// The same project on a second database connection, for a background job.
    pub fn try_clone(&self) -> Result<Self> {
        Ok(Self {
            root: self.root.clone(),
            config: self.config.clone(),
            store: self.store.try_clone()?,
        })
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

fn valid_name(name: &str) -> Result<String> {
    let name = name.trim();
    if name.is_empty() || name.chars().any(char::is_control) {
        bail!("project name must be a non-empty single line");
    }
    Ok(name.to_owned())
}

fn read_config(root: &Path) -> Result<ProjectConfig> {
    let path = root.join("project.json");
    let raw = std::fs::read(&path)
        .with_context(|| format!("no inVectus project at {}", root.display()))?;
    let config: ProjectConfig =
        serde_json::from_slice(&raw).with_context(|| format!("reading {}", path.display()))?;
    if config.format_version != PROJECT_FORMAT {
        bail!(
            "project format {} is not supported (expected {PROJECT_FORMAT})",
            config.format_version
        );
    }
    Ok(config)
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
    fn agent_model_preference_is_optional_persistent_and_validated() {
        let legacy: ProjectConfig = serde_json::from_value(serde_json::json!({
            "format_version": 1, "name": "Legacy", "network_enabled": false
        }))
        .unwrap();
        assert!(legacy.preferred_agent_model.is_none());
        let dir = tempfile::tempdir().unwrap();
        let mut project = Project::create(&dir.path().join("preference")).unwrap();
        project
            .set_preferred_agent_model(Some("Claude Opus 5.5 Medium"))
            .unwrap();
        assert_eq!(
            read_config(&project.root)
                .unwrap()
                .preferred_agent_model
                .as_deref(),
            Some("Claude Opus 5.5 Medium")
        );
        assert!(
            project
                .set_preferred_agent_model(Some("bad\nmodel"))
                .is_err()
        );
        assert_eq!(
            project.preferred_agent_model().unwrap().as_deref(),
            Some("Claude Opus 5.5 Medium")
        );
        project.set_preferred_agent_model(None).unwrap();
        assert!(project.preferred_agent_model().unwrap().is_none());

        assert!(project.config.agent.is_none());
        project.set_agent(Some("on-device")).unwrap();
        assert_eq!(
            read_config(&project.root).unwrap().agent.as_deref(),
            Some("on-device")
        );
        assert!(project.set_agent(Some("Claude Code")).is_err());
        project.set_agent(None).unwrap();
        assert!(read_config(&project.root).unwrap().agent.is_none());
    }

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

    #[test]
    fn rename_persists_without_moving_project() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::create(&dir.path().join("demo")).unwrap();
        let root = p.root.clone();
        p.rename("  Medical research  ").unwrap();
        assert_eq!(p.config.name, "Medical research");
        assert_eq!(p.root, root);
        drop(p);
        assert_eq!(
            Project::open(&root).unwrap().config.name,
            "Medical research"
        );
    }

    #[test]
    fn metadata_rename_does_not_need_database_access() {
        let dir = tempfile::tempdir().unwrap();
        let p = Project::create(&dir.path().join("demo")).unwrap();
        let root = p.root.clone();
        drop(p);
        std::fs::remove_file(root.join("project.duckdb")).unwrap();
        assert_eq!(Project::rename_at(&root, "New title").unwrap(), "New title");
        assert_eq!(read_config(&root).unwrap().name, "New title");
        assert!(!root.join("project.duckdb").exists());
    }

    #[test]
    fn invalid_or_failed_rename_keeps_existing_name() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::create(&dir.path().join("demo")).unwrap();
        for name in ["", "  ", "one\ntwo", "one\0two"] {
            assert!(p.rename(name).is_err());
            assert_eq!(p.config.name, "demo");
            assert_eq!(read_config(&p.root).unwrap().name, "demo");
        }
        p.root = dir.path().join("missing");
        assert!(p.rename("Not saved").is_err());
        assert_eq!(p.config.name, "demo");
    }
}
