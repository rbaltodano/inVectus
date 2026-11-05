//! Connect an AI agent (target product §5.1, roadmap A6): register `invectus mcp` with an agent
//! client. Nothing is written without an explicit `apply`; a plan shows the exact change
//! first. JSON configs are merged (other servers and keys are kept), backed up, and written
//! atomically; Claude Code is configured through its own `claude mcp add` command.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// The server's name in every client's configuration.
pub const SERVER_NAME: &str = "invectus";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Client {
    ClaudeCode,
    ClaudeDesktop,
    Cursor,
}

impl Client {
    pub const ALL: [Client; 3] = [Client::ClaudeCode, Client::ClaudeDesktop, Client::Cursor];

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "claude-code" => Client::ClaudeCode,
            "claude-desktop" => Client::ClaudeDesktop,
            "cursor" => Client::Cursor,
            other => bail!("unknown client {other:?} (claude-code, claude-desktop, cursor)"),
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::ClaudeDesktop => "claude-desktop",
            Client::Cursor => "cursor",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Cursor => "Cursor",
        }
    }

    /// The JSON file this client reads MCP servers from, under `home` (and `appdata` on
    /// Windows). `None` for Claude Code, which is configured by command.
    pub fn config_file(self, home: &Path, appdata: Option<&Path>) -> Option<PathBuf> {
        match self {
            Client::ClaudeCode => None,
            Client::Cursor => Some(home.join(".cursor").join("mcp.json")),
            Client::ClaudeDesktop => Some(if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Claude/claude_desktop_config.json")
            } else if cfg!(windows) {
                appdata
                    .map(Path::to_path_buf)
                    .unwrap_or_else(|| home.join("AppData/Roaming"))
                    .join("Claude/claude_desktop_config.json")
            } else {
                home.join(".config/Claude/claude_desktop_config.json")
            }),
        }
    }
}

/// The `invectus` binary to register: the CLI itself, or the one beside the app's executable
/// (inside `inVectus.app/Contents/MacOS` on macOS).
pub fn server_binary() -> Result<PathBuf> {
    let exe = std::env::current_exe().context("finding this executable")?;
    let name = if cfg!(windows) {
        "invectus.exe"
    } else {
        "invectus"
    };
    let candidate = if exe.file_name().is_some_and(|n| n == name) {
        exe
    } else {
        exe.parent().context("executable has no folder")?.join(name)
    };
    if !candidate.is_file() {
        bail!(
            "the invectus command-line program is not installed beside this app ({}); \
             rebuild the app bundle",
            candidate.display()
        );
    }
    Ok(std::fs::canonicalize(&candidate).unwrap_or(candidate))
}

/// The server entry every JSON client takes.
pub fn server_entry(binary: &Path) -> Value {
    json!({ "command": binary.display().to_string(), "args": ["mcp"] })
}

/// What connecting a client would do. `apply` performs exactly this.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectPlan {
    pub client: Client,
    pub display_name: String,
    pub binary: String,
    /// `file` (merge into `config_file`) or `command` (run `command`).
    pub method: String,
    pub config_file: Option<String>,
    /// The entry as it is now, if any.
    pub current: Option<Value>,
    /// The entry after connecting.
    pub entry: Value,
    pub command: Option<Vec<String>>,
    /// Already connected to this binary: applying changes nothing.
    pub up_to_date: bool,
    /// Text to copy for clients the app cannot write to.
    pub manual: String,
    pub notes: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
pub struct ConnectStatus {
    pub client: Client,
    pub display_name: String,
    /// The client's config file exists (or, for Claude Code, its `claude` command was found).
    pub client_found: bool,
    pub connected: bool,
    /// The registered binary, when connected; it may be a different install than this one.
    pub binary: Option<String>,
    pub matches_this_install: bool,
}

/// Where to look: the user's home and, on Windows, `%APPDATA%`. Tests pass a temporary home.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub appdata: Option<PathBuf>,
    /// The `claude` executable for Claude Code, if found.
    pub claude: Option<PathBuf>,
}

impl Env {
    pub fn system() -> Result<Self> {
        Ok(Self {
            home: dirs::home_dir().context("no home folder")?,
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            claude: find_in_path(if cfg!(windows) {
                "claude.exe"
            } else {
                "claude"
            }),
        })
    }
}

fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|p| p.join(name))
            .find(|p| p.is_file())
    })
}

