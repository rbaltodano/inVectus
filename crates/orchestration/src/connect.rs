//! Connect an AI agent (target product §5.1, roadmap A6): register `invectus mcp` with an agent
//! client. Nothing is written without an explicit `apply`; a plan shows the exact change
//! first. JSON configs are merged (other servers and keys are kept), backed up, and written
//! atomically; Claude Code and Codex are configured through their own `mcp add` commands.
//!
//! Sign-in is left to each client: inVectus never reads, stores or sends a token.
//! [`account`] asks the client's own CLI who is signed in (`claude auth status`, `codex login
//! status`), so the Settings screen can say whether the agent runs on a Claude or ChatGPT
//! subscription or an API key.

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
    Codex,
    Cursor,
}

impl Client {
    pub const ALL: [Client; 4] = [
        Client::ClaudeCode,
        Client::ClaudeDesktop,
        Client::Codex,
        Client::Cursor,
    ];

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "claude-code" => Client::ClaudeCode,
            "claude-desktop" => Client::ClaudeDesktop,
            "codex" => Client::Codex,
            "cursor" => Client::Cursor,
            other => bail!("unknown client {other:?} (claude-code, claude-desktop, codex, cursor)"),
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Client::ClaudeCode => "claude-code",
            Client::ClaudeDesktop => "claude-desktop",
            Client::Codex => "codex",
            Client::Cursor => "cursor",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Client::ClaudeCode => "Claude Code",
            Client::ClaudeDesktop => "Claude Desktop",
            Client::Codex => "Codex",
            Client::Cursor => "Cursor",
        }
    }

    /// The JSON file this client reads MCP servers from, under `home` (and `appdata` on
    /// Windows). `None` for Claude Code and Codex, which are configured by command.
    pub fn config_file(self, home: &Path, appdata: Option<&Path>) -> Option<PathBuf> {
        match self {
            Client::ClaudeCode | Client::Codex => None,
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
    /// Who the client is signed in as; filled by [`account`] when asked for (it runs the
    /// client's CLI), `None` from [`status`].
    pub account: Option<Account>,
}

/// Where to look: the user's home and, on Windows, `%APPDATA%`. Tests pass a temporary home.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub appdata: Option<PathBuf>,
    /// The `claude` executable for Claude Code, if found.
    pub claude: Option<PathBuf>,
    /// The `codex` executable, if found.
    pub codex: Option<PathBuf>,
    /// Codex's settings folder: `$CODEX_HOME`, else `~/.codex`.
    pub codex_home: PathBuf,
}

impl Env {
    pub fn system() -> Result<Self> {
        let home = dirs::home_dir().context("no home folder")?;
        let exe = |name: &str| find_in_path(&format!("{name}{}", std::env::consts::EXE_SUFFIX));
        Ok(Self {
            codex_home: std::env::var_os("CODEX_HOME")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
                .unwrap_or_else(|| home.join(".codex")),
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            claude: exe("claude"),
            codex: exe("codex"),
            home,
        })
    }

    /// The CLI that configures a command-configured client.
    fn cli(&self, client: Client) -> Option<&PathBuf> {
        match client {
            Client::ClaudeCode => self.claude.as_ref(),
            Client::Codex => self.codex.as_ref(),
            Client::ClaudeDesktop | Client::Cursor => None,
        }
    }

    /// The invectus entry a command-configured client has now, read from its own files
    /// (`~/.claude.json`, `$CODEX_HOME/config.toml`) without running it.
    fn registered(&self, client: Client) -> Option<Value> {
        match client {
            Client::ClaudeCode => read_config(&self.home.join(".claude.json"))
                .ok()
                .and_then(|c| c["mcpServers"].get(SERVER_NAME).cloned()),
            Client::Codex => {
                let raw = std::fs::read_to_string(self.codex_home.join("config.toml")).ok()?;
                let config: toml::Table = raw.parse().ok()?;
                let entry = config.get("mcp_servers")?.get(SERVER_NAME)?;
                serde_json::to_value(entry).ok()
            }
            Client::ClaudeDesktop | Client::Cursor => None,
        }
    }
}

/// A command on `PATH`, else in the usual install folders: an app opened from the Finder or
/// Dock gets a minimal `PATH` without Homebrew's or `~/.local/bin`.
pub(crate) fn find_in_path(name: &str) -> Option<PathBuf> {
    let home = dirs::home_dir();
    let usual = ["/opt/homebrew/bin", "/usr/local/bin"]
        .into_iter()
        .map(PathBuf::from)
        .chain(home.map(|h| h.join(".local/bin")));
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).collect::<Vec<_>>())
        .unwrap_or_default()
        .into_iter()
        .chain(usual.filter(|_| cfg!(unix)))
        .map(|p| p.join(name))
        .find(|p| p.is_file())
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

