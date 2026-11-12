// Where data comes from: a file, a folder, or an integration whose token is saved. Used by the
// New project screen and by Add to Dataset. Several sources can be chosen; each chosen tile is
// highlighted in the accent colour. Link-based sources ask for their link in a dialog.
import { useEffect, useMemo, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, FileSpreadsheet, FolderOpen, RotateCw } from "lucide-react";
import { api, inTauri, pickSource, type SourceChoice } from "./api";
import { Shimmer } from "./agents";
import { ago } from "./ago";
import { useFlickerDelays } from "./dot-flicker";
import IntegrationIcon, { type IntegrationMarkName } from "./IntegrationIcon";
import { baseName } from "./paths";
import "./SourcePicker.css";

export type SourceId = "file" | "folder" | "sheets" | "notion" | "zotero" | "airtable" | "typeform" | "tally" | "canny" | "productboard";
interface Source {
  id: SourceId; name: string; hint: string;
  /** Integrations that need a saved token (Integrations page) before they can pull. */
  token?: "notion" | "zotero" | "airtable" | "typeform" | "tally" | "canny" | "productboard";
  /** Link-based sources: what to paste, and a line of help. */
  placeholder?: string; help?: string;
  mark?: IntegrationMarkName;
  /** Integrations that can list what they hold: what the things are called, and what they count. */
  list?: { noun: string; counts?: string };
}

export const SOURCES: Source[] = [
  { id: "file", name: "File", hint: "CSV, TSV, Parquet or Excel" },
  { id: "folder", name: "Folder", hint: "A folder of .txt or .md files" },
  { id: "sheets", name: "Google Sheets", hint: "A sheet shared by link", placeholder: "https://docs.google.com/spreadsheets/d/…", mark: "sheets",
    help: "Paste a link to a sheet shared as “Anyone with the link”." },
  { id: "notion", name: "Notion", hint: "A database", token: "notion", placeholder: "https://www.notion.so/…", list: { noun: "databases" }, mark: "notion",
    help: "The database must be shared with your Notion integration." },
  { id: "zotero", name: "Zotero", hint: "A library or collection", token: "zotero", placeholder: "zotero.org link, or zotero:library", list: { noun: "collections", counts: "items" }, mark: "zotero",
    help: "A zotero.org library or collection link, or zotero:library for your whole library." },
  { id: "airtable", name: "Airtable", hint: "A table or view", token: "airtable", placeholder: "https://airtable.com/app…", list: { noun: "tables" }, mark: "airtable",
    help: "A link to a table or a view." },
  { id: "typeform", name: "Typeform", hint: "Completed responses", token: "typeform", placeholder: "The form's share or edit link", list: { noun: "forms" }, mark: "typeform",
    help: "Only completed responses are pulled." },
  { id: "tally", name: "Tally", hint: "Completed submissions", token: "tally", placeholder: "https://tally.so/r/…", list: { noun: "forms", counts: "submissions" }, mark: "tally",
    help: "The form's share link. Only completed submissions are pulled." },
  { id: "canny", name: "Canny", hint: "Feature requests", token: "canny", placeholder: "canny:all, or canny:board:Name", list: { noun: "boards", counts: "posts" }, mark: "canny",
    help: "canny:all for every board, or canny:board: and the board's name." },
  { id: "productboard", name: "Productboard", hint: "Notes or features", token: "productboard", placeholder: "productboard:notes or productboard:features", list: { noun: "notes or features" }, mark: "productboard",
    help: "productboard:notes for customer notes, or productboard:features for features." },
];

/** A chosen source: what Import reads (a path, a link or a spec). */
export interface Picked { id: SourceId; path: string;
  /** A listed choice's name, shown on the tile instead of its spec. */
  label?: string }

const isLocal = (id: SourceId) => id === "file" || id === "folder";
const shortLink = (s: string) => s.replace(/^https?:\/\/(www\.)?/, "");

