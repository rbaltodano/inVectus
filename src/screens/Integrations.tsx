// Integrations: what inVectus connects to. AI agents (MCP clients) and data sources. Excel is
// read locally; Google Sheets (shared by link) and Notion (internal integration token) are
// pulled into a project only when the person asks, then imported offline (docs/integrations.md).
import { useEffect, useState } from "react";
import { Database, FileSpreadsheet, NotebookText, Sheet } from "lucide-react";
import { api, inTauri } from "../api";
import { ErrorLine, useAction } from "../App";
import Link from "../Link";
import ConnectAgents from "./ConnectAgents";

export default function Integrations() {
  return <div className="page integrations-page">
    <div className="integrations-inner">
      <header className="integrations-header">
        <h1>Integrations</h1>
        <p>Connect inVectus to your AI agents and bring in data from the tools you already use.</p>
      </header>
      <section aria-labelledby="i-data" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="i-data">Data sources</h2>
          <p>Bring rows in as a collection. Online sources are pulled once, when you ask, and then work offline.</p>
        </div>
        <ul className="integration-grid">
          <li className="integration-card">
            <div className="integration-card-top">
              <span className="integration-mark"><FileSpreadsheet size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge">Built in</span>
            </div>
            <div className="integration-name">Excel</div>
            <p className="integration-desc">Import .xlsx, .xls or .ods files from Add to Dataset and pick the sheet. Read on this computer.</p>
          </li>
          <li className="integration-card">
            <div className="integration-card-top">
              <span className="integration-mark"><Sheet size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge">Link import</span>
            </div>
            <div className="integration-name">Google Sheets</div>
            <p className="integration-desc">Paste a sheet's link in Add to Dataset. The sheet must be shared as “Anyone with the link”; no sign-in.</p>
          </li>
          <NotionCard />
          <li className="integration-card" data-disabled="true">
            <div className="integration-card-top">
              <span className="integration-mark"><Database size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge integration-badge-muted">Coming later</span>
            </div>
            <div className="integration-name">Databases</div>
            <p className="integration-desc">Until then, export a table to CSV, Parquet or Excel and import the file.</p>
          </li>
        </ul>
      </section>
      <section aria-labelledby="i-agents" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="i-agents">AI agents</h2>
          <p>Let your agents search collections, inspect evidence and suggest decisions. Works for all projects.</p>
        </div>
        <ConnectAgents />
      </section>
    </div>
  </div>;
}

function NotionCard() {
  const [from, setFrom] = useState<string | null>();
  const [open, setOpen] = useState(false);
  const [token, setToken] = useState("");
  const action = useAction();
  useEffect(() => { if (inTauri) api.notionStatus().then(setFrom).catch(() => setFrom(null)); }, []);
  const connected = !!from;
  const save = () => action.run(async () => { setFrom(await api.setNotionToken(token)); setToken(""); setOpen(false); });
  const disconnect = () => action.run(async () => { setFrom(await api.setNotionToken("")); });

  return <li className="integration-card" data-connected={connected}>
    <div className="integration-card-top">
      <span className="integration-mark"><NotebookText size={22} strokeWidth={1.5} aria-hidden="true" /></span>
      {connected
        ? <span className="integration-badge">Connected</span>
        : <button type="button" className="integration-connect" disabled={!inTauri || from === undefined} onClick={() => setOpen(true)}>Connect</button>}
    </div>
    <div className="integration-name">Notion</div>
    <p className="integration-desc">{connected
      ? `Token saved in the ${from === "environment" ? "INVECTUS_NOTION_TOKEN variable" : "system keychain"}. Paste a database link in Add to Dataset.`
      : "Import a Notion database: one item per page, with its properties and text."}</p>
    {connected && from === "keychain" && <button type="button" className="text-btn integration-disconnect" disabled={action.busy} onClick={disconnect}>Disconnect</button>}
    <ErrorLine error={action.error} />
    {open && <div className="integration-modal-backdrop" onClick={(e) => { if (e.target === e.currentTarget) setOpen(false); }}
      onKeyDown={(e) => { if (e.key === "Escape") setOpen(false); }}>
      <div className="integration-modal stack gap-s" role="dialog" aria-modal="true" aria-labelledby="notion-connect-title">
        <div className="integration-modal-head"><span className="integration-mark"><NotebookText size={22} strokeWidth={1.5} aria-hidden="true" /></span><h3 id="notion-connect-title">Connect Notion</h3></div>
        <ol className="integration-steps small">
          <li>Create an internal integration at <Link url="https://www.notion.so/profile/integrations">notion.so/profile/integrations</Link> with “Read content” access.</li>
          <li>Copy its secret (it starts with <code>ntn_</code>) and paste it below.</li>
          <li>In Notion, open each database you want and choose ••• → Connections → your integration.</li>
        </ol>
        <label className="small">Integration secret
          <input type="password" autoFocus autoComplete="off" spellCheck={false} value={token} placeholder="ntn_…"
            onChange={(e) => setToken(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && token.trim()) save(); }} />
        </label>
        <div className="dim small">Saved in your system keychain, never in project files. Nothing is sent to Notion until you pull a database.</div>
        <ErrorLine error={action.error} />
        <div className="row wrap integration-modal-actions">
          <button type="button" className="text-btn" onClick={() => setOpen(false)}>Cancel</button>
          <button type="button" className="btn primary small" disabled={action.busy || !token.trim()} onClick={save}>Save token</button>
        </div>
      </div>
    </div>}
  </li>;
}