/// `<cli> mcp add …` without the program: Claude Code registers for the user (all projects);
/// Codex's servers are always global, and adding again replaces the entry.
fn add_args(client: Client, binary: &Path) -> Vec<String> {
    let scope: &[&str] = match client {
        Client::ClaudeCode => &["--scope", "user"],
        _ => &[],
    };
    ["mcp", "add"]
        .iter()
        .chain(scope)
        .chain(&[SERVER_NAME, "--"])
        .map(|s| s.to_string())
        .chain([binary.display().to_string(), "mcp".into()])
        .collect()
}

fn remove_args(client: Client) -> Vec<&'static str> {
    match client {
        Client::ClaudeCode => vec!["mcp", "remove", "--scope", "user", SERVER_NAME],
        _ => vec!["mcp", "remove", SERVER_NAME],
    }
}

/// The program name each command-configured client's CLI goes by.
fn cli_name(client: Client) -> &'static str {
    match client {
        Client::Codex => "codex",
        _ => "claude",
    }
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
            let name = cli_name(client);
            let args = add_args(client, binary);
            let manual = format!("{name} {}", args.join(" "));
            let command = env.cli(client).map(|c| {
                std::iter::once(c.display().to_string())
                    .chain(args)
                    .collect::<Vec<_>>()
            });
            if command.is_none() {
                notes.push(format!(
                    "the {name} command was not found on PATH; run the command shown yourself"
                ));
            }
            // Codex replaces an existing entry; Claude Code has always been planned as new.
            let current = match client {
                Client::Codex => env.registered(client),
                _ => None,
            };
            if current.is_some() && current.as_ref() != Some(&entry) {
                notes.push("replaces an existing invectus entry".into());
            }
            Ok(ConnectPlan {
                client,
                display_name: client.display_name().into(),
                binary: binary.display().to_string(),
                method: "command".into(),
                config_file: None,
                up_to_date: current.as_ref() == Some(&entry),
                current,
                entry,
                command,
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

/// Remove the invectus entry from a client (Claude Code and Codex: their `mcp remove`).
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
            let name = cli_name(client);
            let args = remove_args(client);
            let cli = env.cli(client).with_context(|| {
                format!(
                    "the {name} command was not found; run `{name} {}`",
                    args.join(" ")
                )
            })?;
            let out = std::process::Command::new(cli).args(&args).output()?;
            if !out.status.success() {
                bail!(
                    "{name} mcp remove failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            Ok(format!("disconnected {}", client.display_name()))
        }
    }
}

/// Whether each client is connected, and to which binary. Reads only; Claude Code and Codex
/// are read from their own config files without running them.
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
                None => (env.cli(client).is_some(), env.registered(client)),
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
                account: None,
            }
        })
        .collect()
}

/// Who a client is signed in as, from its own CLI. Tokens stay with the client.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Account {
    /// `signed-in`, `signed-out` or `unknown`.
    pub status: String,
    /// What the agent's use is billed to, e.g. "Claude Pro subscription", "ChatGPT
    /// subscription", "Anthropic API key".
    pub label: Option<String>,
    pub email: Option<String>,
    /// What to do next, when not signed in or not checked.
    pub message: Option<String>,
}

impl Account {
    fn new(status: &str, label: Option<String>, email: Option<String>) -> Self {
        Self {
            status: status.into(),
            label,
            email,
            message: None,
        }
    }

    fn unknown(message: impl Into<String>) -> Self {
        Self {
            message: Some(message.into()),
            ..Self::new("unknown", None, None)
        }
    }
}

/// Both CLIs answer from local state in about a second; a stuck one is abandoned.
const ACCOUNT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Ask a client's CLI who is signed in: `claude auth status` (JSON) or `codex login status`.
/// `None` for clients without a CLI to ask (Claude Desktop, Cursor). Not run while offline:
/// whether the CLIs touch the network for this is theirs to decide.
pub fn account(client: Client, env: &Env) -> Option<Account> {
    let args: &[&str] = match client {
        Client::ClaudeCode => &["auth", "status"],
        Client::Codex => &["login", "status"],
        Client::ClaudeDesktop | Client::Cursor => return None,
    };
    let name = cli_name(client);
    let Some(cli) = env.cli(client) else {
        return Some(Account::unknown(format!(
            "the {name} command was not found on PATH"
        )));
    };
    if invectus_core::net::offline() {
        return Some(Account::unknown("not checked while offline"));
    }
    let out = match run_with_timeout(cli, args, ACCOUNT_TIMEOUT) {
        Ok(out) => out,
        Err(e) => {
            return Some(Account::unknown(format!(
                "`{name} {}` failed: {e:#}",
                args.join(" ")
            )));
        }
    };
    Some(match client {
        Client::ClaudeCode => claude_account(&out.stdout),
        _ => codex_account(&format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )),
    })
}

