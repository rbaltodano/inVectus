// Import's table view: the file's first rows as one rounded table that fades out where it goes
// on. Everything is imported; "Choose searched columns" decides which columns are searched (the
// import's text columns, joined), and each column's label can set it as title, ID, date, URL or
// the column role rules read. Content flickers in along the diagonal from the top left; when
// choosing, only searched columns keep their fill, which flickers in and out per cell.
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { createPortal } from "react-dom";
import { fmt, type Role } from "./api";
import { DOT_DELAY_MS } from "./dot-flicker";
import Select from "./Select";
import { proseColumns } from "./table-columns";
export { proseColumns };
import "./TablePicker.css";

export type ColumnRole = "searched" | "title" | "id" | "date" | "url" | "roles" | "kept";
export interface Assignment { searched: string[]; title: string; id: string; date: string; url: string; roles: string }

const ROLE_LABEL: Record<ColumnRole, string> = { searched: "Searched", title: "Title", id: "ID", date: "Date", url: "URL", roles: "Roles", kept: "Kept" };
const ROLE_NOTE: Record<ColumnRole, string> = {
  searched: "What search and gaps read", title: "Shown in results", id: "Keeps items matched on updates",
  date: "Used for gap timelines", url: "Opens the source", roles: "Rules decide demand / coverage", kept: "Imported, not searched",
};

export function roleOf(col: string, a: Assignment): ColumnRole {
  if (a.searched.includes(col)) return "searched";
  for (const k of ["title", "id", "date", "url", "roles"] as const) if (a[k] === col) return k;
  return "kept";
}

/** Give `col` a role; single roles (title, ID…) move off the column that had them. */
export function assign(col: string, role: ColumnRole, a: Assignment): Assignment {
  const next: Assignment = { ...a, searched: a.searched.filter((c) => c !== col) };
  for (const k of ["title", "id", "date", "url", "roles"] as const) if (next[k] === col) next[k] = "";
  if (role === "searched") next.searched = [...next.searched, col];
  else if (role !== "kept") next[role] = col;
  return next;
}

export interface RolesSummary { column: string; rules: { value: string; role: Role }[]; defaultRole: Role }

