//! Connect an AI agent (target product §5.1, roadmap A6): register `invectus mcp` with an agent
//! client. Nothing is written without an explicit `apply`; a plan shows the exact change
//! first. JSON configs are merged (other servers and keys are kept), backed up, and written
//! atomically; Claude Code, Codex and Grok Build are configured through their own `mcp add`
//! commands.
//!
//! Every client is one entry in [`CLIENTS`] (`docs/agent-clients-plan.md`): its name, how it
//! is configured, and how to ask who is signed in. Adding a client that takes a JSON file or an `mcp add` command is a new entry, not new
//! code. A client not listed can still be connected through [`plan_custom`].
//!
//! Sign-in is left to each client: inVectus never reads, stores or sends a token.
//! [`account`] asks the client's own CLI who is signed in (`claude auth status`, `codex login
//! status`, `grok models`), so the Settings screen can say whether the agent runs on a Claude or
//! ChatGPT subscription or an API key, or whether Grok Build is signed in.

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// The server's name in every client's configuration.
pub const SERVER_NAME: &str = "invectus";

/// The id plans and requests use for a client that is not in [`CLIENTS`].
pub const CUSTOM: &str = "custom";

/// Where a client keeps a file.
#[derive(Clone, Copy, Debug)]
pub enum ConfigPath {
    /// Under the home folder on every system.
    Home(&'static str),
    /// In the per-user application settings folder: `~/Library/Application Support` on macOS,
    /// `%APPDATA%` on Windows, `~/.config` on Linux.
    AppConfig(&'static str),
}

/// The shape of the invectus entry a JSON client takes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Shape {
    /// `{ "command": …, "args": ["mcp"] }`
    Standard,
    /// VS Code: `{ "type": "stdio", "command": …, "args": ["mcp"] }`
    Stdio,
}

/// How a client is configured.
#[derive(Clone, Copy, Debug)]
pub enum Method {
    /// Merge the entry under `key` in a JSON file.
    JsonFile {
        path: ConfigPath,
        key: &'static str,
        shape: Shape,
    },
    /// Run the client's own `mcp add` and `mcp remove`.
    Cli(CliSpec),
}

#[derive(Clone, Copy, Debug)]
pub struct CliSpec {
    /// The program's name, without `.exe`.
    pub program: &'static str,
    /// The client's settings folder: an environment variable that overrides it, and the default
    /// under the home folder.
    pub home: Option<(&'static str, &'static str)>,
    /// Also look for the program in `<settings folder>/bin`. Grok's installer links `grok` into
    /// `~/.local/bin` only when that is on PATH; otherwise it is reached through the PATH line it
    /// adds to the shell profile, which an app opened from the Finder does not read.
    pub bin_in_home: bool,
    /// Arguments after `mcp add` / `mcp remove`, before the server name.
    pub scope: &'static [&'static str],
    /// Where the client records the entry, read without running it.
    pub registered: Registered,
    /// `mcp add` replaces an existing entry, so the plan shows the current one.
    pub replaces: bool,
}

#[derive(Clone, Copy, Debug)]
pub enum Registered {
    /// A JSON file under the home folder, entry under `key`.
    Json {
        file: &'static str,
        key: &'static str,
    },
    /// A TOML file in the client's settings folder, entry under `table`. With
    /// `honour_enabled`, `enabled = false` counts as not connected and the key is ignored
    /// otherwise.
    Toml {
        file: &'static str,
        table: &'static str,
        honour_enabled: bool,
    },
}

/// Which CLI command says who is signed in, and how to read its answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AccountProbe {
    Claude,
    Codex,
    Grok,
}

impl AccountProbe {
    fn args(self) -> &'static [&'static str] {
        match self {
            AccountProbe::Claude => &["auth", "status"],
            AccountProbe::Codex => &["login", "status"],
            AccountProbe::Grok => &["models"],
        }
    }

    fn read(self, out: &std::process::Output) -> Account {
        let text = format!(
            "{}\n{}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        );
        match self {
            AccountProbe::Claude => claude_account(&out.stdout),
            AccountProbe::Codex => codex_account(&text),
            AccountProbe::Grok => grok_account(&text, out.status.success()),
        }
    }
}

/// One agent client.
#[derive(Debug)]
pub struct ClientSpec {
    pub id: &'static str,
    pub display_name: &'static str,
    /// Names the client gives in the MCP handshake; one ending in `*` is a prefix.
    pub aliases: &'static [&'static str],
    /// Shown first in Settings.
    pub featured: bool,
    /// A real session has used `invectus mcp` through this client (see PROGRESS.md). Unchecked
    /// clients are labelled in Settings.
    pub checked: bool,
    /// One sentence for Settings: what connecting does.
    pub how: &'static str,
    pub method: Method,
    pub account: Option<AccountProbe>,
    /// Added to the plan's notes.
    pub note: Option<&'static str>,
}