fn run_with_timeout(
    program: &Path,
    args: &[&str],
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    run_with_timeout_env(program, args, &[], timeout)
}

/// Run a program with extra environment variables, killing it after `timeout`.
pub(crate) fn run_with_timeout_env(
    program: &Path,
    args: &[&str],
    env: &[(&str, &str)],
    timeout: std::time::Duration,
) -> Result<std::process::Output> {
    use std::process::Stdio;
    let mut child = std::process::Command::new(program)
        .args(args)
        .envs(env.iter().copied())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;
    let start = std::time::Instant::now();
    while child.try_wait()?.is_none() {
        if start.elapsed() > timeout {
            let _ = child.kill();
            let _ = child.wait();
            bail!("timed out after {}s", timeout.as_secs());
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    Ok(child.wait_with_output()?)
}

/// Lower-case letters and digits only, for matching the CLIs' loosely spelled values.
fn squash(s: &str) -> String {
    s.chars()
        .filter(char::is_ascii_alphanumeric)
        .collect::<String>()
        .to_ascii_lowercase()
}

/// `claude auth status` prints JSON: `loggedIn`, `authMethod` (`claude.ai` for a
/// subscription), `apiProvider`, `subscriptionType`, `email`.
fn claude_account(stdout: &[u8]) -> Account {
    let Ok(v) = serde_json::from_slice::<Value>(stdout) else {
        return Account::unknown("`claude auth status` gave no readable answer");
    };
    if v["loggedIn"] == false {
        return Account {
            message: Some("run `claude auth login`".into()),
            ..Account::new("signed-out", None, None)
        };
    }
    let field = |k: &str| v[k].as_str().map(str::trim).filter(|s| !s.is_empty());
    let label = match field("apiProvider").map(squash).as_deref() {
        Some("bedrock") => Some("Amazon Bedrock".to_string()),
        Some("vertex") => Some("Google Vertex AI".to_string()),
        Some("foundry") => Some("Microsoft Foundry".to_string()),
        _ => match field("authMethod").map(squash).as_deref() {
            Some("apikey" | "anthropicapikey" | "anthropicauthtoken") => {
                Some("Anthropic API key".to_string())
            }
            _ => field("subscriptionType").map(claude_plan_label),
        },
    };
    Account::new("signed-in", label, field("email").map(String::from))
}

fn claude_plan_label(plan: &str) -> String {
    let s = squash(plan);
    let s = s.strip_prefix("claude").unwrap_or(&s);
    let s = s.strip_suffix("subscription").unwrap_or(s);
    let name = match s {
        "pro" => "Pro",
        "max" => "Max",
        "max5x" => "Max 5x",
        "max20x" => "Max 20x",
        "team" => "Team",
        "enterprise" => "Enterprise",
        "free" => "Free",
        _ => plan.trim(),
    };
    format!("Claude {name} subscription")
}

/// `codex login status` prints "Logged in using ChatGPT", "Logged in using an API key - …"
/// (a masked key, which is never repeated) or "Not logged in".
fn codex_account(text: &str) -> Account {
    let t = text.to_ascii_lowercase();
    if t.contains("not logged in") {
        Account {
            message: Some("run `codex login`".into()),
            ..Account::new("signed-out", None, None)
        }
    } else if t.contains("logged in using chatgpt") {
        Account::new("signed-in", Some("ChatGPT subscription".into()), None)
    } else if t.contains("logged in using an api key") {
        Account::new("signed-in", Some("OpenAI API key".into()), None)
    } else if t.contains("logged in") {
        Account::new("signed-in", None, None)
    } else {
        Account::unknown("`codex login status` gave no readable answer")
    }
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
            codex: None,
            codex_home: home.join(".codex"),
        }
    }

    /// A stand-in CLI: a shell script that logs its arguments to `<name>.log`, prints
    /// `stdout` and exits with `code`.
    #[cfg(unix)]
    fn fake_cli(dir: &Path, name: &str, stdout: &str, code: i32) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        let log = dir.join(format!("{name}.log"));
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\necho \"$@\" >> {}\ncat <<'OUT'\n{stdout}\nOUT\nexit {code}\n",
                log.display()
            ),
        )
        .unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    #[test]
    fn codex_uses_its_own_command_and_reads_config_toml() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("invectus");
        let mut e = env(home.path(), None);
        let p = plan(Client::Codex, &bin, &e).unwrap();
        assert_eq!(p.method, "command");
        assert_eq!(
            p.manual,
            format!("codex mcp add invectus -- {} mcp", bin.display())
        );
        assert!(p.command.is_none() && !p.up_to_date);
        assert!(!status(Some(&bin), &e)[2].client_found);
        // Status and plans read the entry Codex wrote, without running it.
        std::fs::create_dir_all(&e.codex_home).unwrap();
        std::fs::write(
            e.codex_home.join("config.toml"),
            format!(
                "model = \"x\"\n[mcp_servers.invectus]\ncommand = \"{}\"\nargs = [\"mcp\"]\n",
                bin.display()
            ),
        )
        .unwrap();
        let s = status(Some(&bin), &e);
        assert_eq!(s[2].client, Client::Codex);
        assert!(s[2].connected && s[2].matches_this_install);
        assert!(plan(Client::Codex, &bin, &e).unwrap().up_to_date);
        let other = plan(Client::Codex, Path::new("/elsewhere/invectus"), &e).unwrap();
        assert!(!other.up_to_date && other.notes.iter().any(|n| n.contains("replaces")));
        #[cfg(unix)]
        {
            e.codex = Some(fake_cli(home.path(), "codex", "", 0));
            apply(&plan(Client::Codex, Path::new("/elsewhere/invectus"), &e).unwrap()).unwrap();
            disconnect(Client::Codex, &e).unwrap();
            let log = std::fs::read_to_string(home.path().join("codex.log")).unwrap();
            assert_eq!(
                log,
                "mcp add invectus -- /elsewhere/invectus mcp\nmcp remove invectus\n"
            );
        }
    }

    #[test]
    fn accounts_name_the_subscription_or_key() {
        let claude = |json: serde_json::Value| claude_account(json.to_string().as_bytes());
        assert_eq!(
            claude(
                json!({"loggedIn": true, "authMethod": "claude.ai", "apiProvider": "firstParty",
                "email": "a@b.c", "subscriptionType": "pro"})
            ),
            Account::new(
                "signed-in",
                Some("Claude Pro subscription".into()),
                Some("a@b.c".into())
            )
        );
        assert_eq!(
            claude(json!({"loggedIn": true, "subscriptionType": "claude_max_20x_subscription"}))
                .label
                .as_deref(),
            Some("Claude Max 20x subscription")
        );
        assert_eq!(
            claude(json!({"loggedIn": true, "authMethod": "api_key", "subscriptionType": "pro"}))
                .label
                .as_deref(),
            Some("Anthropic API key")
        );
        assert_eq!(
            claude(json!({"loggedIn": true, "apiProvider": "bedrock"}))
                .label
                .as_deref(),
            Some("Amazon Bedrock")
        );
        let out = claude(json!({"loggedIn": false}));
        assert_eq!(out.status, "signed-out");
        assert!(out.message.unwrap().contains("claude auth login"));
        assert_eq!(claude_account(b"oops").status, "unknown");

        assert_eq!(
            codex_account("Logged in using ChatGPT\n").label.as_deref(),
            Some("ChatGPT subscription")
        );
        let key = codex_account("Logged in using an API key - sk-proj-***ABCD\n");
        assert_eq!(key.label.as_deref(), Some("OpenAI API key"));
        assert!(
            !format!("{key:?}").contains("sk-"),
            "the key is never repeated"
        );
        assert_eq!(codex_account("Not logged in").status, "signed-out");
        assert_eq!(codex_account("").status, "unknown");
    }

    #[cfg(unix)]
    #[test]
    fn account_runs_the_clients_own_cli() {
        let home = tempfile::tempdir().unwrap();
        let mut e = env(home.path(), None);
        assert_eq!(account(Client::Cursor, &e), None);
        assert_eq!(account(Client::Codex, &e).unwrap().status, "unknown");
        e.claude = Some(fake_cli(
            home.path(),
            "claude",
            r#"{"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}"#,
            0,
        ));
        e.codex = Some(fake_cli(home.path(), "codex", "Not logged in", 1));
        let c = account(Client::ClaudeCode, &e).unwrap();
        assert_eq!(c.label.as_deref(), Some("Claude Max subscription"));
        assert_eq!(account(Client::Codex, &e).unwrap().status, "signed-out");
        assert_eq!(
            std::fs::read_to_string(home.path().join("claude.log")).unwrap(),
            "auth status\n"
        );
        let slow = home.path().join("slow");
        std::fs::write(&slow, "#!/bin/sh\nsleep 5\n").unwrap();
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&slow, std::fs::Permissions::from_mode(0o755)).unwrap();
        }
        let err = run_with_timeout(&slow, &[], std::time::Duration::from_millis(200))
            .unwrap_err()
            .to_string();
        assert!(err.contains("timed out"), "{err}");
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
