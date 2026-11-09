# Plan: support many agent clients in Settings

Status: proposal (2026-11-08). Not built. Source studied: T3 Code
(`github.com/pingdotgg/t3code`, commit `0770446`, MIT).

## 1. How T3 Code does it

T3 Code is an agent **host**: it launches agents and drives their sessions. It lists 11 built-in
providers plus a long tail of about 45 agents from the public ACP Registry. Four ideas carry it.

1. **One package per provider, two halves.**
   - *Client definition* (`packages/provider-*/src/client.ts`, `defineProviderClient`): id
     (`driverKind`), label, SVG icon (light/dark fills), `featured`, `badgeLabel` ("Early
     Access"), model-family prefixes, ACP Registry aliases, environment fields (e.g. an API key,
     `sensitive: true`), and a settings schema.
   - *Server driver* (`ProviderDriver`, `provider-core/src/server/driver.ts`): `configSchema`,
     `defaultConfig`, and `create()`, which returns an instance with a status snapshot (installed,
     version, auth, message), models, a session adapter, and optional auth and usage readers.
     Failures become an "unavailable" snapshot and never crash the list.
2. **Two flat registries.** `apps/server/src/provider/builtInDrivers.ts` and
   `packages/client-runtime/src/providerClients.ts` are plain arrays. Adding a provider means
   adding a package and one line in each array. Nothing else in the app names providers.
3. **Settings forms come from the schema.** Each settings field carries a `providerSettingsForm`
   annotation (title, description, placeholder, `control: "select"`, `hidden`,
   `clearWhenEmpty`, secret). One generic renderer (`ProviderSettingsForm.tsx`) draws every
   provider's form. `ProviderInstanceCard` and `AddProviderInstanceDialog` are the same for every
   provider. Instances are a record (`providerInstances: { [instanceId]: { driver, enabled,
   config, environment } }`), so one provider can have several instances (for example two Codex
   accounts).
4. **A generic driver for the long tail.** `provider-acp-registry` fetches
   `cdn.agentclientprotocol.com/registry/v1/latest/registry.json`, caches it on disk, and installs
   each agent from its listed distribution (`binary`, `npx` or `uvx`). Users can also choose
   "Local command". A search step in the add dialog picks the agent. Known agents keep their own
   drivers, and registry ids are mapped back to them through `acpRegistryAgentIds`.

## 2. How this maps to inVectus

The direction is reversed. In inVectus the agent is the host: Claude Code, Codex and the others
start `invectus mcp` and call our tools (SPEC §7.8). Our job for each client is to **register an
MCP server and read its status**, not to drive sessions. So T3 Code's session adapters, model
catalogs and ACP runtime do not apply. Its structure does.

Today one client is described in five places:

| Place | What it holds |
|---|---|
| `crates/orchestration/src/connect.rs` | `Client` enum with 10+ `match` blocks: name, config file, CLI, add/remove args, registered-entry reader, account probe |
| `src/api.ts` | `AgentClient` union type |
| `src/screens/ConnectAgents.tsx` | `CLIENTS` array (name, "how" text), `HAS_ACCOUNT` |
| `src/agentIdentity.ts` | `clientName` aliases for MCP handshake names |
| `src/AgentIcon.tsx` | `markFor` regexes and marks |

Adding a sixth client touches all five. T3 Code's pattern brings that down to one entry.

**Conclusion: plausible and cheap for the parts that fit.** Most MCP clients use one of three
registration styles, so a data-driven catalog covers the long tail without per-client code.

## 3. Plan

### Phase 1: one client catalog (refactor, no new behaviour)

1. In `invectus-orchestration`, replace the `Client` enum's `match` arms with a static
   `ClientSpec` table. Each entry has:
   - `id`, `display_name`, `aliases` (MCP `clientInfo.name` values, e.g. `codex-mcp-client`),
     `featured`, `badge` (`None` | "Unverified")
   - `method`: one of
     - `JsonFile { path: PathSpec, key: "mcpServers" | "servers" | "context_servers" | …, entry_shape }`
     - `TomlFile { path: PathSpec, table }`, read only, for status
     - `Cli { exe, home_var, add_args, remove_args, registered: ConfigReader }`
   - `account: Option<AccountProbe { args, parser }>`; parsers stay as Rust functions selected
     by name
   - `PathSpec` resolves per OS (`home`, `appdata`, XDG), the way `config_file` does now.
