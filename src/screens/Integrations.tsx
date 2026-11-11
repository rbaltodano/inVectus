// Integrations: what inVectus connects to. AI agents (MCP clients) and data sources. Excel is
// read locally; Google Sheets (shared by link), Notion, Zotero and Airtable (tokens in the
// keychain) are pulled into a project only when the person asks, then imported offline
// (docs/integrations.md).
import { useEffect, useState, type ReactNode } from "react";
import { BookMarked, Database, FileSpreadsheet, NotebookText, Sheet, Table2, type LucideIcon } from "lucide-react";
import { api, inTauri } from "../api";
import { ErrorLine, useAction } from "../App";
import Link from "../Link";
import ConnectAgents from "./ConnectAgents";

export default function Integrations() {
  // undefined while loading; each integration's token source, or null when not connected.
  const [tokens, setTokens] = useState<Record<string, string | null>>();
  useEffect(() => { if (inTauri) api.integrationTokens().then(setTokens).catch(() => setTokens({})); else setTokens({}); }, []);
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
          {TOKEN_INTEGRATIONS.map((it) => <TokenCard key={it.id} it={it} from={tokens === undefined ? undefined : tokens[it.id] ?? null}
            onChange={(from) => setTokens((t) => ({ ...t, [it.id]: from }))} />)}
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

interface TokenIntegration {
  id: "notion" | "zotero" | "airtable"; name: string; icon: LucideIcon; about: string; use: string;
  placeholder: string; label: string; steps: ReactNode[];
}

const TOKEN_INTEGRATIONS: TokenIntegration[] = [
  {
    id: "notion", name: "Notion", icon: NotebookText, label: "Integration secret", placeholder: "ntn_…",
    about: "Import a Notion database: one item per page, with its properties and text.",
    use: "Paste a database link in Add to Dataset.",
    steps: [
      <>Create an internal integration at <Link url="https://www.notion.so/profile/integrations">notion.so/profile/integrations</Link> with “Read content” access.</>,
      <>Copy its secret (it starts with <code>ntn_</code>) and paste it below.</>,
      <>In Notion, open each database you want and choose ••• → Connections → your integration.</>,
    ],
  },
  {
    id: "zotero", name: "Zotero", icon: BookMarked, label: "API key", placeholder: "24 letters and digits",
    about: "Import your Zotero library or one collection: titles, abstracts, authors, tags and DOIs.",
    use: "In Add to Dataset, paste a zotero.org library or collection link, or type zotero:library.",
    steps: [
      <>Create a key at <Link url="https://www.zotero.org/settings/keys/new">zotero.org/settings/keys</Link> with “Allow library access” (read only). For group libraries, also allow group read access.</>,
      <>Copy the key and paste it below.</>,
      <>Your library must be synced to zotero.org (Zotero → Settings → Sync).</>,
    ],
  },
  {
    id: "airtable", name: "Airtable", icon: Table2, label: "Personal access token", placeholder: "pat…",
    about: "Import an Airtable table or view: one item per record, with every field.",
    use: "Paste a table or view link in Add to Dataset.",
    steps: [
      <>Create a token at <Link url="https://airtable.com/create/tokens">airtable.com/create/tokens</Link> with the <code>data.records:read</code> scope.</>,
      <>Under Access, add the bases you want to import from.</>,
      <>Copy the token (it starts with <code>pat</code>) and paste it below.</>,
    ],
  },
];

function TokenCard({ it, from, onChange }: { it: TokenIntegration; from: string | null | undefined; onChange: (from: string | null) => void }) {
  const [open, setOpen] = useState(false);
  const [token, setToken] = useState("");
  const action = useAction();
  const connected = !!from;
  const Icon = it.icon;
  const save = () => action.run(async () => { onChange(await api.setIntegrationToken(it.id, token)); setToken(""); setOpen(false); });
  const disconnect = () => action.run(async () => { onChange(await api.setIntegrationToken(it.id, "")); });
  const titleId = `${it.id}-connect-title`;

  return <li className="integration-card" data-connected={connected}>
    <div className="integration-card-top">
      <span className="integration-mark"><Icon size={22} strokeWidth={1.5} aria-hidden="true" /></span>
      {connected
        ? <span className="integration-badge">Connected</span>
        : <button type="button" className="integration-connect" disabled={!inTauri || from === undefined} onClick={() => setOpen(true)}>Connect</button>}
    </div>
    <div className="integration-name">{it.name}</div>
    <p className="integration-desc">{connected
      ? `Token saved in the ${from === "environment" ? `INVECTUS_${it.id.toUpperCase()}_TOKEN variable` : "system keychain"}. ${it.use}`
      : it.about}</p>
    {connected && from === "keychain" && <button type="button" className="text-btn integration-disconnect" disabled={action.busy} onClick={disconnect}>Disconnect</button>}
    {!open && <ErrorLine error={action.error} />}
    {open && <div className="integration-modal-backdrop" onClick={(e) => { if (e.target === e.currentTarget) setOpen(false); }}
      onKeyDown={(e) => { if (e.key === "Escape") setOpen(false); }}>
      <div className="integration-modal stack gap-s" role="dialog" aria-modal="true" aria-labelledby={titleId}>
        <div className="integration-modal-head"><span className="integration-mark"><Icon size={22} strokeWidth={1.5} aria-hidden="true" /></span><h3 id={titleId}>Connect {it.name}</h3></div>
        <ol className="integration-steps small">{it.steps.map((step, i) => <li key={i}>{step}</li>)}</ol>
        <label className="small">{it.label}
          <input type="password" autoFocus autoComplete="off" spellCheck={false} value={token} placeholder={it.placeholder}
            onChange={(e) => setToken(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter" && token.trim()) save(); }} />
        </label>
        <div className="dim small">Saved in your system keychain, never in project files. Nothing is sent to {it.name} until you pull.</div>
        <ErrorLine error={action.error} />
        <div className="row wrap integration-modal-actions">
          <button type="button" className="text-btn" onClick={() => setOpen(false)}>Cancel</button>
          <button type="button" className="btn primary small" disabled={action.busy || !token.trim()} onClick={save}>Save token</button>
        </div>
      </div>
    </div>}
  </li>;
}