/// Every client inVectus can connect, in display order. Paths and entry shapes of unchecked
/// clients come from their documentation (2026-11-09) and need a real session to confirm.
pub static CLIENTS: &[ClientSpec] = &[
    ClientSpec {
        id: "claude-code",
        display_name: "Claude Code",
        aliases: &["claude-code"],
        featured: true,
        checked: true,
        how: "Registers inVectus with Claude Code's own `claude mcp add` command.",
        method: Method::Cli(CliSpec {
            program: "claude",
            home: None,
            bin_in_home: false,
            scope: &["--scope", "user"],
            registered: Registered::Json {
                file: ".claude.json",
                key: "mcpServers",
            },
            // Claude Code has always been planned as new.
            replaces: false,
        }),
        account: Some(AccountProbe::Claude),
        note: None,
    },
    ClientSpec {
        id: "claude-desktop",
        display_name: "Claude Desktop",
        aliases: &["claude-desktop"],
        featured: true,
        checked: false,
        how: "Adds inVectus to Claude Desktop's configuration file.",
        method: Method::JsonFile {
            path: ConfigPath::AppConfig("Claude/claude_desktop_config.json"),
            key: "mcpServers",
            shape: Shape::Standard,
        },
        account: None,
        note: Some("restart Claude Desktop to load the server"),
    },
    ClientSpec {
        id: "codex",
        display_name: "Codex",
        aliases: &["codex", "codex-mcp-client"],
        featured: true,
        checked: true,
        how: "Registers inVectus with Codex's own `codex mcp add` command (saved in \
              ~/.codex/config.toml).",
        method: Method::Cli(CliSpec {
            program: "codex",
            home: Some(("CODEX_HOME", ".codex")),
            bin_in_home: false,
            scope: &[],
            registered: Registered::Toml {
                file: "config.toml",
                table: "mcp_servers",
                honour_enabled: false,
            },
            replaces: true,
        }),
        account: Some(AccountProbe::Codex),
        note: None,
    },
    ClientSpec {
        id: "cursor",
        display_name: "Cursor",
        aliases: &["cursor"],
        featured: true,
        checked: false,
        how: "Adds inVectus to Cursor's ~/.cursor/mcp.json.",
        method: Method::JsonFile {
            path: ConfigPath::Home(".cursor/mcp.json"),
            key: "mcpServers",
            shape: Shape::Standard,
        },
        account: None,
        note: None,
    },
    ClientSpec {
        id: "grok",
        display_name: "Grok Build",
        aliases: &["grok", "grok-shell-*"],
        featured: true,
        checked: false,
        how: "Registers inVectus with Grok Build's own `grok mcp add` command (saved in \
              ~/.grok/config.toml).",
        method: Method::Cli(CliSpec {
            program: "grok",
            home: Some(("GROK_HOME", ".grok")),
            bin_in_home: true,
            scope: &["--scope", "user"],
            // Grok also loads servers from `~/.claude.json`, but its own entry takes
            // precedence, so only that one is read.
            registered: Registered::Toml {
                file: "config.toml",
                table: "mcp_servers",
                honour_enabled: true,
            },
            replaces: true,
        }),
        account: Some(AccountProbe::Grok),
        note: None,
    },
    ClientSpec {
        id: "gemini-cli",
        display_name: "Gemini CLI",
        aliases: &["gemini-cli"],
        featured: false,
        checked: false,
        how: "Adds inVectus to Gemini CLI's user settings, ~/.gemini/settings.json.",
        method: Method::JsonFile {
            path: ConfigPath::Home(".gemini/settings.json"),
            key: "mcpServers",
            shape: Shape::Standard,
        },
        account: None,
        note: None,
    },
    ClientSpec {
        id: "kiro",
        display_name: "Kiro",
        aliases: &["kiro"],
        featured: false,
        checked: false,
        how: "Adds inVectus to Kiro's user MCP settings, ~/.kiro/settings/mcp.json.",
        method: Method::JsonFile {
            path: ConfigPath::Home(".kiro/settings/mcp.json"),
            key: "mcpServers",
            shape: Shape::Standard,
        },
        account: None,
        note: None,
    },
    ClientSpec {
        id: "lm-studio",
        display_name: "LM Studio",
        aliases: &["lm-studio", "lmstudio"],
        featured: false,
        checked: false,
        how: "Adds inVectus to LM Studio's ~/.lmstudio/mcp.json.",
        method: Method::JsonFile {
            path: ConfigPath::Home(".lmstudio/mcp.json"),
            key: "mcpServers",
            shape: Shape::Standard,
        },
        account: None,
        note: None,
    },
    ClientSpec {
        id: "vscode",
        display_name: "VS Code",
        aliases: &["vscode", "visual studio code"],
        featured: false,
        checked: false,
        how: "Adds inVectus to VS Code's user MCP configuration (mcp.json in the user settings \
              folder), for agent mode.",
        method: Method::JsonFile {
            path: ConfigPath::AppConfig("Code/User/mcp.json"),
            key: "servers",
            shape: Shape::Stdio,
        },
        account: None,
        note: Some("VS Code asks you to trust the server the first time it starts"),
    },
];

