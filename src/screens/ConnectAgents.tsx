// Agents page (target product §5.1, A6). The clients come from the backend's
// catalog (`connect_clients`): featured and detected clients
// first, the rest under "More clients", and an "Other client" entry for any client that reads a
// JSON file. Connecting shows the exact change first (the file and entry, or the command) and
// makes it only after "Make this change". Below, the agents' recent calls through this app.
import { useEffect, useState } from "react";
import { api, inTauri, type AgentAccount, type ClientInfo, type ConnectPlan, type ConnectStatus, type CustomClient } from "../api";
import { ErrorLine, useAction } from "../App";
import { ago, describe, useAgentActivity } from "../agents";
import AgentIcon from "../AgentIcon";
import { clientName, registerClients } from "../agentIdentity";
/** The id the backend takes for a client that is not listed. */
const CUSTOM_ID = "custom";

function accountText(a: AgentAccount | null | undefined): string | null {
  if (!a) return null;
  if (a.status === "signed-in") return `Signed in${a.label ? ` with ${a.label}` : ""}${a.email ? ` (${a.email})` : ""}`;
  if (a.status === "signed-out") return `Not signed in: ${a.message ?? ""}`;
  return `Sign-in not checked: ${a.message ?? ""}`;
}

function statusText(s: ConnectStatus | undefined): string {
  if (!s) return "Checking…";
  if (s.connected && s.matches_this_install) return "Connected";
  if (s.connected) return "Connected to another install";
  return s.client_found ? "Not connected" : "Not found on this computer";
}