export default function TablePicker({ fileName, columns, sample, rowsRead, assignment, onChange, onImport, onList, onRoles, onCancel, busy, roles, counts, stats, existing }: {
  fileName: string; columns: string[]; sample: (string | null)[][]; rowsRead: number;
  assignment: Assignment; onChange: (a: Assignment) => void;
  onImport: () => void; onList: () => void; onRoles: () => void; onCancel: () => void; busy: boolean;
  roles: RolesSummary; counts?: Record<Role, number>;
  /** The engine's preview over the first rows: rows read, and why it would skip some. */
  stats?: { read: number; skipped: Record<string, number> };
  /** Importing into a collection that already exists. */
  existing?: string;
}) {
  const [choosing, setChoosing] = useState(false);
  const [hover, setHover] = useState<number>();
  // Columns whose fill should animate (changed since the table opened); others show still.
  const [animated, setAnimated] = useState<Set<string>>(new Set());
  const scroll = useRef<HTMLDivElement>(null);
  const [fades, setFades] = useState({ bottom: false, right: false });
  const rows = sample.slice(0, 50);
  // Delays: content enters along the diagonal; fills flicker per cell at random.
  const enter = useMemo(() => rows.map((_, r) => columns.map((_, c) => 120 + (r + 1 + c + 1) * 28 + Math.random() * 40)), [rows.length, columns.length]);
  const fillDelay = useMemo(() => rows.map(() => columns.map(() => Math.random() * DOT_DELAY_MS)), [rows.length, columns.length]);
  const rowsLabel = rowsRead >= 1000 ? "all rows" : `all ${fmt(rowsRead)} rows`;

  const measure = () => {
    const el = scroll.current;
    if (!el) return;
    setFades({ bottom: el.scrollTop < el.scrollHeight - el.clientHeight - 2, right: el.scrollLeft < el.scrollWidth - el.clientWidth - 2 });
  };
  useEffect(() => { measure(); const ro = new ResizeObserver(measure); if (scroll.current) ro.observe(scroll.current); return () => ro.disconnect(); }, []);
  useEffect(() => {
    // Escape closes the table and shows the list, keeping every choice.
    const key = (e: KeyboardEvent) => { if (e.key === "Escape" && !document.querySelector(".select-menu")) onList(); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [onList]);

  const setMode = (next: boolean) => { setChoosing(next); setAnimated(new Set(columns)); };
  const toggle = (col: string) => {
    if (!choosing) return;
    setAnimated((s) => new Set(s).add(col));
    onChange(assign(col, assignment.searched.includes(col) ? "kept" : "searched", assignment));
  };
  const fill = (col: string, r: number, c: number) => {
    const on = !choosing || assignment.searched.includes(col);
    const anim = animated.has(col);
    return { className: `tp-fill ${on ? (anim ? "in" : "on") : (anim ? "out" : "off")}`, style: anim ? { animationDelay: `${fillDelay[r]?.[c] ?? 0}ms` } : undefined };
  };
  const gridStyle = { gridTemplateColumns: `44px ${columns.map((c) => (c.length > 14 || proseLike(c) ? "minmax(200px, 320px)" : "minmax(110px, 180px)")).join(" ")}` } as CSSProperties;
  const n = assignment.searched.length;
  const neutralOnly = !roles.column || (!roles.rules.length && roles.defaultRole === "neutral") || (counts && counts.demand + counts.coverage === 0);
  const notes = checks(columns, sample, assignment, roles, counts, stats, existing);

  return createPortal(<div className="integration-modal-backdrop tp-backdrop" onClick={(e) => { if (e.target === e.currentTarget) onCancel(); }}>
    <div className="tp-modal" role="dialog" aria-modal="true" aria-labelledby="tp-title">
      <div className="tp-head">
        <div className="eyebrow">Import · {fileName} · {rowsRead >= 1000 ? "1,000+" : fmt(rowsRead)} rows · {columns.length} columns</div>
        <h3 id="tp-title">{choosing ? "Choose searched columns" : "Ready to import"}</h3>
        <p className="dim small">{choosing
          ? "Every column is imported. Click a column to search it or not; click its label to use it as the title, ID, date, URL or roles."
          : "Every row and column comes in. Searched now: " + (n ? assignment.searched.join(", ") : "nothing yet; choose at least one column") + "."}</p>
      </div>
      <div className="tp-wrap">
        <div ref={scroll} className={`tp-scroll${fades.bottom ? " fb" : ""}${fades.right ? " fr" : ""}`} onScroll={measure}>
          <div className="tp-grid" style={gridStyle} role="grid" aria-rowcount={rows.length + 1}>
            <div className="tp-c tp-h tp-rn"><span className="tp-v" style={{ animationDelay: "120ms" }}>#</span></div>
            {columns.map((col, c) => {
              const f = fill(col, 0, c), role = roleOf(col, assignment);
              return <div key={c} role="columnheader" className={`tp-c tp-h${choosing && hover === c ? " hover" : ""}`}
                tabIndex={choosing ? 0 : -1} aria-pressed={choosing ? assignment.searched.includes(col) : undefined}
                aria-label={choosing ? `${col}: ${assignment.searched.includes(col) ? "searched" : "not searched"}; press to change` : col}
                onKeyDown={(e) => { if (choosing && (e.key === "Enter" || e.key === " ") && e.target === e.currentTarget) { e.preventDefault(); toggle(col); } }}
                onPointerEnter={() => setHover(c)} onPointerLeave={() => setHover(undefined)} onClick={() => toggle(col)}>
                <div className={f.className} style={f.style} />
                <span className="tp-v" style={{ animationDelay: `${120 + (c + 1) * 28}ms` }} title={col}>{col}</span>
                {choosing && <span className={`tp-label${role === "searched" ? " s" : ""}`} onClick={(e) => e.stopPropagation()}>
                  <Select<ColumnRole> variant="bare" ariaLabel={`${col} holds`} menuTitle={`${col} holds…`} value={role}
                    onChange={(next) => { setAnimated((s) => new Set(s).add(col)); onChange(assign(col, next, assignment)); }}
                    options={(Object.keys(ROLE_LABEL) as ColumnRole[]).map((k) => ({ value: k, label: ROLE_LABEL[k], note: ROLE_NOTE[k] }))} />
                </span>}
              </div>;
            })}
            {rows.map((row, r) => <div key={r} className="tp-row" role="row">
              <div className="tp-c tp-rn"><span className="tp-v" style={{ animationDelay: `${enter[r][0] - 28}ms` }}>{r + 1}</span></div>
              {columns.map((col, c) => {
                const f = fill(col, r + 1, c);
                return <div key={c} role="gridcell" className={`tp-c${choosing && hover === c ? " hover" : ""}`}
                  onPointerEnter={() => setHover(c)} onPointerLeave={() => setHover(undefined)} onClick={() => toggle(col)}>
                  <div className={f.className} style={f.style} />
                  <span className="tp-v" style={{ animationDelay: `${enter[r][c]}ms` }}>{row[c] ?? ""}</span>
                </div>;
              })}
            </div>)}
          </div>
        </div>
      </div>
      {notes.length > 0 && <ul className="tp-notes">
        {notes.map((note) => <li key={note.text} className={note.warn ? "warn" : undefined}>{note.text}
          {note.action === "roles" && <button type="button" className="text-btn" onClick={onRoles}>{neutralOnly ? "Set roles" : "Change"}</button>}</li>)}
      </ul>}
      <div className="tp-foot">
        <span className="dim small"><span className="tp-strong">{rowsRead >= 1000 ? "All rows" : `All ${fmt(rowsRead)} rows`}</span> · all {columns.length} columns · {n} searched</span>
        <span className="tp-actions">
          {choosing
            ? <button type="button" className="text-btn" onClick={() => onChange({ ...assignment, searched: proseColumns(columns, sample) })}>Search every text column</button>
            : <button type="button" className="text-btn" onClick={() => setMode(true)}>Choose searched columns</button>}
          {choosing && <button type="button" className="text-btn" onClick={() => setMode(false)}>Done choosing</button>}
          <button type="button" className="text-btn" onClick={onList}>Use the list instead</button>
          <button type="button" className="btn" onClick={onCancel}>Cancel</button>
          <button type="button" className="btn primary" disabled={!n || busy} title={n ? undefined : "Choose at least one searched column"} onClick={onImport}>
            {busy ? "Importing…" : `Import ${rowsLabel}${neutralOnly ? " without roles" : ""}`}</button>
        </span>
      </div>
    </div>
  </div>, document.body);
}

const proseLike = (name: string) => /abstract|text|body|content|description|summary|title|notes?|comment/i.test(name);

interface Note { text: string; warn?: boolean; action?: "roles" }

/** What the person should know before importing straight from the table. Counts come from the
 *  engine's preview of the first rows, so they are estimates for the whole file. */
function checks(columns: string[], sample: (string | null)[][], a: Assignment, roles: RolesSummary,
  counts: Record<Role, number> | undefined, stats: { read: number; skipped: Record<string, number> } | undefined, existing: string | undefined): Note[] {
  const notes: Note[] = [];
  const first = stats ? `the first ${fmt(stats.read)} rows` : "the first rows";
  // Roles: gap scans compare demand with coverage; all neutral means no gaps.
  if (!roles.column || (!roles.rules.length && roles.defaultRole === "neutral") || (counts && counts.demand + counts.coverage === 0)) {
    notes.push({ text: "Roles: every row would be neutral, so gap scans would find nothing. Gaps compare demand (what is asked) with coverage (what answers it).", warn: true, action: "roles" });
  } else {
    const cover = roles.rules.filter((r) => r.role === "coverage").map((r) => r.value);
    const demand = roles.rules.filter((r) => r.role === "demand").map((r) => r.value);
    const parts = [cover.length ? `${cover.join(", ")} → coverage` : "", demand.length ? `${demand.join(", ")} → demand` : "", `everything else → ${roles.defaultRole}`].filter(Boolean);
    notes.push({ text: `Roles from ${roles.column}: ${parts.join("; ")}${counts ? ` (in ${first}: ${fmt(counts.demand)} demand, ${fmt(counts.coverage)} coverage, ${fmt(counts.neutral)} neutral)` : ""}.`, action: "roles" });
  }
  // Rows whose searched columns are empty become no item.
  const empty = stats?.skipped["empty text"] ?? 0;
  if (empty > 0) notes.push({ text: `${fmt(empty)} of ${first} have nothing in the searched columns and would be skipped${a.title && !a.searched.includes(a.title) ? `; searching ${a.title} too would keep them` : ""}.`, warn: true });
  // The ID keeps items matched when the file is imported again.
  if (!a.id) notes.push({ text: "No ID column: items are numbered by row, so a later update matches them by position. Set an ID if rows may move." });
  else {
    const i = columns.indexOf(a.id);
    const vals = sample.map((r) => (r[i] ?? "").trim());
    const blank = vals.filter((v) => !v).length, dup = vals.length - new Set(vals.filter(Boolean)).size - blank;
    if (blank || dup) notes.push({ text: `ID ${a.id}: ${[blank ? `${blank} empty` : "", dup ? `${dup} repeated` : ""].filter(Boolean).join(", ")} in the first ${sample.length} rows; items with a repeated ID are merged.`, warn: true });
  }
  if (existing) notes.push({ text: `This updates the collection “${existing}”. Items whose searched text changes are indexed again.` });
  return notes;
}
