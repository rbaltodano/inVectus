// Integrations: data sources and outside checks by category, with search and filters (AI agents
// have their own page, Agents.tsx). Excel is
// read locally; Google Sheets (shared by link), Notion, Zotero and Airtable (tokens in the
// keychain) are pulled into a project only when the person asks, then imported offline
// (docs/integrations.md).
import { useEffect, useState, type ReactNode } from "react";
import { BookMarked, ClipboardList, Database, FileSpreadsheet, Library, Lightbulb, ListChecks, Map as MapIcon, NotebookText, Search, Sheet, Table2, X, type LucideIcon } from "lucide-react";
import { api, inTauri } from "../api";
import { ErrorLine, useAction } from "../App";
import Link from "../Link";

type Category = "tables" | "docs" | "research" | "forms" | "product" | "databases";
type StatusFilter = "all" | "connected" | "token" | "ready";

const CATEGORIES: { id: Category; label: string; short: string; about: string }[] = [
  { id: "tables", label: "Spreadsheets & tables", short: "Tables", about: "Rows from files and table tools; each row becomes an item." },
  { id: "docs", label: "Docs & notes", short: "Docs", about: "Pages from your team's notes and wikis." },
  { id: "research", label: "Research", short: "Research", about: "Your reference library, and the scholarly indexes gap checks search." },
  { id: "forms", label: "Forms & surveys", short: "Forms", about: "Open-text answers: what people say they need." },
  { id: "product", label: "Product feedback", short: "Feedback", about: "Feature requests and customer notes, next to what you plan to build." },
  { id: "databases", label: "Databases", short: "Databases", about: "Direct database sources." },
];

const STATUS_FILTERS: { id: StatusFilter; label: string }[] = [
  { id: "all", label: "Any status" },
  { id: "connected", label: "Connected" },
  { id: "token", label: "Needs a token" },
  { id: "ready", label: "No setup" },
];

/** One card. Token cards (Notion, Zotero…) come from TOKEN_INTEGRATIONS; the rest are fixed. */
interface Entry {
  id: string; name: string; category: Category; code: string; keywords: string;
  token?: TokenIntegration;
  icon?: LucideIcon; desc?: string; badge?: string; muted?: boolean;
}