export default function SourcePicker({ picked, onChange, onIntegrations, disabled = false }: {
  picked: Picked[]; onChange: (next: Picked[]) => void; onIntegrations: () => void; disabled?: boolean;
}) {
  const [tokens, setTokens] = useState<Record<string, string | null>>({});
  const [editing, setEditing] = useState<Source>();
  const flicker = useFlickerDelays(); // each tile flickers in after its own random delay
  const [error, setError] = useState<string>();
  useEffect(() => { if (inTauri) api.integrationTokens().then(setTokens).catch(() => undefined); }, []);

  const shown = SOURCES.filter((s) => !s.token || !!tokens[s.token]);
  const chosen = (id: SourceId) => picked.find((p) => p.id === id);
  const set = (id: SourceId, path: string | null, label?: string) =>
    onChange(path ? [...picked.filter((p) => p.id !== id), { id, path, label }] : picked.filter((p) => p.id !== id));

  const choose = async (s: Source) => {
    if (disabled) return;
    setError(undefined);
    // A chosen source opens its dialog (change or remove); a new file or folder opens the picker.
    if (chosen(s.id) || !isLocal(s.id)) { setEditing(s); return; }
    const path = await pickSource(s.id === "folder").catch((e) => { setError(String(e)); return null; });
    if (path) set(s.id, path);
  };

  return <div className="source-picker">
    <ul className="np-sources">
      {shown.map((s) => {
        const p = chosen(s.id);
        return <li key={s.id}>
          <button type="button" className="np-source flicker-in" aria-pressed={!!p} disabled={disabled} onClick={() => void choose(s)} style={flicker(s.id)}>
            <span className="np-source-mark">{s.mark ? <IntegrationIcon name={s.mark} size={20} />
              : s.id === "file" ? <FileSpreadsheet size={20} strokeWidth={1.5} aria-hidden="true" />
              : <FolderOpen size={20} strokeWidth={1.5} aria-hidden="true" />}</span>
            <span className="np-source-name">{s.name}</span>
            <span className="np-source-hint">{p ? (isLocal(s.id) ? baseName(p.path) : p.label ?? shortLink(p.path)) : s.hint}</span>
            {p && <span className="np-source-check" aria-label="Will be imported"><Check size={12} strokeWidth={2.5} aria-hidden="true" /></span>}
          </button>
        </li>;
      })}
    </ul>
    <p className="dim small">More sources: <button type="button" className="text-btn" onClick={onIntegrations}>connect them on Integrations</button></p>
    {error && <p className="err small" role="alert">{error}</p>}
    {editing && <SourceDialog source={editing} current={chosen(editing.id)?.path}
      onUseFile={async () => {
        // A private sheet: import a downloaded copy instead, as a file (nothing is shared).
        const path = await pickSource(false).catch((e) => { setError(String(e)); return null; });
        if (path) onChange([...picked.filter((p) => p.id !== "file" && p.id !== editing.id), { id: "file", path }]);
        setEditing(undefined);
      }}
      onSave={(path, label) => { set(editing.id, path, label); setEditing(undefined); }}
      onReplace={async () => {
        const path = await pickSource(editing.id === "folder").catch((e) => { setError(String(e)); return null; });
        if (path) set(editing.id, path);
        setEditing(undefined);
      }}
      onRemove={() => { set(editing.id, null); setEditing(undefined); }}
      onClose={() => setEditing(undefined)} />}
  </div>;
}

