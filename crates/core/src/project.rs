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
    /// The "External checks" switch: calls that send text derived from the collection to
    /// another service (verification connectors, a cloud LLM) are refused unless this is true
    /// (SPEC §7.7). Pulls and model downloads do not need it. The field keeps its old name so
    /// existing `project.json` files load unchanged.
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
    /// Whether AI agents may read this project through inVectus tools (item text, titles,
    /// gaps, source files). Off unless the person turned it on, on this machine.
    #[serde(default)]
    pub agent_access: bool,
    /// Random id naming this project's data key in the keychain (encryption at rest, phase 1;
    /// `keys.rs`). `None` for projects made before keys existed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
    /// Recognises the right key without storing it: HMAC-SHA-256 of a fixed label and the id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub key_check: Option<String>,
    /// `project.duckdb` is encrypted with the project key (encryption plan, phase 3). False for
    /// projects made before that, including phase-1 projects that have a key but a plaintext
    /// database.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub store_encrypted: bool,
    /// Sources and verification caches use the authenticated file format (phase 4). Separate
    /// from store_encrypted: projects created during phase 3 may still have plaintext files.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub files_encrypted: bool,
}

impl ProjectConfig {
    pub fn key_status(&self) -> KeyStatus {
        let (Some(id), Some(check)) = (&self.project_id, &self.key_check) else {
            return KeyStatus::NoKey;
        };
        match crate::keys::load(id, check) {
            Ok(Some((_, source))) => KeyStatus::Available { source },
            Ok(None) => KeyStatus::Missing,
            Err(e) => KeyStatus::Unusable {
                reason: format!("{e:#}"),
            },
        }
    }

    pub fn data_key(&self) -> Result<crate::keys::DataKey> {
        let (Some(id), Some(check)) = (&self.project_id, &self.key_check) else {
            bail!("this project was made before projects had keys");
        };
        match crate::keys::load(id, check)? {
            Some((key, _)) => Ok(key),
            None => bail!(
                "this project's key is not on this computer: restore it with the project's \
                 recovery code"
            ),
        }
    }

    /// Keep the project's key on this computer, from its recovery code.
    pub fn restore_key(&self, code: &str) -> Result<()> {
        let (Some(id), Some(check)) = (&self.project_id, &self.key_check) else {
            bail!("this project was made before projects had keys");
        };
        let key = crate::keys::DataKey::from_recovery_code(code)?;
        if key.check_value(id) != *check {
            bail!("this recovery code belongs to another project");
        }
        crate::keys::store(id, &key)
    }
}

/// Opening an encrypted project whose key is not on this computer. The app recognises it (by
/// downcast, or by [`ProjectLocked::PREFIX`] in the message) and asks for the recovery code.
#[derive(Debug)]
pub struct ProjectLocked {
    pub root: PathBuf,
}

impl ProjectLocked {
    pub const PREFIX: &str = "project locked:";
}

impl std::fmt::Display for ProjectLocked {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} is encrypted and its key is not on this computer. Unlock it with the \
             project's recovery code (`invectus key restore -p {}`)",
            Self::PREFIX,
            self.root.display(),
            self.root.display()
        )
    }
}

impl std::error::Error for ProjectLocked {}

