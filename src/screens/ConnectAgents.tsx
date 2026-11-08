// Settings → Connect an AI agent (target product §5.1, A6). Each client shows whether it is
// connected; connecting shows the exact change first (the file and entry, or the command) and
// makes it only after "Make this change". Below, the agents' recent calls through this app.
import { useEffect, useState } from "react";
import { api, inTauri, type AgentAccount, type AgentClient, type ConnectPlan, type ConnectStatus } from "../api";
import { ErrorLine, useAction } from "../App";
import { ago, describe, useAgentActivity } from "../agents";
import { clientName } from "../agentIdentity";

const CLIENTS: { id: AgentClient; name: string; how: string }[] = [
  { id: "claude-code", name: "Claude Code", how: "Registers inVectus with Claude Code's own `claude mcp add` command." },
  { id: "claude-desktop", name: "Claude Desktop", how: "Adds inVectus to Claude Desktop's configuration file." },
  { id: "codex", name: "Codex", how: "Registers inVectus with Codex's own `codex mcp add` command (saved in ~/.codex/config.toml)." },
  { id: "cursor", name: "Cursor", how: "Adds inVectus to Cursor's ~/.cursor/mcp.json." },
  { id: "grok", name: "Grok Build", how: "Registers inVectus with Grok Build's own `grok mcp add` command (saved in ~/.grok/config.toml)." },
];

// Clients whose CLI can say who is signed in. The client keeps the sign-in.
const HAS_ACCOUNT: AgentClient[] = ["claude-code", "codex", "grok"];

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
  const [status, setStatus] = useState<ConnectStatus[]>([]);
  const [plan, setPlan] = useState<ConnectPlan>();
  const [done, setDone] = useState<string>();
  const [copied, setCopied] = useState(false);
  const [accounts, setAccounts] = useState<Partial<Record<AgentClient, AgentAccount | null>>>({});
  const action = useAction();
  const activity = useAgentActivity();
  const reload = () => { if (inTauri) api.connectStatus().then(setStatus).catch(() => undefined); };
  // Each CLI takes about a second to answer, so accounts load after the connection status.
  const checkAccounts = () => {
    if (!inTauri) return;
    for (const c of HAS_ACCOUNT) api.connectAccount(c).then((a) => setAccounts((m) => ({ ...m, [c]: a }))).catch(() => undefined);
  };
  useEffect(() => { reload(); checkAccounts(); }, []);

  const open = (client: AgentClient) => action.run(async () => { setDone(undefined); setCopied(false); setPlan(await api.connectPlan(client)); });
  const apply = () => action.run(async () => { setDone(await api.connectApply(plan!.client)); setPlan(undefined); reload(); });
  const remove = (client: AgentClient) => action.run(async () => { setDone(await api.connectRemove(client)); setPlan(undefined); reload(); });
  const copy = async (text: string) => { try { await navigator.clipboard.writeText(text); setCopied(true); } catch { /* clipboard unavailable */ } };
  const canApply = plan && !plan.up_to_date && (plan.method === "file" || !!plan.command);

  return <section aria-labelledby="s-agents" className="panel stack gap">
    <h2 id="s-agents">Connect an AI agent</h2>
    <div className="muted small">Lets an agent such as Claude, Codex or Grok use inVectus through MCP: search, verdicts, gaps, descriptions and suggestions. It runs on this computer over stdio and opens no network port. While this app is open, the agent works in the project you have open here, and its suggestions wait for your decision.</div>
    <div className="muted small">The agent runs on your own Claude, ChatGPT or Grok plan (or API key), signed in through its own app. inVectus never sees the sign-in; it only asks the client who is signed in.</div>
    {CLIENTS.map((c) => {
      const s = status.find((x) => x.client === c.id);
      const connected = s?.connected && s.matches_this_install;
      const account = s?.client_found ? accountText(accounts[c.id]) : null;
      return <div key={c.id} className="spread setting-row">
        <div><b>{c.name}</b>
          <div className="muted small">{statusText(s)}{s?.connected && !s.matches_this_install && s.binary ? `: ${s.binary}` : ""}</div>
          {account && <div className="dim small">{account}</div>}</div>
        <div className="row">
          {s?.connected && <button type="button" className="text-btn" disabled={action.busy} onClick={() => remove(c.id)}>Disconnect</button>}
          {!connected && <button type="button" className="btn small" disabled={action.busy} onClick={() => open(c.id)}>{s?.connected ? "Use this install…" : "Connect…"}</button>}
        </div>
      </div>;
    })}
    {plan && <div className="connect-plan stack gap-s" role="dialog" aria-label={`Connect ${plan.display_name}`}>
      <div className="label">Connect {plan.display_name}</div>
      <div className="muted small">{CLIENTS.find((c) => c.id === plan.client)?.how} Nothing has changed yet.</div>
      {plan.config_file && <div className="small">File: <code className="path">{plan.config_file}</code></div>}
      <pre className="connect-code">{plan.method === "file" ? JSON.stringify({ mcpServers: { invectus: plan.entry } }, null, 2) : plan.manual}</pre>
      {plan.method === "file" && <div className="dim small">Other servers and settings in the file are kept; the old file is saved beside it as a backup.</div>}
      {plan.notes.map((n) => <div key={n} className="dim small">Note: {n}</div>)}
      <div className="row wrap">
        {canApply && <button type="button" className="btn primary small" disabled={action.busy} onClick={apply}>Make this change</button>}
        {plan.method === "command" && <button type="button" className="btn small" onClick={() => copy(plan.manual)}>{copied ? "Copied" : "Copy command"}</button>}
        <button type="button" className="text-btn" onClick={() => setPlan(undefined)}>Cancel</button>
      </div>
    </div>}
    {done && <div className="note small">{done}</div>}
    <ErrorLine error={action.error} />
    <div className="muted small">Or by hand: <code>invectus connect add claude-code</code> (or <code>codex</code>, <code>grok</code>) shows the change, and <code>--yes</code> makes it. Claude Desktop can also install <code>invectus connect bundle</code>'s .mcpb file.</div>

    <div className="label agent-activity-head">Recent agent activity</div>
    {activity.length === 0 && <div className="dim small">No agent has worked through this app since it opened.</div>}
    {activity.length > 0 && <ul className="plain agent-activity">{activity.slice(0, 8).map((a, i) => <li key={`${a.at_ms}-${i}`} className="small">
      <span className="when">{ago(a.at_ms, Date.now())}</span>
      <span>{clientName(a.client)} {describe(a)}</span>
      {!a.ok && <span className="err">failed</span>}
    </li>)}</ul>}
  </section>;
}