/// A client from [`CLIENTS`].
#[derive(Clone, Copy)]
pub struct Client(&'static ClientSpec);

impl PartialEq for Client {
    fn eq(&self, other: &Self) -> bool {
        self.0.id == other.0.id
    }
}
impl Eq for Client {}

impl std::fmt::Debug for Client {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0.id)
    }
}

impl Serialize for Client {
    fn serialize<S: serde::Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        s.serialize_str(self.0.id)
    }
}

impl Client {
    pub fn all() -> impl Iterator<Item = Client> {
        CLIENTS.iter().map(Client)
    }

    pub fn parse(s: &str) -> Result<Self> {
        Self::all().find(|c| c.as_str() == s).with_context(|| {
            let ids: Vec<_> = Self::all().map(|c| c.as_str()).collect();
            format!("unknown client {s:?} ({})", ids.join(", "))
        })
    }

    /// The client a handshake name belongs to, by its aliases (case-insensitive).
    pub fn from_handshake(name: &str) -> Option<Self> {
        let name = name.trim().to_ascii_lowercase();
        Self::all().find(|c| {
            c.0.aliases.iter().any(|a| match a.strip_suffix('*') {
                Some(prefix) => name.starts_with(prefix),
                None => name == *a,
            })
        })
    }

    pub fn spec(self) -> &'static ClientSpec {
        self.0
    }

    pub fn as_str(self) -> &'static str {
        self.0.id
    }

    pub fn display_name(self) -> &'static str {
        self.0.display_name
    }

    fn cli(self) -> Option<&'static CliSpec> {
        match &self.0.method {
            Method::Cli(c) => Some(c),
            Method::JsonFile { .. } => None,
        }
    }

    /// The JSON file this client reads MCP servers from, with its key and entry shape. `None`
    /// for clients configured by command.
    pub fn config_file(self, env: &Env) -> Option<(PathBuf, &'static str, Shape)> {
        match self.0.method {
            Method::JsonFile { path, key, shape } => Some((env.resolve(path), key, shape)),
            Method::Cli(_) => None,
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

/// The server entry every standard JSON client takes.
pub fn server_entry(binary: &Path) -> Value {
    entry_for(Shape::Standard, binary)
}

fn entry_for(shape: Shape, binary: &Path) -> Value {
    let command = binary.display().to_string();
    match shape {
        Shape::Standard => json!({ "command": command, "args": ["mcp"] }),
        Shape::Stdio => json!({ "type": "stdio", "command": command, "args": ["mcp"] }),
    }
}

/// What connecting a client would do. `apply` performs exactly this.
#[derive(Clone, Debug, Serialize)]
pub struct ConnectPlan {
    /// A client id, or [`CUSTOM`].
    pub client: String,
    pub display_name: String,
    pub binary: String,
    /// `file` (merge into `config_file`) or `command` (run `command`).
    pub method: String,
    pub config_file: Option<String>,
    /// The key in `config_file` that holds the servers.
    pub config_key: Option<String>,
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
    /// The client's config file or its folder exists (or, for a command-configured client, its
    /// command was found).
    pub client_found: bool,
    pub connected: bool,
    /// The registered binary, when connected; it may be a different install than this one.
    pub binary: Option<String>,
    pub matches_this_install: bool,
    /// Who the client is signed in as; filled by [`account`] when asked for (it runs the
    /// client's CLI), `None` from [`status`].
    pub account: Option<Account>,
}

/// A client as Settings lists it.
#[derive(Clone, Debug, Serialize)]
pub struct ClientInfo {
    pub id: &'static str,
    pub display_name: &'static str,
    pub aliases: &'static [&'static str],
    pub featured: bool,
    pub checked: bool,
    pub how: &'static str,
    /// `file` or `command`.
    pub method: &'static str,
    /// The file a `file` client is configured in, on this computer.
    pub config_file: Option<String>,
    /// [`account`] can say who is signed in.
    pub has_account: bool,
}

/// Every client, for Settings.
pub fn catalog(env: &Env) -> Vec<ClientInfo> {
    Client::all()
        .map(|c| ClientInfo {
            id: c.0.id,
            display_name: c.0.display_name,
            aliases: c.0.aliases,
            featured: c.0.featured,
            checked: c.0.checked,
            how: c.0.how,
            method: if c.cli().is_some() { "command" } else { "file" },
            config_file: c.config_file(env).map(|(p, ..)| p.display().to_string()),
            has_account: c.0.account.is_some(),
        })
        .collect()
}

/// Where to look: the user's home and, on Windows, `%APPDATA%`. Tests pass a temporary home.
#[derive(Clone, Debug)]
pub struct Env {
    pub home: PathBuf,
    pub appdata: Option<PathBuf>,
    /// Client CLIs found, by program name.
    pub programs: BTreeMap<&'static str, PathBuf>,
    /// Client settings folders, by program name; a client without one here uses its default
    /// under `home`.
    pub cli_homes: BTreeMap<&'static str, PathBuf>,
}

impl Env {
    /// Nothing found yet: only the home folder (and `AppData/Roaming` under it, for Windows).
    pub fn bare(home: &Path) -> Self {
        Self {
            home: home.to_path_buf(),
            appdata: Some(home.join("AppData/Roaming")),
            programs: BTreeMap::new(),
            cli_homes: BTreeMap::new(),
        }
    }

    pub fn system() -> Result<Self> {
        // Respect the Windows process profile, including isolated CLI invocations. dirs
        // otherwise asks the Known Folder API, which ignores USERPROFILE overrides.
        let profile = if cfg!(windows) {
            std::env::var_os("USERPROFILE")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from)
        } else {
            None
        };
        let home = profile.or_else(dirs::home_dir).context("no home folder")?;
        let mut env = Self {
            appdata: std::env::var_os("APPDATA").map(PathBuf::from),
            ..Self::bare(&home)
        };
        for cli in Client::all().filter_map(Client::cli) {
            let exe = format!("{}{}", cli.program, std::env::consts::EXE_SUFFIX);
            if let Some((var, _)) = cli.home
                && let Some(dir) = std::env::var_os(var).filter(|v| !v.is_empty())
            {
                env.cli_homes.insert(cli.program, PathBuf::from(dir));
            }
            let own = || {
                let p = env.cli_home(cli)?.join("bin").join(&exe);
                p.is_file().then_some(p)
            };
            let found = find_in_path(&exe).or_else(|| if cli.bin_in_home { own() } else { None });
            if let Some(path) = found {
                env.programs.insert(cli.program, path);
            }
        }
        Ok(env)
    }

    fn resolve(&self, path: ConfigPath) -> PathBuf {
        match path {
            ConfigPath::Home(rel) => self.home.join(rel),
            ConfigPath::AppConfig(rel) => {
                let base = if cfg!(target_os = "macos") {
                    self.home.join("Library/Application Support")
                } else if cfg!(windows) {
                    self.appdata
                        .clone()
                        .unwrap_or_else(|| self.home.join("AppData/Roaming"))
                } else {
                    self.home.join(".config")
                };
                base.join(rel)
            }
        }
    }

    fn cli_home(&self, cli: &CliSpec) -> Option<PathBuf> {
        let (_, default) = cli.home?;
        Some(
            self.cli_homes
                .get(cli.program)
                .cloned()
                .unwrap_or_else(|| self.home.join(default)),
        )
    }

    /// The CLI that configures a command-configured client.
    fn program(&self, cli: &CliSpec) -> Option<&PathBuf> {
        self.programs.get(cli.program)
    }

    /// The invectus entry a command-configured client has now, read from its own files
    /// (`~/.claude.json`, `$CODEX_HOME/config.toml`, `$GROK_HOME/config.toml`) without running
    /// it.
    fn registered(&self, cli: &CliSpec) -> Option<Value> {
        match cli.registered {
            Registered::Json { file, key } => read_config(&self.home.join(file))
                .ok()
                .and_then(|c| c[key].get(SERVER_NAME).cloned()),
            Registered::Toml {
                file,
                table,
                honour_enabled,
            } => {
                let raw = std::fs::read_to_string(self.cli_home(cli)?.join(file)).ok()?;
                let config: toml::Table = raw.parse().ok()?;
                let mut entry = config.get(table)?.get(SERVER_NAME)?.clone();
                if honour_enabled
                    && entry
                        .as_table_mut()?
                        .remove("enabled")
                        .is_some_and(|e| e.as_bool() == Some(false))
                {
                    return None;
                }
                serde_json::to_value(entry).ok()
            }
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
                    "{} is not valid JSON (comments are not supported); fix or move it before \
                     connecting",
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

/// The program an entry runs: `command` as a string, or the first item of a `command` list.
fn entry_command(entry: &Value) -> Option<String> {
    match &entry["command"] {
        Value::String(s) => Some(s.clone()),
        Value::Array(a) => a.first()?.as_str().map(String::from),
        _ => None,
    }
}

/// `<cli> mcp add …` without the program: Claude Code and Grok Build register for the user (all
/// projects); Codex's servers are always global. Codex and Grok replace an existing entry.
fn add_args(cli: &CliSpec, binary: &Path) -> Vec<String> {
    ["mcp", "add"]
        .iter()
        .chain(cli.scope)
        .chain(&[SERVER_NAME, "--"])
        .map(|s| s.to_string())
        .chain([binary.display().to_string(), "mcp".into()])
        .collect()
}

fn remove_args(cli: &CliSpec) -> Vec<&'static str> {
    ["mcp", "remove"]
        .iter()
        .chain(cli.scope)
        .chain(&[SERVER_NAME])
        .copied()
        .collect()
}

/// Plan connecting `client` to `binary`. Reads only.
pub fn plan(client: Client, binary: &Path, env: &Env) -> Result<ConnectPlan> {
    match client.0.method {
        Method::JsonFile { path, key, shape } => {
            let mut plan = plan_file(
                client.as_str(),
                client.display_name(),
                &env.resolve(path),
                key,
                shape,
                binary,
            )?;
            plan.notes.extend(client.0.note.map(String::from));
            Ok(plan)
        }
        Method::Cli(ref cli) => {
            let entry = server_entry(binary);
            let mut notes = Vec::new();
            let name = cli.program;
            let args = add_args(cli, binary);
            let manual = format!("{name} {}", args.join(" "));
            let command = env.program(cli).map(|c| {
                std::iter::once(c.display().to_string())
                    .chain(args)
                    .collect::<Vec<_>>()
            });
            if command.is_none() {
                notes.push(format!(
                    "the {name} command was not found on PATH; run the command shown yourself"
                ));
            }
            let current = if cli.replaces {
                env.registered(cli)
            } else {
                None
            };
            if current.is_some() && current.as_ref() != Some(&entry) {
                notes.push("replaces an existing invectus entry".into());
            }
            notes.extend(client.0.note.map(String::from));
            Ok(ConnectPlan {
                client: client.as_str().into(),
                display_name: client.display_name().into(),
                binary: binary.display().to_string(),
                method: "command".into(),
                config_file: None,
                config_key: None,
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

/// Check a custom client's file before planning or removing: an absolute path to a `.json`
/// file, and a plain key.
fn check_custom(file: &Path, key: &str) -> Result<()> {
    if !file.is_absolute() {
        bail!("give the full path to the client's configuration file");
    }
    if file
        .extension()
        .is_none_or(|e| !e.eq_ignore_ascii_case("json"))
    {
        bail!("the configuration file must be a .json file");
    }
    if key.is_empty()
        || !key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
    {
        bail!("the key must be a plain name such as mcpServers");
    }
    Ok(())
}

/// Plan connecting a client that is not listed: merge the standard entry under `key` in its
/// JSON configuration file. Reads only.
pub fn plan_custom(file: &Path, key: &str, binary: &Path) -> Result<ConnectPlan> {
    check_custom(file, key)?;
    let mut plan = plan_file(CUSTOM, "Other client", file, key, Shape::Standard, binary)?;
    plan.notes
        .push("check the client's documentation for where it reads MCP servers".into());
    Ok(plan)
}

fn plan_file(
    id: &str,
    display_name: &str,
    path: &Path,
    key: &str,
    shape: Shape,
    binary: &Path,
) -> Result<ConnectPlan> {
    let entry = entry_for(shape, binary);
    let manual_json = serde_json::to_string_pretty(&json!({ key: { SERVER_NAME: entry } }))?;
    let mut notes = Vec::new();
    let config = read_config(path)?;
    let current = config[key].get(SERVER_NAME).cloned();
    if current.is_some() && current.as_ref() != Some(&entry) {
        notes.push("replaces an existing invectus entry".into());
    }
    if !path.exists() {
        notes.push(format!("creates {}", path.display()));
    }
    Ok(ConnectPlan {
        client: id.into(),
        display_name: display_name.into(),
        binary: binary.display().to_string(),
        method: "file".into(),
        config_file: Some(path.display().to_string()),
        config_key: Some(key.into()),
        up_to_date: current.as_ref() == Some(&entry),
        current,
        entry,
        command: None,
        manual: format!("Add to {}:\n{manual_json}", path.display()),
        notes,
    })
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
            let key = plan.config_key.as_deref().unwrap_or("mcpServers");
            let mut config = read_config(&path)?;
            let servers = config
                .as_object_mut()
                .expect("read_config returns an object")
                .entry(key)
                .or_insert_with(|| json!({}));
            let Some(servers) = servers.as_object_mut() else {
                bail!("`{key}` in {} is not an object", path.display());
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

/// Remove the invectus entry from a client (Claude Code, Codex and Grok: their `mcp remove`).
pub fn disconnect(client: Client, env: &Env) -> Result<String> {
    match client.0.method {
        Method::JsonFile { path, key, .. } => {
            disconnect_file(client.display_name(), &env.resolve(path), key)
        }
        Method::Cli(ref cli) => {
            let name = cli.program;
            let args = remove_args(cli);
            let program = env.program(cli).with_context(|| {
                format!(
                    "the {name} command was not found; run `{name} {}`",
                    args.join(" ")
                )
            })?;
            let out = std::process::Command::new(program).args(&args).output()?;
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

/// Remove the invectus entry from a client that is not listed.
pub fn disconnect_custom(file: &Path, key: &str) -> Result<String> {
    check_custom(file, key)?;
    disconnect_file("Other client", file, key)
}

fn disconnect_file(display_name: &str, path: &Path, key: &str) -> Result<String> {
    let mut config = read_config(path)?;
    let removed = config
        .get_mut(key)
        .and_then(Value::as_object_mut)
        .and_then(|s| s.remove(SERVER_NAME))
        .is_some();
    if !removed {
        return Ok(format!("{display_name} was not connected"));
    }
    std::fs::copy(path, backup_path(path))?;
    invectus_core::project::write_json_atomic(path, &config)?;
    Ok(format!("disconnected {display_name}"))
}

/// Whether each client is connected, and to which binary. Reads only; the command-configured
/// clients are read from their own config files without running them.
pub fn status(binary: Option<&Path>, env: &Env) -> Vec<ConnectStatus> {
    Client::all()
        .map(|client| {
            let (found, entry) = match client.0.method {
                Method::JsonFile { path, key, .. } => {
                    let path = env.resolve(path);
                    (
                        path.exists() || path.parent().is_some_and(Path::is_dir),
                        read_config(&path)
                            .ok()
                            .and_then(|c| c[key].get(SERVER_NAME).cloned()),
                    )
                }
                Method::Cli(ref cli) => (env.program(cli).is_some(), env.registered(cli)),
            };
            let registered = entry.as_ref().and_then(entry_command);
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

/// The CLIs answer in about a second; a stuck one is abandoned.
const ACCOUNT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

/// Ask a client's CLI who is signed in: `claude auth status` (JSON), `codex login status` or
/// `grok models` (Grok Build has no status command; its model list says when it is signed out).
/// `None` for clients without a CLI to ask (Claude Desktop, Cursor). Not run while offline:
/// whether the CLIs touch the network for this is theirs to decide.
pub fn account(client: Client, env: &Env) -> Option<Account> {
    let probe = client.spec().account?;
    let cli = client.cli()?;
    let args = probe.args();
    let name = cli.program;
    let Some(program) = env.program(cli) else {
        return Some(Account::unknown(format!(
            "the {name} command was not found on PATH"
        )));
    };
    if invectus_core::net::offline() {
        return Some(Account::unknown("not checked while offline"));
    }
    Some(match run_with_timeout(program, args, ACCOUNT_TIMEOUT) {
        Ok(out) => probe.read(&out),
        Err(e) => Account::unknown(format!("`{name} {}` failed: {e:#}", args.join(" "))),
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

/// `grok models` prints "You are not authenticated." above the model list when signed out.
/// Otherwise it lists the models without naming the plan, so a signed-in account has no label.
fn grok_account(text: &str, success: bool) -> Account {
    let t = text.to_ascii_lowercase();
    if t.contains("not authenticated") || t.contains("not signed in") {
        Account {
            message: Some("run `grok login`".into()),
            ..Account::new("signed-out", None, None)
        }
    } else if success && t.contains("available models") {
        Account::new("signed-in", None, None)
    } else {
        Account::unknown("`grok models` gave no readable answer")
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
        let mut e = Env::bare(home);
        if let Some(c) = claude {
            e.programs.insert("claude", c);
        }
        e
    }

    fn client(id: &str) -> Client {
        Client::parse(id).unwrap()
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
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut e = env(home.path(), None);
        let p = plan(client("codex"), &bin, &e).unwrap();
        assert_eq!(p.method, "command");
        assert_eq!(
            p.manual,
            format!("codex mcp add invectus -- {} mcp", bin.display())
        );
        assert!(p.command.is_none() && !p.up_to_date);
        assert!(!status(Some(&bin), &e)[2].client_found);
        // Status and plans read the entry Codex wrote, without running it.
        let codex_home = home.path().join(".codex");
        std::fs::create_dir_all(&codex_home).unwrap();
        std::fs::write(
            codex_home.join("config.toml"),
            format!(
                // `{:?}` escapes Windows backslashes as a TOML basic string needs.
                "model = \"x\"\n[mcp_servers.invectus]\ncommand = {:?}\nargs = [\"mcp\"]\n",
                bin.display().to_string()
            ),
        )
        .unwrap();
        let s = status(Some(&bin), &e);
        assert_eq!(s[2].client, client("codex"));
        assert!(s[2].connected && s[2].matches_this_install);
        assert!(plan(client("codex"), &bin, &e).unwrap().up_to_date);
        let other = plan(client("codex"), Path::new("/elsewhere/invectus"), &e).unwrap();
        assert!(!other.up_to_date && other.notes.iter().any(|n| n.contains("replaces")));
        #[cfg(unix)]
        {
            e.programs
                .insert("codex", fake_cli(home.path(), "codex", "", 0));
            apply(&plan(client("codex"), Path::new("/elsewhere/invectus"), &e).unwrap()).unwrap();
            disconnect(client("codex"), &e).unwrap();
            let log = std::fs::read_to_string(home.path().join("codex.log")).unwrap();
            assert_eq!(
                log,
                "mcp add invectus -- /elsewhere/invectus mcp\nmcp remove invectus\n"
            );
        }
    }

    #[test]
    fn grok_uses_its_own_command_and_reads_its_config_toml() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("invectus");
        #[cfg_attr(not(unix), allow(unused_mut))]
        let mut e = env(home.path(), None);
        let p = plan(client("grok"), &bin, &e).unwrap();
        assert_eq!(p.method, "command");
        assert_eq!(
            p.manual,
            format!(
                "grok mcp add --scope user invectus -- {} mcp",
                bin.display()
            )
        );
        assert!(p.command.is_none() && !p.up_to_date);
        assert!(!status(Some(&bin), &e)[4].client_found);
        // Grok also loads `~/.claude.json`; only its own config counts as connected.
        std::fs::write(
            home.path().join(".claude.json"),
            json!({ "mcpServers": { "invectus": server_entry(&bin) } }).to_string(),
        )
        .unwrap();
        assert!(!status(Some(&bin), &e)[4].connected);
        // The entry as `grok mcp add` writes it, with `enabled`.
        let grok_home = home.path().join(".grok");
        std::fs::create_dir_all(&grok_home).unwrap();
        let write = |enabled: bool| {
            std::fs::write(
                grok_home.join("config.toml"),
                format!(
                    "[cli]\ninstaller = \"internal\"\n\n[mcp_servers.invectus]\n\
                     command = {:?}\nargs = [\"mcp\"]\nenabled = {enabled}\n",
                    bin.display().to_string()
                ),
            )
            .unwrap()
        };
        write(true);
        let s = status(Some(&bin), &e);
        assert_eq!(s[4].client, client("grok"));
        assert!(s[4].connected && s[4].matches_this_install);
        assert!(plan(client("grok"), &bin, &e).unwrap().up_to_date);
        let other = plan(client("grok"), Path::new("/elsewhere/invectus"), &e).unwrap();
        assert!(!other.up_to_date && other.notes.iter().any(|n| n.contains("replaces")));
        // Disabled in Grok: not connected, and adding again enables it.
        write(false);
        assert!(!status(Some(&bin), &e)[4].connected);
        assert!(!plan(client("grok"), &bin, &e).unwrap().up_to_date);
        #[cfg(unix)]
        {
            e.programs
                .insert("grok", fake_cli(home.path(), "grok", "", 0));
            apply(&plan(client("grok"), Path::new("/elsewhere/invectus"), &e).unwrap()).unwrap();
            disconnect(client("grok"), &e).unwrap();
            let log = std::fs::read_to_string(home.path().join("grok.log")).unwrap();
            assert_eq!(
                log,
                "mcp add --scope user invectus -- /elsewhere/invectus mcp\n\
                 mcp remove --scope user invectus\n"
            );
        }
    }

    #[test]
    fn grok_account_reads_its_model_list() {
        // Grok 1.0.77, signed out.
        let out = grok_account(
            "You are not authenticated.\n\nDefault model: grok-4.6\n\nAvailable models:\n  \
             * grok-4.6 (default)\n  - grok-4.5\n",
            true,
        );
        assert_eq!(out.status, "signed-out");
        assert!(out.message.unwrap().contains("grok login"));
        let signed_in = grok_account(
            "Default model: grok-4.6\n\nAvailable models:\n  * grok-4.6 (default)\n",
            true,
        );
        assert_eq!(signed_in.status, "signed-in");
        assert_eq!(signed_in.label, None);
        assert_eq!(grok_account("", true).status, "unknown");
        assert_eq!(grok_account("Available models:\n", false).status, "unknown");
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
        assert_eq!(account(client("cursor"), &e), None);
        assert_eq!(account(client("codex"), &e).unwrap().status, "unknown");
        e.programs.insert(
            "claude",
            fake_cli(
                home.path(),
                "claude",
                r#"{"loggedIn": true, "authMethod": "claude.ai", "subscriptionType": "max"}"#,
                0,
            ),
        );
        e.programs
            .insert("codex", fake_cli(home.path(), "codex", "Not logged in", 1));
        let claude = account(client("claude-code"), &e).unwrap();
        assert_eq!(claude.label.as_deref(), Some("Claude Max subscription"));
        assert_eq!(account(client("codex"), &e).unwrap().status, "signed-out");
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

        let p = plan(client("cursor"), &bin, &e).unwrap();
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

        let again = plan(client("cursor"), &bin, &e).unwrap();
        assert!(again.up_to_date);
        let s = status(Some(&bin), &e);
        let cs = s.iter().find(|s| s.client == client("cursor")).unwrap();
        assert!(cs.connected && cs.matches_this_install);
        // Another install's binary replaces the entry, with a note.
        let other = plan(client("cursor"), Path::new("/elsewhere/invectus"), &e).unwrap();
        assert!(other.notes.iter().any(|n| n.contains("replaces")));

        disconnect(client("cursor"), &e).unwrap();
        let c: Value = serde_json::from_str(&std::fs::read_to_string(&cursor).unwrap()).unwrap();
        assert!(c["mcpServers"].get("invectus").is_none());
        assert_eq!(c["mcpServers"]["other"]["command"], "x");
    }

    #[test]
    fn missing_files_are_created_and_broken_ones_refused() {
        let home = tempfile::tempdir().unwrap();
        let e = env(home.path(), None);
        let bin = home.path().join("invectus");
        let p = plan(client("claude-desktop"), &bin, &e).unwrap();
        assert!(p.notes.iter().any(|n| n.contains("creates")));
        apply(&p).unwrap();
        let path = PathBuf::from(p.config_file.unwrap());
        assert!(path.exists());
        std::fs::write(&path, "{ not json").unwrap();
        let err = plan(client("claude-desktop"), &bin, &e)
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
        let p = plan(client("claude-code"), &bin, &env(home.path(), None)).unwrap();
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
            let p = plan(client("claude-code"), &bin, &env(home.path(), Some(fake))).unwrap();
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
    fn catalog_ids_are_unique_and_parse() {
        let ids: Vec<_> = Client::all().map(Client::as_str).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), ids.len());
        for id in &ids {
            assert_eq!(client(id).as_str(), *id);
        }
        // The first five keep their order (Settings and older status consumers).
        assert_eq!(
            &ids[..5],
            ["claude-code", "claude-desktop", "codex", "cursor", "grok"]
        );
        assert!(Client::parse(CUSTOM).is_err());
        let err = Client::parse("nope").unwrap_err().to_string();
        assert!(err.contains("gemini-cli"), "{err}");
        let home = tempfile::tempdir().unwrap();
        let cat = catalog(&Env::bare(home.path()));
        assert_eq!(cat.len(), CLIENTS.len());
        let vscode = cat.iter().find(|i| i.id == "vscode").unwrap();
        assert_eq!(vscode.method, "file");
        assert!(vscode.config_file.as_deref().unwrap().ends_with("mcp.json"));
        assert!(cat.iter().find(|i| i.id == "codex").unwrap().has_account);
    }

    #[test]
    fn handshake_names_map_to_clients() {
        let id = |n: &str| Client::from_handshake(n).map(Client::as_str);
        assert_eq!(id(" codex-mcp-client "), Some("codex"));
        assert_eq!(id("grok-shell-invectus"), Some("grok"));
        assert_eq!(id("claude-code"), Some("claude-code"));
        assert_eq!(id("Visual Studio Code"), Some("vscode"));
        assert_eq!(id("something-else"), None);
    }

    #[test]
    fn every_json_client_merges_under_its_own_key() {
        let home = tempfile::tempdir().unwrap();
        let e = env(home.path(), None);
        let bin = home.path().join("bin/invectus");
        for client in Client::all() {
            let Some((path, key, shape)) = client.config_file(&e) else {
                continue;
            };
            assert!(
                path.starts_with(home.path()),
                "{client:?}: {}",
                path.display()
            );
            assert!(
                !status(Some(&bin), &e)
                    .iter()
                    .any(|s| s.client == client && s.connected)
            );
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(
                &path,
                json!({ key: { "other": { "command": "x" } }, "k": 1 }).to_string(),
            )
            .unwrap();
            let p = plan(client, &bin, &e).unwrap();
            assert_eq!(p.config_key.as_deref(), Some(key));
            assert_eq!(p.entry, entry_for(shape, &bin));
            apply(&p).unwrap();
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert_eq!(v[key]["invectus"], entry_for(shape, &bin), "{client:?}");
            assert_eq!(v[key]["other"]["command"], "x");
            assert_eq!(v["k"], 1);
            let s = status(Some(&bin), &e);
            let s = s.iter().find(|s| s.client == client).unwrap();
            assert!(
                s.client_found && s.connected && s.matches_this_install,
                "{client:?}"
            );
            assert!(plan(client, &bin, &e).unwrap().up_to_date);
            disconnect(client, &e).unwrap();
            let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
            assert!(v[key].get("invectus").is_none());
        }
        // VS Code takes `servers` and a typed entry.
        let (_, key, shape) = client("vscode").config_file(&e).unwrap();
        assert_eq!((key, shape), ("servers", Shape::Stdio));
        assert_eq!(entry_for(shape, &bin)["type"], "stdio");
    }

    #[test]
    fn custom_clients_take_an_absolute_json_file_and_a_plain_key() {
        let home = tempfile::tempdir().unwrap();
        let bin = home.path().join("invectus");
        let file = home.path().join("tool/config.json");
        assert!(plan_custom(Path::new("config.json"), "mcpServers", &bin).is_err());
        assert!(plan_custom(&home.path().join("x.toml"), "mcpServers", &bin).is_err());
        assert!(plan_custom(&file, "a.b", &bin).is_err());
        assert!(plan_custom(&file, "", &bin).is_err());
        let p = plan_custom(&file, "mcp_servers", &bin).unwrap();
        assert_eq!(p.client, CUSTOM);
        assert!(p.notes.iter().any(|n| n.contains("creates")));
        apply(&p).unwrap();
        let v: Value = serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(v["mcp_servers"]["invectus"], server_entry(&bin));
        assert!(plan_custom(&file, "mcp_servers", &bin).unwrap().up_to_date);
        assert_eq!(
            disconnect_custom(&file, "mcp_servers").unwrap(),
            "disconnected Other client"
        );
        assert!(
            disconnect_custom(&file, "mcp_servers")
                .unwrap()
                .contains("was not connected")
        );
    }

    #[test]
    fn entry_command_reads_strings_and_lists() {
        assert_eq!(
            entry_command(&json!({"command": "/a"})).as_deref(),
            Some("/a")
        );
        assert_eq!(
            entry_command(&json!({"command": ["/b", "mcp"]})).as_deref(),
            Some("/b")
        );
        assert_eq!(entry_command(&json!({})), None);
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