export default function ConnectAgents() {
  const [clients, setClients] = useState<ClientInfo[]>([]);
  const [status, setStatus] = useState<ConnectStatus[]>([]);
  const [plan, setPlan] = useState<ConnectPlan>();
  const [planCustom, setPlanCustom] = useState<CustomClient>();
  const [custom, setCustom] = useState<CustomClient>({ file: "", key: "mcpServers" });
  const [done, setDone] = useState<string>();
  const [copied, setCopied] = useState(false);
  const [accounts, setAccounts] = useState<Record<string, AgentAccount | null>>({});
  const action = useAction();
  const activity = useAgentActivity();
  const reload = () => { if (inTauri) api.connectStatus().then(setStatus).catch(() => undefined); };
  useEffect(() => {
    if (!inTauri) return;
    reload();
    api.connectClients().then((list) => {
      registerClients(list);
      setClients(list);
      // Each CLI takes about a second to answer, so accounts load after the connection status.
      for (const c of list.filter((c) => c.has_account)) {
        api.connectAccount(c.id).then((a) => setAccounts((m) => ({ ...m, [c.id]: a }))).catch(() => undefined);
      }
    }).catch(() => undefined);
  }, []);

  const open = (client: string, c?: CustomClient) => action.run(async () => {
    setDone(undefined); setCopied(false); setPlanCustom(c); setPlan(await api.connectPlan(client, c));
  });
  const apply = () => action.run(async () => { setDone(await api.connectApply(plan!.client, planCustom)); setPlan(undefined); reload(); });
  const remove = (client: string, c?: CustomClient) => action.run(async () => { setDone(await api.connectRemove(client, c)); setPlan(undefined); reload(); });
  const copy = async (text: string) => { try { await navigator.clipboard.writeText(text); setCopied(true); } catch { /* clipboard unavailable */ } };
  const canApply = plan && !plan.up_to_date && (plan.method === "file" || !!plan.command);
  const customReady = custom.file.trim() !== "" && custom.key.trim() !== "";
  const customInput = { file: custom.file.trim(), key: custom.key.trim() };

  const statusOf = (id: string) => status.find((x) => x.client === id);
  // Featured clients, and any other client found on this computer or connected, show first.
  const shown = (c: ClientInfo) => { const s = statusOf(c.id); return c.featured || !!s?.client_found || !!s?.connected; };
  const main = clients.filter(shown);
  const more = clients.filter((c) => !shown(c));
  const how = plan?.client === CUSTOM_ID
    ? "Adds inVectus to the file you named."
    : clients.find((c) => c.id === plan?.client)?.how;

  const card = (c: ClientInfo) => {
    const s = statusOf(c.id);
    const connected = !!(s?.connected && s.matches_this_install);
    const account = s?.client_found ? accountText(accounts[c.id]) : null;
    return <li key={c.id} className="integration-card" data-connected={connected}>
      <div className="integration-card-top">
        <span className="integration-mark"><AgentIcon client={c.id} size={22} /></span>
        {connected
          ? <span className="integration-badge">Connected</span>
          : <button type="button" className="integration-connect" disabled={action.busy} onClick={() => open(c.id)}>{s?.connected ? "Use this install" : "Connect"}</button>}
      </div>
      <div className="integration-name">{c.display_name}{!c.checked && <span className="tag connect-unverified" title="No session has been checked through this client yet">Unverified</span>}</div>
      <p className="integration-desc">{statusText(s)}{s?.connected && !s.matches_this_install && s.binary ? `: ${s.binary}` : ""}{account ? `. ${account}` : ""}</p>
      {s?.connected && <button type="button" className="text-btn integration-disconnect" disabled={action.busy} onClick={() => remove(c.id)}>Disconnect</button>}
    </li>;
  };

  return <div className="integrations-agents">
    <ul className="integration-grid" data-code="AG" aria-label="AI agent clients">{main.map(card)}</ul>
    <details className="connect-more">
      <summary>More clients{more.length ? ` (${more.length})` : ""}</summary>
      <ul className="integration-grid" data-code="AG" aria-label="More AI agent clients">{more.map(card)}</ul>
      <div className="integration-card integration-card-wide">
        <div className="integration-card-top"><span className="integration-mark"><AgentIcon client={null} size={22} /></span></div>
        <div className="integration-name">Other client</div>
        <p className="integration-desc">Any client that reads MCP servers from a JSON file. The entry is <code>{"{ \"command\": …, \"args\": [\"mcp\"] }"}</code> under the key you name.</p>
        <div className="row wrap connect-custom">
          <input aria-label="Client's configuration file (full path)" placeholder="/full/path/to/mcp.json" value={custom.file} onChange={(e) => setCustom({ ...custom, file: e.target.value })} />
          <input aria-label="Key holding its servers" placeholder="mcpServers" value={custom.key} onChange={(e) => setCustom({ ...custom, key: e.target.value })} />
          <button type="button" className="btn small" disabled={action.busy || !customReady} onClick={() => open(CUSTOM_ID, customInput)}>Connect…</button>
          <button type="button" className="text-btn" disabled={action.busy || !customReady} onClick={() => remove(CUSTOM_ID, customInput)}>Disconnect</button>
        </div>
      </div>
    </details>
    {plan && <div className="integration-modal-backdrop" onClick={(e) => { if (e.target === e.currentTarget) setPlan(undefined); }}
      onKeyDown={(e) => { if (e.key === "Escape") setPlan(undefined); }}>
      <div className="integration-modal connect-plan stack gap-s" role="dialog" aria-modal="true" aria-label={`Connect ${plan.display_name}`}>
        <div className="integration-modal-head"><span className="integration-mark"><AgentIcon client={plan.client} size={22} /></span><h3>Connect {plan.display_name}</h3></div>
        <div className="muted small">{how} Nothing has changed yet.</div>
        {plan.config_file && <div className="small">File: <code className="path">{plan.config_file}</code></div>}
        <pre className="connect-code">{plan.method === "file" ? JSON.stringify({ [plan.config_key ?? "mcpServers"]: { invectus: plan.entry } }, null, 2) : plan.manual}</pre>
        {plan.method === "file" && <div className="dim small">Other servers and settings in the file are kept; the old file is saved beside it as a backup.</div>}
        {plan.notes.map((n) => <div key={n} className="dim small">Note: {n}</div>)}
        <div className="row wrap integration-modal-actions">
          <button type="button" className="text-btn" onClick={() => setPlan(undefined)}>Cancel</button>
          {plan.method === "command" && <button type="button" className="btn small" onClick={() => copy(plan.manual)}>{copied ? "Copied" : "Copy command"}</button>}
          {canApply && <button type="button" className="btn primary small" autoFocus disabled={action.busy} onClick={apply}>Make this change</button>}
        </div>
      </div>
    </div>}
    {done && <div className="note small">{done}</div>}
    <ErrorLine error={action.error} />
    <details className="settings-agent-about">
      <summary>How connections work</summary>
      <div className="muted small">Lets an agent such as Claude, Codex or Grok use inVectus through MCP: search, verdicts, gaps, descriptions and suggestions. It runs on this computer over stdio and opens no network port. While this app is open, the agent works in the project you have open here, and its suggestions wait for your decision.</div>
      <div className="muted small">The agent runs on your own plan or API key, signed in through its own app. inVectus never sees the sign-in; for Claude Code, Codex and Grok Build it only asks the client who is signed in.</div>
      <div className="muted small">Clients marked Unverified are set up from their documentation; no session has been checked through them yet.</div>
      <div className="muted small">Or by hand: <code>invectus connect clients</code> lists them, and <code>invectus connect add codex</code> (or any listed id) shows the change; <code>--yes</code> makes it. Claude Desktop can also install <code>invectus connect bundle</code>'s .mcpb file.</div>
    </details>

    <div className="label agent-activity-head">Recent agent activity</div>
    {activity.length === 0 && <div className="dim small">No agent has worked through this app since it opened.</div>}
    {activity.length > 0 && <ul className="plain agent-activity">{activity.slice(0, 8).map((a, i) => <li key={`${a.at_ms}-${i}`} className="small">
      <span className="when">{ago(a.at_ms, Date.now())}</span>
      <span>{clientName(a.client)} {describe(a)}</span>
      {!a.ok && <span className="err">failed</span>}
    </li>)}</ul>}
  </div>;
}