/** A link source's link (or a chosen file's change and remove), in a dialog. */
function SourceDialog({ source, current, onSave, onReplace, onRemove, onClose, onUseFile }: {
  source: Source; current?: string; onSave: (path: string, label?: string) => void; onReplace: () => void; onRemove: () => void; onClose: () => void; onUseFile: () => void;
}) {
  const [value, setValue] = useState(current ?? "");
  // The chosen list entry's name, while the value is its spec (the link field then stays empty).
  const [label, setLabel] = useState<string>();
  const [linkOpen, setLinkOpen] = useState(!source.list || (!!current && /^https?:|\.(so|com|org|site)\//.test(current)));
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { e.preventDefault(); onClose(); } };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [onClose]);
  const local = isLocal(source.id);
  const titleId = `source-${source.id}-title`;
  return createPortal(<div className="integration-modal-backdrop" onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
    <div ref={box} className="integration-modal stack gap-s source-dialog" role="dialog" aria-modal="true" aria-labelledby={titleId}>
      <div className="integration-modal-head">
        <span className="integration-mark">{source.mark ? <IntegrationIcon name={source.mark} size={22} /> : source.id === "file"
          ? <FileSpreadsheet size={22} strokeWidth={1.5} aria-hidden="true" /> : <FolderOpen size={22} strokeWidth={1.5} aria-hidden="true" />}</span>
        <h3 id={titleId}>Import from {source.name}</h3>
      </div>
      {local
        ? <p className="small">Chosen: <span className="mono">{current}</span></p>
        : <>
          {source.list && <ChoiceList source={source} value={value} onPick={(c) => { setValue(c.spec); setLabel(c.title); }}
            onOpen={(c) => onSave(c.spec, c.title)} onListed={(n) => setLinkOpen(n === 0)} />}
          {source.list && !linkOpen
            ? <button type="button" className="text-btn source-link-toggle flicker-in" onClick={() => setLinkOpen(true)}>Paste a link instead</button>
            : <>
              <label className="small">{source.list ? "Or paste a link" : `${source.name} link`}
                <input autoFocus={!source.list} value={label ? "" : value} spellCheck={false} placeholder={source.placeholder}
                  onChange={(e) => { setValue(e.target.value); setLabel(undefined); }}
                  onKeyDown={(e) => { if (e.key === "Enter" && value.trim()) onSave(value.trim(), label); }} /></label>
              {source.help && <p className="dim small">{source.help}</p>}
            </>}
          {source.id === "sheets" && <div className="source-private small">
            <span className="strong">Private sheet?</span> Keep it private: in Google Sheets choose File → Download → Microsoft Excel (.xlsx), then import that file.
            <button type="button" className="text-btn" onClick={onUseFile}>Choose the downloaded file</button>
          </div>}
          <p className="dim small">Pulling uses the network once; everything after runs offline on the local copy.</p>
        </>}
      <div className="row integration-modal-actions">
        {current && <button type="button" className="text-btn push-right" onClick={onRemove}>Remove</button>}
        <button type="button" className="btn" onClick={onClose}>Cancel</button>
        {local
          ? <button type="button" className="btn primary" onClick={onReplace}>Choose another</button>
          : <button type="button" className="btn primary" disabled={!value.trim()} onClick={() => onSave(value.trim(), label)}>{current ? "Save" : "Add"}</button>}
      </div>
    </div>
  </div>, document.body);
}

/** Lists fetched this session, by source: reopening a dialog shows them without asking again. */
const listed = new Map<SourceId, SourceChoice[]>();

/** What a connected integration can pull. Asking the service is the person's click (network);
    Productboard's two choices are known without asking. */
