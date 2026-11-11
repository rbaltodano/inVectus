// Where data comes from: a file, a folder, or an integration whose token is saved. Used by the
// New project screen and by Add to Dataset. Several sources can be chosen; each chosen tile is
// highlighted in the accent colour. Link-based sources ask for their link in a dialog.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, FileSpreadsheet, FolderOpen } from "lucide-react";
import { api, inTauri, pickSource } from "./api";
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
}

export const SOURCES: Source[] = [
  { id: "file", name: "File", hint: "CSV, TSV, Parquet or Excel" },
  { id: "folder", name: "Folder", hint: "A folder of .txt or .md files" },
  { id: "sheets", name: "Google Sheets", hint: "A sheet shared by link", placeholder: "https://docs.google.com/spreadsheets/d/…", mark: "sheets",
    help: "Paste a link to a sheet shared as “Anyone with the link”." },
  { id: "notion", name: "Notion", hint: "A database", token: "notion", placeholder: "https://www.notion.so/…", mark: "notion",
    help: "The database must be shared with your Notion integration." },
  { id: "zotero", name: "Zotero", hint: "A library or collection", token: "zotero", placeholder: "zotero.org link, or zotero:library", mark: "zotero",
    help: "A zotero.org library or collection link, or zotero:library for your whole library." },
  { id: "airtable", name: "Airtable", hint: "A table or view", token: "airtable", placeholder: "https://airtable.com/app…", mark: "airtable",
    help: "A link to a table or a view." },
  { id: "typeform", name: "Typeform", hint: "Completed responses", token: "typeform", placeholder: "The form's share or edit link", mark: "typeform",
    help: "Only completed responses are pulled." },
  { id: "tally", name: "Tally", hint: "Completed submissions", token: "tally", placeholder: "https://tally.so/r/…", mark: "tally",
    help: "The form's share link. Only completed submissions are pulled." },
  { id: "canny", name: "Canny", hint: "Feature requests", token: "canny", placeholder: "canny:all, or canny:board:Name", mark: "canny",
    help: "canny:all for every board, or canny:board: and the board's name." },
  { id: "productboard", name: "Productboard", hint: "Notes or features", token: "productboard", placeholder: "productboard:notes or productboard:features", mark: "productboard",
    help: "productboard:notes for customer notes, or productboard:features for features." },
];

/** A chosen source: what Import reads (a path, a link or a spec). */
export interface Picked { id: SourceId; path: string }

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
  const set = (id: SourceId, path: string | null) => onChange(path ? [...picked.filter((p) => p.id !== id), { id, path }] : picked.filter((p) => p.id !== id));

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
            <span className="np-source-hint">{p ? (isLocal(s.id) ? baseName(p.path) : shortLink(p.path)) : s.hint}</span>
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
      onSave={(path) => { set(editing.id, path); setEditing(undefined); }}
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
  source: Source; current?: string; onSave: (path: string) => void; onReplace: () => void; onRemove: () => void; onClose: () => void; onUseFile: () => void;
}) {
  const [value, setValue] = useState(current ?? "");
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
          <label className="small">{source.name} link
            <input autoFocus value={value} spellCheck={false} placeholder={source.placeholder} onChange={(e) => setValue(e.target.value)}
              onKeyDown={(e) => { if (e.key === "Enter" && value.trim()) onSave(value.trim()); }} /></label>
          {source.help && <p className="dim small">{source.help}</p>}
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
          : <button type="button" className="btn primary" disabled={!value.trim()} onClick={() => onSave(value.trim())}>{current ? "Save" : "Add"}</button>}
      </div>
    </div>
  </div>, document.body);
}
