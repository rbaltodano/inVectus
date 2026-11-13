import { useEffect, useState } from "react";
import { api, inTauri, type OutboundCall } from "./api";

/** Every network call this app made since it opened (SPEC §7.7): when, which host, why, and
 *  how it ended. Paths, ids, tokens and contents are never recorded, so none are shown. */
export default function OutboundLog() {
  const [calls, setCalls] = useState<OutboundCall[]>();
  const load = () => { if (inTauri) api.outboundLog().then(setCalls).catch(() => setCalls([])); else setCalls([]); };
  useEffect(load, []);
  const shown = (calls ?? []).slice().reverse().slice(0, 100);
  return <section aria-labelledby="s-outbound" className="panel settings-group stack gap">
    <div className="spread">
      <h3 id="s-outbound">Network activity</h3>
      <button type="button" className="btn small" onClick={load}>Refresh</button>
    </div>
    <div className="muted small">Every call inVectus made since it opened: the service and why. Addresses, ids, tokens and your data are never recorded.</div>
    {calls === undefined ? <p className="muted small" role="status">Loading…</p>
      : shown.length === 0 ? <p className="muted small">No network calls yet.</p>
      : <ul className="plain outbound-log">{shown.map((c, i) => <li key={`${c.at_ms}-${i}`} className="spread small">
          <span><b>{c.purpose}</b> <span className="muted mono">{c.host}</span></span>
          <span className="muted mono">{c.status === null ? "no answer" : c.status} · {new Date(c.at_ms).toLocaleTimeString()}</span>
        </li>)}</ul>}
  </section>;
}