function ChoiceList({ source, value, onPick, onOpen, onListed }: {
  source: Source; value: string; onPick: (c: SourceChoice) => void; onOpen: (c: SourceChoice) => void; onListed: (n: number) => void;
}) {
  const local = source.id === "productboard";
  const [items, setItems] = useState<SourceChoice[] | undefined>(listed.get(source.id));
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [filter, setFilter] = useState("");
  const flicker = useFlickerDelays();
  const noun = source.list!.noun;
  // Like the table view: the bottom fades while more of the list is below.
  const rows = useRef<HTMLUListElement>(null);
  const [more, setMore] = useState(false);
  const measure = () => { const el = rows.current; setMore(!!el && el.scrollTop < el.scrollHeight - el.clientHeight - 2); };
  // The chosen row's light and outline flicker in; the one it replaces flickers out.
  const [leaving, setLeaving] = useState<string>();
  const last = useRef(value);
  useEffect(() => {
    if (last.current === value) return;
    setLeaving(last.current); last.current = value;
    const t = setTimeout(() => setLeaving(undefined), 160);
    return () => clearTimeout(t);
  }, [value]);

  const load = async () => {
    setBusy(true); setError(undefined);
    try {
      const got = await api.sourceChoices(source.id);
      listed.set(source.id, got); setItems(got); onListed(got.length);
    } catch (e) { setError(String(e)); onListed(0); } finally { setBusy(false); }
  };
  useEffect(() => { if (local && !items) void load(); }, []); // eslint-disable-line react-hooks/exhaustive-deps

  const shown = useMemo(() => {
    const q = filter.trim().toLowerCase();
    return (items ?? []).filter((c) => !q || `${c.title} ${c.group ?? ""}`.toLowerCase().includes(q));
  }, [items, filter]);
  useEffect(measure, [shown]);
  const groups = new Set((items ?? []).map((c) => c.group ?? "")).size > 1 && source.id !== "productboard";

  if (!items) return <div className="choice-ask flicker-in" style={flicker("ask")}>
    {busy ? <Shimmer text={`Asking ${source.name} for your ${noun}…`} />
      : <><button type="button" className="btn" onClick={() => void load()}>Show my {source.name} {noun}</button>
        <span className="dim small">Asks {source.name} once for the {noun} your token can read.</span></>}
    {error && <p className="err small" role="alert">{error}</p>}
  </div>;

  const meta = (c: SourceChoice) => [
    c.count !== undefined && source.list!.counts ? `${c.count.toLocaleString()} ${source.list!.counts}` : null,
    c.edited ? `edited ${ago(Date.parse(c.edited))}` : null,
  ].filter(Boolean).join(" · ");

  return <div className="choice-list stack gap-s">
    <div className="choice-list-head">
      {items.length > 8
        ? <input className="choice-filter flicker-in" style={flicker("filter")} value={filter} autoFocus spellCheck={false} placeholder={`Filter ${items.length} ${noun}`} onChange={(e) => setFilter(e.target.value)} />
        : <span className="eyebrow flicker-in" style={flicker("head")}>{items.length ? `Your ${noun}` : `No ${noun}`}</span>}
      {!local && <button type="button" className="choice-refresh flicker-in" style={flicker("refresh")} title="Ask again" aria-label={`Ask ${source.name} again`} disabled={busy} onClick={() => void load()}>
        <RotateCw size={13} strokeWidth={1.75} aria-hidden="true" /></button>}
    </div>
    {items.length === 0 && <p className="dim small">{source.name} lists no {noun} for this token.
      {source.id === "notion" && " Share a database with your integration in Notion (••• → Connections), then ask again."}</p>}
    {shown.length > 0 && <ul ref={rows} onScroll={measure} className={`choice-rows${more ? " fb" : ""}`} role="listbox" aria-label={`${source.name} ${noun}`}>
      {shown.map((c, i) => {
        const on = c.spec === value;
        const head = groups && c.group !== shown[i - 1]?.group;
        return <li key={c.spec} role="presentation">
          {head && <div className="choice-group eyebrow flicker-in" style={flicker(`group:${c.group}`)}>{c.group}</div>}
          <button type="button" role="option" aria-selected={on} className={`choice-row flicker-in${c.nested ? " nested" : ""}${on ? " on" : ""}${c.spec === leaving ? " leaving" : ""}`} style={flicker(c.spec)}
            onClick={() => onPick(c)} onDoubleClick={() => onOpen(c)}>
            <span className="choice-dot" aria-hidden="true" />
            <span className="choice-title" title={c.title}>{c.nested && !filter.trim() ? c.title.split(" / ").pop() : c.title}</span>
            <span className="choice-meta">{!groups && c.group && source.id === "productboard" ? c.group : meta(c)}</span>
          </button>
        </li>;
      })}
    </ul>}
    {items.length > 0 && shown.length === 0 && <p className="dim small">Nothing matches “{filter}”.</p>}
    {error && <p className="err small" role="alert">{error}</p>}
  </div>;
}