/// Whether this project's data key is at hand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[serde(tag = "state", rename_all = "kebab-case")]
pub enum KeyStatus {
    /// Made before projects had keys (an existing project; migration is a later phase).
    NoKey,
    Available {
        source: crate::keys::KeySource,
    },
    /// Not on this computer: restore it with the recovery code.
    Missing,
    /// A stored key exists but does not belong to this project, or could not be read.
    Unusable {
        reason: String,
    },
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
        // The key is kept before `project.json` names it, so no project records a key check
        // without a stored key.
        let project_id = crate::keys::new_project_id()?;
        let key = crate::keys::DataKey::generate()?;
        crate::keys::store(&project_id, &key)?;
        let key_check = key.check_value(&project_id);
        // Owner-only from the start (audit 2026-11-12, item 6).
        for dir in ["vectors", "cache", "exports"] {
            crate::private::create_dir_all(&root.join(dir))?;
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
            agent_access: false,
            project_id: Some(project_id),
            key_check: Some(key_check),
            store_encrypted: true,
            files_encrypted: true,
        };
        write_json_atomic(&root.join("project.json"), &config)?;
        let store = Store::open_encrypted(&root.join("project.duckdb"), &key)?;
        drop(key);
        remember_encrypted(&root, &config);
        crate::private::restrict(&root.join("project.duckdb"))?;
        let mut project = Self {
            root,
            config,
            store,
        };
        // New projects start with the person's last agent-access choice on this machine.
        if agent_access_default() {
            project.set_agent_access(true)?;
        }
        Ok(project)
    }

    /// A project's folder and settings, without opening its database (works while the project
    /// is locked or open elsewhere).
    pub fn config_at(path: &Path) -> Result<(PathBuf, ProjectConfig)> {
        let root = if path.join("project.json").exists() {
            path.to_path_buf()
        } else {
            with_suffix(path)
        };
        let config = load_config(&root)?;
        validate_file_encryption(&root, &config)?;
        Ok((root, config))
    }

    pub fn open(path: &Path) -> Result<Self> {
        let (root, config) = Self::config_at(path)?;
        // Projects made by older versions: tighten the folder and its standard parts. Best
        // effort: a project on read-only media or owned by someone else still opens.
        let mut parts = vec![root.clone(), root.join("project.duckdb")];
        parts.extend(["vectors", "cache", "exports", "sources"].map(|d| root.join(d)));
        for part in parts.iter().filter(|p| p.exists()) {
            if let Err(e) = crate::private::restrict(part) {
                eprintln!(
                    "warning: could not make {} owner-only: {e:#}",
                    part.display()
                );
            }
        }
        let db = root.join("project.duckdb");
        // Opening never creates the database: a missing one is reported, not replaced by an
        // empty project.
        if !db.exists() {
            bail!(
                "{} is missing; the project's data is gone or was moved",
                db.display()
            );
        }
        if !config.store_encrypted && known_encrypted(&root, &config) {
            bail!(
                "{} was encrypted on this computer, but its project.json now says it is not. \
                 Refusing to open it: its database may have been replaced",
                root.display()
            );
        }
        let store = if config.store_encrypted {
            let key = match config.key_status() {
                KeyStatus::Available { .. } => config.data_key()?,
                KeyStatus::Missing => return Err(ProjectLocked { root }.into()),
                KeyStatus::Unusable { reason } => bail!("this project's key is unusable: {reason}"),
                KeyStatus::NoKey => bail!(
                    "{} says its database is encrypted but names no key; project.json was \
                     edited or damaged",
                    root.display()
                ),
            };
            let store = Store::open_encrypted(&db, &key)?;
            remember_encrypted(&root, &config);
            store
        } else {
            Store::open(&db)?
        };
        Ok(Self {
            root,
            config,
            store,
        })
    }

    /// Turn the project's External checks switch on or off: recorded on this machine and in
    /// `project.json`.
    pub fn set_network_enabled(&mut self, enabled: bool) -> Result<()> {
        // Turning off writes the project first, so a failure to record consent cannot leave the
        // switch on; turning on records consent first.
        if enabled {
            set_machine_consent(&self.root, Kind::ExternalChecks, true)?;
        }
        self.config.network_enabled = enabled;
        write_json_atomic(&self.root.join("project.json"), &self.config)?;
        if !enabled {
            set_machine_consent(&self.root, Kind::ExternalChecks, false)?;
        }
        Ok(())
    }

    /// Let AI agents read this project, or stop them: recorded on this machine and in
    /// `project.json`. New projects then start with the same choice.
    pub fn set_agent_access(&mut self, enabled: bool) -> Result<()> {
        if enabled {
            set_machine_consent(&self.root, Kind::AgentAccess, true)?;
        }
        self.config.agent_access = enabled;
        write_json_atomic(&self.root.join("project.json"), &self.config)?;
        if !enabled {
            set_machine_consent(&self.root, Kind::AgentAccess, false)?;
        }
        Ok(())
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
        self.config = load_config(&self.root)?;
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
        self.config = load_config(&self.root)?;
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

    /// Whether this project's data key is available here, and from where.
    pub fn key_status(&self) -> KeyStatus {
        self.config.key_status()
    }

    /// This project's data key; an error when the project has none or it is not here.
    pub fn data_key(&self) -> Result<crate::keys::DataKey> {
        self.config.data_key()
    }

    /// The recovery code of this project's key, for the person to keep somewhere safe.
    pub fn recovery_code(&self) -> Result<String> {
        Ok(self.data_key()?.recovery_code())
    }

    /// Keep this project's key on this computer from its recovery code (a project moved here,
    /// or a lost keychain entry). Refused unless the code belongs to this project.
    pub fn restore_key(&self, code: &str) -> Result<()> {
        self.config.restore_key(code)
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

/// The project's settings as they apply on this machine: External checks and agent access
/// count only when this machine recorded the person's consent for this folder, so a copied or
/// shared project arrives with both off. Every reload of `self.config` goes through here.
fn load_config(root: &Path) -> Result<ProjectConfig> {
    let mut config = read_config(root)?;
    config.network_enabled = config.network_enabled && machine_consent(root, Kind::ExternalChecks);
    config.agent_access = config.agent_access && machine_consent(root, Kind::AgentAccess);
    Ok(config)
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
    crate::private::replace(path, &serde_json::to_vec_pretty(value)?)
}

// ---- Encrypted projects known to this machine ----------------------------------------------

/// `<OS data dir>/inVectus/encrypted-projects.json`: ids and folders of the projects whose
/// database this machine has created or opened encrypted. Such a project never opens
/// unencrypted, whatever its `project.json` says: someone able to edit the folder could
/// otherwise put a plaintext database in its place, turn `store_encrypted` off, and read
/// everything written afterwards. (A project moved to a new folder with its id removed is not
/// recognised; that needs both the edit and the move.)
#[derive(Default, Serialize, Deserialize)]
struct EncryptedRecord {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    roots: Vec<String>,
    #[serde(default)]
    file_ids: Vec<String>,
    #[serde(default)]
    file_roots: Vec<String>,
}

static ENCRYPTED_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn encrypted_file() -> Option<PathBuf> {
    crate::paths::app_data_dir().map(|d| d.join("encrypted-projects.json"))
}

fn read_encrypted() -> EncryptedRecord {
    encrypted_file()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn known_encrypted(root: &Path, config: &ProjectConfig) -> bool {
    let r = read_encrypted();
    r.roots.contains(&root_key(root))
        || config
            .project_id
            .as_ref()
            .is_some_and(|id| r.ids.contains(id))
}

pub(crate) fn validate_file_encryption(root: &Path, config: &ProjectConfig) -> Result<()> {
    if !config.store_encrypted && known_encrypted(root, config) {
        bail!(
            "this project was encrypted on this computer; refusing a plaintext database downgrade"
        );
    }
    let r = read_encrypted();
    let known = r.file_roots.contains(&root_key(root))
        || config
            .project_id
            .as_ref()
            .is_some_and(|id| r.file_ids.contains(id));
    if known && !config.files_encrypted {
        bail!(
            "this project's source and cache files were encrypted on this computer; refusing a plaintext downgrade"
        );
    }
    if config.files_encrypted && !config.store_encrypted {
        bail!("encrypted source and cache files require an encrypted project database");
    }
    Ok(())
}

/// Best effort: the project still opens if the record cannot be written; it is retried on
/// every open.
fn remember_encrypted(root: &Path, config: &ProjectConfig) {
    if let Err(e) = record_encrypted(root, config) {
        eprintln!(
            "warning: could not record that {} is encrypted: {e:#}",
            root.display()
        );
    }
}

fn record_encrypted(root: &Path, config: &ProjectConfig) -> Result<()> {
    let _lock = ENCRYPTED_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let file = encrypted_file().ok_or_else(|| anyhow::anyhow!("no OS data folder"))?;
    crate::private::create_dir_all(file.parent().context("no settings folder")?)?;
    // Lock a stable sibling, not the JSON file that is atomically replaced. The process-local
    // mutex alone cannot stop another app/CLI process overwriting an intervening addition.
    let lock_path = file.with_extension("lock");
    let mut options = std::fs::OpenOptions::new();
    options.read(true).write(true).create(true).truncate(false);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    // Never truncate a file another process may currently have locked (Windows enforces
    // byte-range locks). Permissions are tightened without replacing the lock's inode.
    let record_lock = options.open(&lock_path)?;
    crate::private::restrict(&lock_path)?;
    record_lock
        .lock()
        .context("locking the encryption downgrade record")?;
    let mut r = read_encrypted();
    let key = root_key(root);
    let id = config.project_id.clone();
    let files_recorded = !config.files_encrypted
        || (r.file_roots.contains(&key) && id.as_ref().is_none_or(|id| r.file_ids.contains(id)));
    if r.roots.contains(&key) && id.as_ref().is_none_or(|id| r.ids.contains(id)) && files_recorded {
        return Ok(());
    }
    let tmp = std::env::temp_dir();
    r.roots.retain(|p| {
        p != &key
            && !(Path::new(p).starts_with(&tmp) && !Path::new(p).join("project.json").exists())
    });
    r.roots.push(key.clone());
    if let Some(id) = id.as_ref().filter(|id| !r.ids.contains(id)) {
        r.ids.push(id.clone());
    }
    if config.files_encrypted {
        r.file_roots.retain(|p| {
            p != &key
                && !(Path::new(p).starts_with(&tmp) && !Path::new(p).join("project.json").exists())
        });
        r.file_roots.push(key);
        if let Some(id) = id.filter(|id| !r.file_ids.contains(id)) {
            r.file_ids.push(id);
        }
    }
    crate::private::write(&file, &serde_json::to_vec_pretty(&r)?)
}

// ---- Consent kept per machine --------------------------------------------------------------

/// `<OS data dir>/inVectus/consent.json`: the project folders on this machine whose External
/// checks and agent access the person turned on, and the agent-access choice new projects start
/// with. Consent lives here, not only in the project, so a copied or shared project never
/// arrives with checks or agent access already allowed.
#[derive(Default, Serialize, Deserialize)]
struct Consent {
    #[serde(default)]
    external_checks: Vec<String>,
    #[serde(default)]
    agent_access: Vec<String>,
    /// The last agent-access choice the person made; new projects start with it.
    #[serde(default)]
    agent_access_default: bool,
}

#[derive(Clone, Copy)]
enum Kind {
    ExternalChecks,
    AgentAccess,
}

fn consent_file() -> Option<PathBuf> {
    crate::paths::app_data_dir().map(|d| d.join("consent.json"))
}

/// Read-modify-write of the consent file is serialised within the process.
static CONSENT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn read_consent() -> Consent {
    consent_file()
        .and_then(|f| std::fs::read(f).ok())
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_consent(c: &Consent) -> Result<()> {
    let Some(file) = consent_file() else {
        bail!("no OS data folder to keep consent choices in");
    };
    crate::private::write(&file, &serde_json::to_vec_pretty(c)?)
}

fn root_key(root: &Path) -> String {
    root.canonicalize()
        .unwrap_or_else(|_| root.to_path_buf())
        .display()
        .to_string()
}

fn machine_consent(root: &Path, kind: Kind) -> bool {
    let c = read_consent();
    let list = match kind {
        Kind::ExternalChecks => &c.external_checks,
        Kind::AgentAccess => &c.agent_access,
    };
    list.contains(&root_key(root))
}

fn set_machine_consent(root: &Path, kind: Kind, on: bool) -> Result<()> {
    let _lock = CONSENT_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let key = root_key(root);
    let mut c = read_consent();
    let list = match kind {
        Kind::ExternalChecks => &mut c.external_checks,
        Kind::AgentAccess => {
            c.agent_access_default = on;
            &mut c.agent_access
        }
    };
    // Replace this project's entry. Others are kept even when their folder is not reachable
    // right now (an unmounted drive), except vanished folders under the temp folder (test runs).
    let tmp = std::env::temp_dir();
    list.retain(|r| {
        r != &key
            && !(Path::new(r).starts_with(&tmp) && !Path::new(r).join("project.json").exists())
    });
    if on {
        list.push(key);
    }
    write_consent(&c)
}

/// Whether new projects start with agent access on: the person's last choice on this machine
/// (off until they first turn it on).
pub fn agent_access_default() -> bool {
    read_consent().agent_access_default
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn downgrade_record_child() {
        let Ok(base) = std::env::var("INVECTUS_GUARD_TEST_ROOT") else {
            return;
        };
        let child: usize = std::env::var("INVECTUS_GUARD_TEST_CHILD")
            .unwrap()
            .parse()
            .unwrap();
        for i in 0..16 {
            let root = Path::new(&base).join(format!("project-{child}-{i}.invectus"));
            std::fs::create_dir_all(&root).unwrap();
            let config: ProjectConfig = serde_json::from_value(serde_json::json!({
                "format_version":1,"name":"Guard test","network_enabled":false,
                "project_id":format!("{child:016x}{i:016x}"),
                "store_encrypted":true,"files_encrypted":true
            }))
            .unwrap();
            write_json_atomic(&root.join("project.json"), &config).unwrap();
            record_encrypted(&root, &config).unwrap();
        }
    }

    #[test]
    fn concurrent_processes_preserve_all_encryption_downgrade_records() {
        let dir = tempfile::tempdir().unwrap();
        let settings = dir.path().join("settings");
        let mut children = Vec::new();
        for i in 0..4 {
            children.push(
                std::process::Command::new(std::env::current_exe().unwrap())
                    .args(["--exact", "project::tests::downgrade_record_child"])
                    .env("INVECTUS_DATA_DIR", &settings)
                    .env("INVECTUS_GUARD_TEST_ROOT", dir.path())
                    .env("INVECTUS_GUARD_TEST_CHILD", i.to_string())
                    .stdout(std::process::Stdio::null())
                    .stderr(std::process::Stdio::piped())
                    .spawn()
                    .unwrap(),
            );
        }
        for child in children {
            let output = child.wait_with_output().unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
        }
        let record: EncryptedRecord = serde_json::from_slice(
            &std::fs::read(settings.join("encrypted-projects.json")).unwrap(),
        )
        .unwrap();
        assert_eq!(record.ids.len(), 64);
        assert_eq!(record.roots.len(), 64);
        assert_eq!(record.file_ids.len(), 64);
        assert_eq!(record.file_roots.len(), 64);
    }

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
        // A new project has a key: stored, recognised, restorable from its recovery code.
        let id = p.config.project_id.clone().unwrap();
        assert!(matches!(p.key_status(), KeyStatus::Available { .. }));
        let code = p.recovery_code().unwrap();
        let json = std::fs::read_to_string(p.root.join("project.json")).unwrap();
        assert!(
            json.contains(&id) && !json.contains(&code),
            "the key never goes into project.json"
        );
        assert!(p.config.store_encrypted && p.store.is_encrypted());
        crate::keys::forget(&id).unwrap();
        assert_eq!(p.key_status(), KeyStatus::Missing);
        assert!(p.data_key().is_err());
        let other = crate::keys::DataKey::generate().unwrap().recovery_code();
        assert!(
            p.restore_key(&other)
                .unwrap_err()
                .to_string()
                .contains("another project")
        );
        p.restore_key(&code).unwrap();
        assert!(matches!(p.key_status(), KeyStatus::Available { .. }));
        // Windows locks the database file, so close this handle before reopening.
        let root = p.root.clone();
        drop(p);
        assert_eq!(Project::open(&root).unwrap().recovery_code().unwrap(), code);

        // Without its key the project is locked: the error says so, and the recovery code
        // unlocks it without opening the database.
        crate::keys::forget(&id).unwrap();
        let Err(e) = Project::open(&root) else {
            panic!("opened without its key")
        };
        assert!(e.downcast_ref::<ProjectLocked>().is_some(), "{e:#}");
        assert!(e.to_string().starts_with(ProjectLocked::PREFIX));
        let (_, config) = Project::config_at(&root).unwrap();
        config.restore_key(&code).unwrap();
        assert!(Project::open(&root).unwrap().store.is_encrypted());

        // A plaintext database swapped in with `store_encrypted` turned off is refused, even
        // with the project id removed too (this machine remembers the folder).
        let db = root.join("project.duckdb");
        std::fs::rename(&db, root.join("saved.duckdb")).unwrap();
        drop(crate::store::Store::open(&db).unwrap());
        let mut edited = config.clone();
        edited.store_encrypted = false;
        edited.project_id = None;
        edited.key_check = None;
        write_json_atomic(&root.join("project.json"), &edited).unwrap();
        let Err(e) = Project::open(&root) else {
            panic!("opened a downgraded project")
        };
        assert!(
            e.to_string().contains("was encrypted on this computer"),
            "{e:#}"
        );
        write_json_atomic(&root.join("project.json"), &config).unwrap();

        // A missing database is reported, not silently recreated.
        std::fs::remove_file(&db).unwrap();
        let Err(e) = Project::open(&root) else {
            panic!("recreated a missing database")
        };
        assert!(e.to_string().contains("is missing"), "{e:#}");
        assert!(!db.exists());
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

    #[cfg(unix)]
    #[test]
    fn project_folder_is_owner_only_and_copies_do_not_carry_consent() {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        let mut p = Project::create(&dir.path().join("a")).unwrap();
        for d in ["", "vectors", "cache", "exports"] {
            let m = std::fs::metadata(p.root.join(d))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(m, 0o700, "{d}");
        }
        for f in ["project.json", "project.duckdb"] {
            let m = std::fs::metadata(p.root.join(f))
                .unwrap()
                .permissions()
                .mode()
                & 0o777;
            assert_eq!(m, 0o600, "{f}");
        }
        p.set_network_enabled(true).unwrap();
        let root = p.root.clone();
        drop(p);
        assert!(
            Project::open(&root).unwrap().config.network_enabled,
            "kept on this machine"
        );
        // A copy of the folder (another path, or another machine) arrives with checks off,
        // even though its project.json says they were on.
        let copy = dir.path().join("copy.invectus");
        std::fs::create_dir_all(&copy).unwrap();
        for f in ["project.json", "project.duckdb"] {
            std::fs::copy(root.join(f), copy.join(f)).unwrap();
        }
        let raw: serde_json::Value =
            serde_json::from_slice(&std::fs::read(copy.join("project.json")).unwrap()).unwrap();
        assert_eq!(raw["network_enabled"], true);
        assert!(!Project::open(&copy).unwrap().config.network_enabled);
        Project::open(&root)
            .unwrap()
            .set_network_enabled(false)
            .unwrap();
    }
}
