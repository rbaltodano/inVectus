// Integrations: data sources and outside checks (AI agents have their own page, Agents.tsx). Excel is
// read locally; Google Sheets (shared by link), Notion, Zotero and Airtable (tokens in the
// keychain) are pulled into a project only when the person asks, then imported offline
// (docs/integrations.md).
import { useEffect, useState, type ReactNode } from "react";
import { BookMarked, ClipboardList, Database, FileSpreadsheet, Library, Lightbulb, ListChecks, Map as MapIcon, NotebookText, Search, Sheet, Table2, X, type LucideIcon } from "lucide-react";
import { api, inTauri } from "../api";
import { ErrorLine, useAction } from "../App";
import Link from "../Link";

export default function Integrations() {
  // undefined while loading; each integration's token source, or null when not connected.
  const [tokens, setTokens] = useState<Record<string, string | null>>();
  useEffect(() => { if (inTauri) api.integrationTokens().then(setTokens).catch(() => setTokens({})); else setTokens({}); }, []);
  const [query, setQuery] = useState("");
  // Every word must appear in the card's name, description or keywords.
  const terms = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  const shown = (...text: string[]) => { const t = text.join(" ").toLowerCase(); return terms.every((w) => t.includes(w)); };
  const tokenCards = TOKEN_INTEGRATIONS.filter((it) => shown(it.name, it.about, it.use, it.keywords));
  const builtIn = {
    excel: shown("Excel xlsx xls ods spreadsheet OpenDocument file local Built in"),
    sheets: shown("Google Sheets spreadsheet link shared Link import"),
    databases: shown("Databases SQL Postgres MySQL SQLite DuckDB Coming later"),
    scholarly: shown("Scholarly sources PubMed OpenAlex Semantic Scholar arXiv papers research Built in"),
  };
  const anyData = builtIn.excel || builtIn.sheets || builtIn.databases || tokenCards.length > 0;
  return <div className="page integrations-page">
    <div className="integrations-inner">
      <header className="integrations-header integrations-header-row">
        <div className="integrations-header-text">
          <h1>Integrations</h1>
          <p>Bring in data from the tools you already use, and choose where gap checks look for existing work. AI agents have their own page.</p>
        </div>
        <div className="settings-search integrations-search">
          <Search size={14} aria-hidden="true" />
          <input aria-label="Search integrations" placeholder="Search integrations…" value={query} onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Escape") { e.preventDefault(); setQuery(""); } }} />
          {query && <button type="button" aria-label="Clear integrations search" onClick={() => setQuery("")}><X size={13} aria-hidden="true" /></button>}
        </div>
      </header>
      {!anyData && !builtIn.scholarly && <p className="integrations-empty" role="status">No integrations match “{query.trim()}”.</p>}
      {anyData && <section aria-labelledby="i-data" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="i-data">Data sources</h2>
          <p>Bring rows in as a collection. Online sources are pulled once, when you ask, and then work offline.</p>
        </div>
        <ul className="integration-grid" data-code="DS">
          {builtIn.excel && <li className="integration-card" data-label="DS-01">
            <div className="integration-card-top">
              <span className="integration-mark"><FileSpreadsheet size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge" data-tone="ink">Built in</span>
            </div>
            <div className="integration-name">Excel</div>
            <p className="integration-desc">Import .xlsx, .xls or .ods files from Add to Dataset and pick the sheet. Read on this computer.</p>
          </li>}
          {builtIn.sheets && <li className="integration-card" data-label="DS-02">
            <div className="integration-card-top">
              <span className="integration-mark"><Sheet size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge" data-tone="ink">Link import</span>
            </div>
            <div className="integration-name">Google Sheets</div>
            <p className="integration-desc">Paste a sheet's link in Add to Dataset. The sheet must be shared as “Anyone with the link”; no sign-in.</p>
          </li>}
          {tokenCards.map((it) => <TokenCard key={it.id} it={it} label={`DS-${String(TOKEN_INTEGRATIONS.indexOf(it) + 3).padStart(2, "0")}`} from={tokens === undefined ? undefined : tokens[it.id] ?? null}
            onChange={(from) => setTokens((t) => ({ ...t, [it.id]: from }))} />)}
          {builtIn.databases && <li className="integration-card" data-disabled="true" data-label={`DS-${String(TOKEN_INTEGRATIONS.length + 3).padStart(2, "0")}`}>
            <div className="integration-card-top">
              <span className="integration-mark"><Database size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge integration-badge-muted">Coming later</span>
            </div>
            <div className="integration-name">Databases</div>
            <p className="integration-desc">Until then, export a table to CSV, Parquet or Excel and import the file.</p>
          </li>}
        </ul>
      </section>}
      {builtIn.scholarly && <section aria-labelledby="i-checks" className="integrations-section">
        <div className="integrations-section-head">
          <h2 id="i-checks">Outside checks</h2>
          <p>Where a gap check looks for existing work. Choose them per project in Settings → Network; they run only when that project's network switch is on.</p>
        </div>
        <ul className="integration-grid" data-code="OC">
          <li className="integration-card" data-label="OC-01">
            <div className="integration-card-top">
              <span className="integration-mark"><Library size={22} strokeWidth={1.5} aria-hidden="true" /></span>
              <span className="integration-badge" data-tone="ink">Built in</span>
            </div>
            <div className="integration-name">Scholarly sources</div>
            <p className="integration-desc">PubMed and OpenAlex by default; Semantic Scholar and arXiv when you turn them on. No keys needed.</p>
          </li>
        </ul>
      </section>}
    </div>
  </div>;
}