2. `plan`, `apply`, `disconnect`, `status` and `account` become one generic code path over the
   spec. The behaviour stays the same: show the plan first, merge, back up, write atomically,
   never touch tokens. Port the existing tests table-driven: one test per method kind plus the
   current five clients.
3. Add a Tauri command and CLI command `connect clients` that return the catalog as JSON (id,
   name, method summary, featured, badge, has_account, aliases). The frontend reads that instead
   of hardcoding:
   - `AgentClient` becomes `string`, checked by the backend.
   - `CLIENTS`, `HAS_ACCOUNT` and `clientName` aliases come from the catalog.
   - Icons stay in the frontend (`AgentIcon.tsx`), keyed by catalog `id`, with the `Bot` fallback.
4. Exit check: the five clients behave exactly as today; `cargo test --workspace --release`,
   clippy and `npm run build` pass.

### Phase 2: more clients, and a custom entry

1. Add clients as catalog entries only. Candidates: Gemini CLI, VS Code (Copilot agent mode),
   Windsurf, Zed, opencode, Kiro, Cline, LM Studio. **Check each client's current config path,
   key and entry shape against its own docs before adding it.** Mark it `badge: "Unverified"`
   until a session has called `invectus mcp` through it on a real machine, as A6 already does for
   Desktop and Cursor.
2. Add a **custom client** entry for the long tail, in place of T3 Code's ACP Registry. The user
   gives a JSON config file path and a key, and we merge the same entry. Alternatively, we show
   the generic `{ "command": …, "args": ["mcp"] }` snippet to copy. No catalog download is
   needed, so no network use is added.
3. Settings UI, modelled on T3 Code's cards:
   - Featured clients show first (Claude Code, Codex, Claude Desktop, Cursor). The rest go in a
     collapsed "More clients" list with a filter box, shown once the list passes about 8 entries.
   - Detected clients (`client_found`) sort to the top.
   - One generic `ClientRow` component draws every row from the catalog.
   - Account checks run only for entries with `has_account`, as now.

### Phase 3 (post-launch, owner 2026-11-09): host agents through ACP

Deferred until after launch. If it is built, build only the narrow version:

- launch agents that are already installed and signed in; never download agents from the ACP
  Registry and never install them through `npx`/`uvx`;
- offer the agent no file or terminal access, so it works only through `invectus mcp` tools;
- deny all other permission requests by default;
- run the agent in the project folder only;
- refuse to start under `--offline`.

Why: downloading agents brings supply-chain risk, granting file or terminal access makes
inVectus responsible for what the agent changes, and prompt injection from collection
documents is more dangerous when the agent has broad permissions.

The rest of this section is the original proposal.

T3 Code's ACP route would let inVectus start an agent itself. ACP's `session/new` accepts
`mcpServers`, so we could attach `invectus mcp` and run "describe these gaps" or "name these
regions" jobs from the app. This would be a new subsystem: a Rust ACP client, process
lifecycle, permission prompts, and streaming into the UI. It also conflicts with the current
spec:

- SPEC §7.8 says agents call inVectus; inVectus does not launch them.
- Fetching the ACP Registry, or installing agents through `npx`/`uvx`, uses the network and
  downloads code. Both need an explicit switch, must be refused under `--offline`, and need the
  owner's approval for each download.

Do not start Phase 3 without an owner decision. If approved, record it under "Decisions and spec
conflicts" in PROGRESS.md.

## 4. What not to copy

- Model catalogs, usage and limit readers, and session adapters (T3 Code drives sessions; we
  don't).
- Effect-Schema annotations as the form engine. Our clients have no settings beyond the
  registration itself, so a plain catalog is enough. Revisit only if clients gain options (for
  example per-client environment variables such as `INVECTUS_OFFLINE=1` in the entry).
- Storing secrets. inVectus never stores a client's sign-in.

## 5. Effort and risk

- Phase 1: about 1–2 days. The risk is behaviour drift in `connect.rs`; the existing tests and
  the plan/apply snapshot guard against it.
- Phase 2: about half a day per client, mostly checking the client's config format. The custom
  entry takes about half a day.
- Support claims: list only clients checked on a real machine as "checked". Windows and Linux
  config paths stay unverified until a CI run or a manual run on that OS covers them.