export default function Integrations() {
  // undefined while loading; each integration's token source, or null when not connected.
  const [tokens, setTokens] = useState<Record<string, string | null>>();
  useEffect(() => { if (inTauri) api.integrationTokens().then(setTokens).catch(() => setTokens({})); else setTokens({}); }, []);
  const [query, setQuery] = useState("");
  const [category, setCategory] = useState<Category | "all">("all");
  const [status, setStatus] = useState<StatusFilter>("all");
  // Every word must appear in the card's name, description, category or keywords.
  const terms = query.toLowerCase().trim().split(/\s+/).filter(Boolean);
  const text = (e: Entry) => [e.name, e.desc, e.keywords, e.token?.about, e.token?.use, e.token?.keywords, CATEGORIES.find((c) => c.id === e.category)!.label].join(" ").toLowerCase();
  const connected = (e: Entry) => !!e.token && !!tokens?.[e.token.id];
  const statusOk = (e: Entry) => status === "all" || (status === "connected" ? connected(e) : status === "token" ? !!e.token && !connected(e) : !e.token && !e.muted);
  const searched = ENTRIES.filter((e) => terms.every((w) => text(e).includes(w)) && statusOk(e));
  const visible = searched.filter((e) => category === "all" || e.category === category);
  const filtered = !!query.trim() || category !== "all" || status !== "all";
  const clear = () => { setQuery(""); setCategory("all"); setStatus("all"); };

  const card = (e: Entry) => e.token
    ? <TokenCard key={e.id} it={e.token} label={e.code} from={tokens === undefined ? undefined : tokens[e.token.id] ?? null}
      onChange={(from) => setTokens((t) => ({ ...t, [e.token!.id]: from }))} />
    : <li key={e.id} className="integration-card" data-disabled={e.muted || undefined} data-label={e.code}>
      <div className="integration-card-top">
        <span className="integration-mark">{e.icon && <e.icon size={22} strokeWidth={1.5} aria-hidden="true" />}</span>
        <span className={`integration-badge${e.muted ? " integration-badge-muted" : ""}`} data-tone={e.muted ? undefined : "ink"}>{e.badge}</span>
      </div>
      <div className="integration-name">{e.name}</div>
      <p className="integration-desc">{e.desc}</p>
    </li>;

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
      <div className="integrations-filters">
        <div role="group" aria-label="Category" className="integrations-chips">
          {[{ id: "all" as const, short: "All", label: "All categories" }, ...CATEGORIES].map((c) => {
            const n = c.id === "all" ? searched.length : searched.filter((e) => e.category === c.id).length;
            return <button key={c.id} type="button" className="integrations-chip" aria-pressed={category === c.id} disabled={n === 0 && category !== c.id}
              title={c.label} onClick={() => setCategory(c.id)}>{c.short}<span className="integrations-chip-count">{n}</span></button>;
          })}
        </div>
        <label className="integrations-status" data-active={status !== "all"}>
          <span>Status</span>
          <select value={status} onChange={(e) => setStatus(e.target.value as StatusFilter)}>
            {STATUS_FILTERS.map((f) => <option key={f.id} value={f.id}>{f.label}</option>)}
          </select>
        </label>
      </div>
      {visible.length === 0 && <p className="integrations-empty" role="status">No integrations match these filters. <button type="button" className="text-btn" onClick={clear}>Clear filters</button></p>}
      {CATEGORIES.map((c) => {
        const items = visible.filter((e) => e.category === c.id);
        if (items.length === 0) return null;
        return <section key={c.id} aria-labelledby={`i-${c.id}`} className="integrations-section">
          <div className="integrations-section-head">
            <h2 id={`i-${c.id}`}>{c.label}<span className="integrations-section-count">{String(items.length).padStart(2, "0")}</span></h2>
            <p>{c.about}</p>
          </div>
          <ul className="integration-grid">{items.map(card)}</ul>
        </section>;
      })}
      {!filtered && <p className="integrations-footnote">Online sources are pulled once, when you ask, and then work offline. Outside checks run only when a project's network switch is on.</p>}
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

const token = (id: TokenIntegration["id"]) => TOKEN_INTEGRATIONS.find((t) => t.id === id)!;

// Codes stay fixed per integration (DS: data source, OC: outside check), whatever is filtered.
const ENTRIES: Entry[] = [
  { id: "excel", name: "Excel", category: "tables", code: "DS-01", icon: FileSpreadsheet, badge: "Built in", keywords: "xlsx xls ods spreadsheet OpenDocument file local",
    desc: "Import .xlsx, .xls or .ods files from Add to Dataset and pick the sheet. Read on this computer." },
  { id: "sheets", name: "Google Sheets", category: "tables", code: "DS-02", icon: Sheet, badge: "Link import", keywords: "spreadsheet link shared google",
    desc: "Paste a sheet's link in Add to Dataset. The sheet must be shared as “Anyone with the link”; no sign-in." },
  { id: "airtable", name: "Airtable", category: "tables", code: "DS-03", token: token("airtable"), keywords: "" },
  { id: "notion", name: "Notion", category: "docs", code: "DS-04", token: token("notion"), keywords: "" },
  { id: "zotero", name: "Zotero", category: "research", code: "DS-05", token: token("zotero"), keywords: "" },
  { id: "scholarly", name: "Scholarly sources", category: "research", code: "OC-01", icon: Library, badge: "Built in", keywords: "outside check gap verify PubMed OpenAlex Semantic Scholar arXiv papers",
    desc: "Outside check. PubMed and OpenAlex by default; Semantic Scholar and arXiv when you turn them on in Settings → Network. No keys needed." },
  { id: "typeform", name: "Typeform", category: "forms", code: "DS-06", token: token("typeform"), keywords: "" },
  { id: "tally", name: "Tally", category: "forms", code: "DS-07", token: token("tally"), keywords: "" },
  { id: "canny", name: "Canny", category: "product", code: "DS-08", token: token("canny"), keywords: "" },
  { id: "productboard", name: "Productboard", category: "product", code: "DS-09", token: token("productboard"), keywords: "" },
  { id: "databases", name: "Databases", category: "databases", code: "DS-10", icon: Database, badge: "Coming later", muted: true, keywords: "SQL Postgres MySQL SQLite DuckDB",
    desc: "Until then, export a table to CSV, Parquet or Excel and import the file." },
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