fn read_config(path: &Path) -> Result<Value> {
    match std::fs::read(path) {
        Ok(raw) if raw.iter().all(u8::is_ascii_whitespace) => Ok(json!({})),
        Ok(raw) => {
            let v: Value = serde_json::from_slice(&raw).with_context(|| {
                format!(
                    "{} is not valid JSON; fix or move it before connecting",
                    path.display()
                )
            })?;
            if !v.is_object() {
                bail!("{} does not hold a JSON object", path.display());
            }
            Ok(v)
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(json!({})),
        Err(e) => Err(e).with_context(|| format!("reading {}", path.display())),
    }
}

fn claude_code_command(claude: &Path, binary: &Path) -> Vec<String> {
    vec![
        claude.display().to_string(),
        "mcp".into(),
        "add".into(),
        "--scope".into(),
        "user".into(),
        SERVER_NAME.into(),
        "--".into(),
        binary.display().to_string(),
        "mcp".into(),
    ]
}

/// Plan connecting `client` to `binary`. Reads only.
pub fn plan(client: Client, binary: &Path, env: &Env) -> Result<ConnectPlan> {
    let entry = server_entry(binary);
    let manual_json =
        serde_json::to_string_pretty(&json!({ "mcpServers": { SERVER_NAME: entry } }))?;
    let mut notes = Vec::new();
    match client.config_file(&env.home, env.appdata.as_deref()) {
        Some(path) => {
            let config = read_config(&path)?;
            let current = config["mcpServers"].get(SERVER_NAME).cloned();
            if current.is_some() && current.as_ref() != Some(&entry) {
                notes.push("replaces an existing invectus entry".into());
            }
            if !path.exists() {
                notes.push(format!("creates {}", path.display()));
            }
            if client == Client::ClaudeDesktop {
                notes.push("restart Claude Desktop to load the server".into());
            }
            Ok(ConnectPlan {
                client,
                display_name: client.display_name().into(),
                binary: binary.display().to_string(),
                method: "file".into(),
                config_file: Some(path.display().to_string()),
                up_to_date: current.as_ref() == Some(&entry),
                current,
                entry,
                command: None,
                manual: format!("Add to {}:\n{manual_json}", path.display()),
                notes,
            })
        }
        None => {
            let manual = format!(
                "claude mcp add --scope user {SERVER_NAME} -- {} mcp",
                binary.display()
            );
            let command = env.claude.as_ref().map(|c| claude_code_command(c, binary));
            if command.is_none() {
                notes.push(
                    "the claude command was not found on PATH; run the command shown yourself"
                        .into(),
                );
            }
            Ok(ConnectPlan {
                client,
                display_name: client.display_name().into(),
                binary: binary.display().to_string(),
                method: "command".into(),
                config_file: None,
                current: None,
                entry,
                command,
                up_to_date: false,
                manual,
                notes,
            })
        }
    }
}

/// Carry out a plan (after the person agreed). JSON configs: other keys and servers are kept,
/// the previous file is copied to `<file>.invectus-backup`, and the new one is written
/// atomically. Returns what was done.
pub fn apply(plan: &ConnectPlan) -> Result<String> {
    if plan.up_to_date {
        return Ok(format!("{} is already connected", plan.display_name));
    }
    match (&plan.config_file, &plan.command) {
        (Some(path), _) => {
            let path = PathBuf::from(path);
            let mut config = read_config(&path)?;
            let servers = config
                .as_object_mut()
                .expect("read_config returns an object")
                .entry("mcpServers")
                .or_insert_with(|| json!({}));
            let Some(servers) = servers.as_object_mut() else {
                bail!("`mcpServers` in {} is not an object", path.display());
            };
            servers.insert(SERVER_NAME.into(), plan.entry.clone());
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir)?;
            }
            if path.exists() {
                std::fs::copy(&path, backup_path(&path))
                    .with_context(|| format!("backing up {}", path.display()))?;
            }
            invectus_core::project::write_json_atomic(&path, &config)?;
            Ok(format!(
                "connected {} ({})",
                plan.display_name,
                path.display()
            ))
        }
        (None, Some(cmd)) => {
            let out = std::process::Command::new(&cmd[0])
                .args(&cmd[1..])
                .output()
                .with_context(|| format!("running {}", cmd[0]))?;
            if !out.status.success() {
                bail!(
                    "`{}` failed: {}",
                    cmd.join(" "),
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Ok(format!("connected {}", plan.display_name))
        }
        (None, None) => bail!(
            "{} cannot be configured automatically here; run: {}",
            plan.display_name,
            plan.manual
        ),
    }
}

fn backup_path(path: &Path) -> PathBuf {
    let mut name = path.file_name().unwrap_or_default().to_os_string();
    name.push(".invectus-backup");
    path.with_file_name(name)
}

/// Remove the invectus entry from a JSON client (Claude Code: `claude mcp remove`).
pub fn disconnect(client: Client, env: &Env) -> Result<String> {
    match client.config_file(&env.home, env.appdata.as_deref()) {
        Some(path) => {
            let mut config = read_config(&path)?;
            let removed = config
                .get_mut("mcpServers")
                .and_then(Value::as_object_mut)
                .and_then(|s| s.remove(SERVER_NAME))
                .is_some();
            if !removed {
                return Ok(format!("{} was not connected", client.display_name()));
            }
            std::fs::copy(&path, backup_path(&path))?;
            invectus_core::project::write_json_atomic(&path, &config)?;
            Ok(format!("disconnected {}", client.display_name()))
        }
        None => {
            let claude = env
                .claude
                .as_ref()
                .context("the claude command was not found; run `claude mcp remove invectus`")?;
            let out = std::process::Command::new(claude)
                .args(["mcp", "remove", "--scope", "user", SERVER_NAME])
                .output()?;
            if !out.status.success() {
                bail!(
                    "claude mcp remove failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Ok("disconnected Claude Code".into())
        }
    }
}

/// Whether each client is connected, and to which binary. Reads only; Claude Code is read
/// from its user config (`~/.claude.json`) without running it.
pub fn status(binary: Option<&Path>, env: &Env) -> Vec<ConnectStatus> {
    Client::ALL
        .iter()
        .map(|&client| {
            let (found, entry) = match client.config_file(&env.home, env.appdata.as_deref()) {
                Some(path) => (
                    path.exists(),
                    read_config(&path)
                        .ok()
                        .and_then(|c| c["mcpServers"].get(SERVER_NAME).cloned()),
                ),
                None => (
                    env.claude.is_some(),
                    read_config(&env.home.join(".claude.json"))
                        .ok()
                        .and_then(|c| c["mcpServers"].get(SERVER_NAME).cloned()),
                ),
            };
            let registered = entry
                .as_ref()
                .and_then(|e| e["command"].as_str())
                .map(String::from);
            ConnectStatus {
                client,
                display_name: client.display_name().into(),
                client_found: found,
                connected: entry.is_some(),
                matches_this_install: matches!((&registered, binary),
                    (Some(r), Some(b)) if Path::new(r) == b),
                binary: registered,
            }
        })
        .collect()
}

/// `manifest.json` of a Claude Desktop extension (`.mcpb`) that runs the bundled binary. The
/// bundle itself is a zip of this manifest and `server/invectus`.
pub fn mcpb_manifest(version: &str) -> Value {
    let platforms: Vec<&str> = if cfg!(target_os = "macos") {
        vec!["darwin"]
    } else if cfg!(windows) {
        vec!["win32"]
    } else {
        vec!["linux"]
    };
    let entry = if cfg!(windows) {
        "server/invectus.exe"
    } else {
        "server/invectus"
    };
    json!({
        "manifest_version": "0.3",
        "name": SERVER_NAME,
        "display_name": "inVectus",
        "version": version,
        "description": "Does an idea already exist in your collection, and where are its gaps? \
            Calibrated, offline search over your own inVectus projects.",
        "long_description": "Runs the inVectus engine on this computer over stdio. Search, \
            calibrated verdicts and gap analysis run offline on your projects; verification \
            against PubMed and OpenAlex runs only when a project's network switch is on. \
            Agents can suggest decisions and write descriptions; the person decides.",
        "author": { "name": "Ryan Baltodano" },
        "license": "MIT",
        "server": {
            "type": "binary",
            "entry_point": entry,
            "mcp_config": {
                "command": format!("${{__dirname}}/{entry}"),
                "args": ["mcp"],
                "env": {}
            }
        },
        "compatibility": { "platforms": platforms },
        "tools_generated": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env(home: &Path, claude: Option<PathBuf>) -> Env {
        Env {
            home: home.to_path_buf(),
            appdata: Some(home.join("AppData/Roaming")),
            claude,
        }
    }

    #[test]
    fn json_clients_merge_keep_other_servers_and_back_up() {
        let home = tempfile::tempdir().unwrap();
        let e = env(home.path(), None);
        let bin = home.path().join("bin/invectus");
        let cursor = home.path().join(".cursor/mcp.json");
        std::fs::create_dir_all(cursor.parent().unwrap()).unwrap();
        std::fs::write(
            &cursor,
            r#"{"mcpServers":{"other":{"command":"x"}},"theme":"dark"}"#,
        )
        .unwrap();

        let p = plan(Client::Cursor, &bin, &e).unwrap();
        assert_eq!(p.method, "file");
        assert!(!p.up_to_date && p.current.is_none());
        // Planning writes nothing.
        assert!(
            !std::fs::read_to_string(&cursor)
                .unwrap()
                .contains("invectus")
        );
        apply(&p).unwrap();
        let c: Value = serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
        assert_eq!(c["mcpServers"]["other"]["command"], "x");
        assert_eq!(c["theme"], "dark");
        assert_eq!(c["mcpServers"]["invectus"], server_entry(&bin));
        assert!(backup_path(&cursor).exists());

        let again = plan(Client::Cursor, &bin, &e).unwrap();
        assert!(again.up_to_date);
        let s = status(Some(&bin), &e);
        let cs = s.iter().find(|s| s.client == Client::Cursor).unwrap();
        assert!(cs.connected && cs.matches_this_install);
        // Another install's binary replaces the entry, with a note.
        let other = plan(Client::Cursor, Path::new("/elsewhere/invectus"), &e).unwrap();
        assert!(other.notes.iter().any(|n| n.contains("replaces")));

        disconnect(Client::Cursor, &e).unwrap();
        let c: Value = serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
        assert!(c["mcpServers"].get("invectus").is_none());
        assert_eq!(c["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn missing_files_are_created_and_broken_ones_refused() {
        let home = tempfile::tempdir().unwrap();
        let e = env(home.path(), None);
        let bin = home.path().join("invectus");
        let p = plan(Client::ClaudeDesktop, &bin, &e).unwrap();
        assert!(p.notes.iter().any(|n| n.contains("creates")));
        apply(&p).unwrap();
        let path = PathBuf::from(p.config_file.unwrap());
        assert!(path.exists());
        std::fs::write(&path, "{ not json").unwrap();
        let err = plan(Client::ClaudeDesktop, &bin, &e)
            .unwrap_err()
            .to_string();
        assert!(err.contains("not valid JSON"), "{err}");
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "{ not json",
            "left alone"
        );
    }

    #[test]
    fn claude_code_uses_its_own_command() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("invectus");
        let p = plan(Client::ClaudeCode, &bin, &env(home.path(), None)).unwrap();
        assert_eq!(p.method, "command");
        assert!(
            p.command.is_none()
                && p.manual
                    .starts_with("claude mcp add --scope user invectus -- ")
        );
        assert!(apply(&p).is_err(), "nothing to run without claude");
        // A stand-in `claude` that records its arguments.
        let fake = home.path().join("claude");
        let log = home.path().join("args.txt");
        std::fs::write(
            &fake,
            format!("#!/bin/sh\necho \"$@\" > {}\n", log.display()),
        )
        .unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
            let p = plan(Client::ClaudeCode, &bin, &env(home.path(), Some(fake))).unwrap();
            apply(&p).unwrap();
            let args = std::fs::read_to_string(&log).unwrap();
            assert_eq!(
                args.trim(),
                format!("mcp add --scope user invectus -- {} mcp", bin.display())
            );
        }
        // Status reads ~/.claude.json without running anything.
        std::fs::write(
            home.path().join(".claude.json"),
            serde_json::to_string(&json!({"mcpServers": {"invectus": server_entry(&bin)}}))
                .unwrap(),
        )
        .unwrap();
        let s = status(Some(&bin), &env(home.path(), None));
        assert!(s[0].connected && s[0].matches_this_install);
    }

    #[test]
    fn mcpb_manifest_runs_the_bundled_binary() {
        let m = mcpb_manifest("0.1.0");
        assert_eq!(m["name"], "invectus");
        assert_eq!(m["server"]["type"], "binary");
        assert!(
            m["server"]["mcp_config"]["command"]
                .as_str()
                .unwrap()
                .starts_with("${__dirname}/server/invectus")
        );
        assert_eq!(m["server"]["mcp_config"]["args"], json!(["mcp"]));
    }
}