interface TokenIntegration {
  id: "notion" | "zotero" | "airtable" | "typeform" | "tally" | "canny" | "productboard"; name: string; icon: LucideIcon; about: string; use: string;
  placeholder: string; label: string; steps: ReactNode[];
  /** Extra words the page search matches (what people might type). */
  keywords: string;
}

const TOKEN_INTEGRATIONS: TokenIntegration[] = [
  {
    id: "notion", name: "Notion", icon: NotebookText, label: "Integration secret", placeholder: "ntn_…",
    keywords: "notes wiki docs pages database workspace",
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
    keywords: "research papers references citations library bibliography",
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
    keywords: "table base spreadsheet records",
    about: "Import an Airtable table or view: one item per record, with every field.",
    use: "Paste a table or view link in Add to Dataset.",
    steps: [
      <>Create a token at <Link url="https://airtable.com/create/tokens">airtable.com/create/tokens</Link> with the <code>data.records:read</code> scope.</>,
      <>Under Access, add the bases you want to import from.</>,
      <>Copy the token (it starts with <code>pat</code>) and paste it below.</>,
    ],
  },
  {
    id: "typeform", name: "Typeform", icon: ClipboardList, label: "Personal access token", placeholder: "tfp_…",
    keywords: "forms survey responses feedback",
    about: "Import a form's completed responses: one item per response, one column per question.",
    use: "Paste the form's share or edit link in Add to Dataset.",
    steps: [
      <>Create a token at <Link url="https://admin.typeform.com/user/tokens">admin.typeform.com/user/tokens</Link> with the <code>forms:read</code> and <code>responses:read</code> scopes.</>,
      <>Copy the token (it starts with <code>tfp_</code>) and paste it below.</>,
    ],
  },
  {
    id: "tally", name: "Tally", icon: ListChecks, label: "API key", placeholder: "tly-…",
    keywords: "forms survey submissions feedback",
    about: "Import a form's completed submissions: one item per submission, one column per question.",
    use: "Paste the form's share link (tally.so/r/…) in Add to Dataset.",
    steps: [
      <>In Tally, open Settings → API keys and create a key.</>,
      <>Copy the key (it starts with <code>tly-</code>) and paste it below.</>,
    ],
  },
  {
    id: "canny", name: "Canny", icon: Lightbulb, label: "Secret API key", placeholder: "Canny API key",
    keywords: "feature requests feedback votes roadmap",
    about: "Import feature requests with their status and votes: what people ask for, next to what you build.",
    use: "In Add to Dataset, type canny:all, or canny:board: and the board's name.",
    steps: [
      <>In Canny, open Settings → API &amp; Webhooks and copy the secret API key.</>,
      <>Paste it below. It is read only here: inVectus never writes to Canny.</>,
    ],
  },
  {
    id: "productboard", name: "Productboard", icon: MapIcon, label: "Access token", placeholder: "Productboard token",
    keywords: "feedback customer notes features roadmap product",
    about: "Import customer notes (what people ask for) or features (what you plan to build).",
    use: "In Add to Dataset, type productboard:notes or productboard:features.",
    steps: [
      <>In Productboard, open Settings → Integrations → Public API and create an access token.</>,
      <>Paste it below. inVectus only reads notes and features.</>,
    ],
  },
];

function TokenCard({ it, from, onChange, label }: { it: TokenIntegration; from: string | null | undefined; onChange: (from: string | null) => void; label: string }) {
  const [open, setOpen] = useState(false);
  const [token, setToken] = useState("");
  const action = useAction();
  const connected = !!from;
  const Icon = it.icon;
  const save = () => action.run(async () => { onChange(await api.setIntegrationToken(it.id, token)); setToken(""); setOpen(false); });
  const disconnect = () => action.run(async () => { onChange(await api.setIntegrationToken(it.id, "")); });
  const titleId = `${it.id}-connect-title`;

  return <li className="integration-card" data-connected={connected} data-label={label}>
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
