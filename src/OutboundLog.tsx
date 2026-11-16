import { useEffect, useState } from "react";
import { api, inTauri, type OutboundCall } from "./api";
import SettingsGroup from "./SettingsGroup";

/** Every network call inVectus made since the app opened, including the CLI and connected
 *  agents (SPEC §7.7): when, which host, why, and how it ended. Paths, ids, tokens and contents
 *  are never recorded, so none are shown. "ran" marks another program (gh, claude, codex,
 *  grok) that inVectus started and that may have used the network. */
export default function OutboundLog() {
  const [calls, setCalls] = useState<OutboundCall[]>();
  const load = () => { if (inTauri) api.outboundLog().then(setCalls).catch(() => setCalls([])); else setCalls([]); };
  useEffect(load, []);
  const shown = (calls ?? []).slice().reverse().slice(0, 100);
  return <SettingsGroup id="s-outbound" title="Network activity"
    description="Every call inVectus made since the app opened, including the CLI and connected agents: the service and why. Addresses, ids, tokens and your data are never recorded."
    action={<button type="button" className="btn small" onClick={load}>Refresh</button>}>
    {calls === undefined ? <p className="muted small" role="status">Loading…</p>
      : shown.length === 0 ? <p className="muted small">No network calls yet.</p>
      : <ul className="plain outbound-log">{shown.map((c, i) => <li key={`${c.at_ms}-${i}`} className="spread small">
          <span><b>{c.purpose}</b> <span className="muted mono">{c.host}</span></span>
          <span className="muted mono">{c.status !== null ? c.status : c.via === "command" ? "ran" : "no answer"}{c.process && c.process !== "app" ? ` · ${c.process === "agent" ? "agent" : "CLI"}` : ""} · {new Date(c.at_ms).toLocaleTimeString()}</span>
        </li>)}</ul>}
  </SettingsGroup>;
}
