import { Fragment, useEffect, useMemo, useState } from "react";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { api, fmt, inTauri, pickSource, ROLES, type ImportForm, type ImportOutcome, type Preview, type Role, type SourceInfo } from "../api";
import { ErrorLine, useAction, type Ctx } from "../App";
import { JobProgress, isRunning, useJobs } from "../jobs";
import { DownloadConsent } from "../models";

const STEPS = ["Source", "Columns", "Roles", "Preview", "Import"];
type Step = 0 | 1 | 2 | 3;

interface Rule { value: string; role: Role }

/** Guess a column for a field from common names. */
function guess(columns: string[], names: string[]) {
  const lower = columns.map((c) => c.toLowerCase());
  for (const n of names) { const i = lower.indexOf(n); if (i >= 0) return columns[i]; }
  for (const n of names) { const i = lower.findIndex((c) => c.includes(n)); if (i >= 0) return columns[i]; }
  return "";
}

/** Add or update a collection: source → columns → roles → preview → import, then indexing
 *  starts by itself in the backend. */
export default function Import({ ctx, onDone, onCancel, initialPath }: { ctx: Ctx; onDone: (collection: string) => void; onCancel: () => void; initialPath?: string }) {
  const jobs = useJobs();
  const [step, setStep] = useState<Step>(0);
  const [collection, setCollection] = useState("");
  const [path, setPath] = useState("");
  const [source, setSource] = useState<SourceInfo>();
  const [map, setMap] = useState({ text: "", title: "", id: "", date: "", url: "" });
  const [chunk, setChunk] = useState({ mode: "tokens", tokens: 448, tokenOverlap: 48, chars: 2000, overlap: 200 });
  const [defaultRole, setDefaultRole] = useState<Role>("neutral");
  const [roleColumn, setRoleColumn] = useState("");
  const [rules, setRules] = useState<Rule[]>([]);
  const [counts, setCounts] = useState<Record<Role, number>>();
  const [preview, setPreview] = useState<Preview>();
  const [outcome, setOutcome] = useState<ImportOutcome>();
  const report = outcome?.report;
  const [dragging, setDragging] = useState(false);
  const { busy, error, setError, run } = useAction();

  const look = (p: string) => run(async () => {
    const s = await api.inspect(p);
    setPath(p); setSource(s); setPreview(undefined); setOutcome(undefined);
    const c = s.columns;
    setMap({
      text: guess(c, ["abstract", "text", "body", "content", "description", "summary"]),
      title: guess(c, ["title", "name", "headline"]),
      id: guess(c, ["id", "pmid", "doi", "key", "uid"]),
      date: guess(c, ["date", "pub_date", "published", "year", "created"]),
      url: guess(c, ["url", "link", "href", "doi"]),
    });
    setRoleColumn(guess(c, ["pub_types", "publication_types", "type", "kind", "role"]));
    if (!collection) setCollection(p.split("/").pop()!.replace(/\.[^.]+$/, "").replace(/[^A-Za-z0-9_-]+/g, "-").toLowerCase());
    return true;
  });
  // A dataset picked from the circle goes straight to the column step.
  useEffect(() => { if (initialPath) look(initialPath).then((ok) => { if (ok) setStep(1); }); }, [initialPath]);
  const choose = async (folder: boolean) => {
    const p = await pickSource(folder).catch((e) => { setError(String(e)); return null; });
    if (p) look(p);
  };

  // Paths of files dropped on the window (Tauri's native drag and drop).
  useEffect(() => {
    if (!inTauri) return;
    const un = getCurrentWebview().onDragDropEvent((e) => {
      if (e.payload.type === "enter" || e.payload.type === "over") setDragging(true);
      else if (e.payload.type === "leave") setDragging(false);
      else if (e.payload.type === "drop") { setDragging(false); if (step === 0 && e.payload.paths[0]) look(e.payload.paths[0]); }
    });
    return () => { un.then((f) => f()); };
  }, [step]);

  const folder = source?.kind === "folder";
  const form: ImportForm = {
    collection, source_path: path, text: [map.text], title: map.title, id: map.id, date: map.date, url: map.url,
    role: defaultRole, role_column: rules.length ? roleColumn : "",
    role_rules: rules.filter((r) => r.value.trim()).map((r) => `${r.value.trim()}=${r.role}`).join(","),
    chunk_tokenizer: chunk.mode === "tokens" ? "bge-small-en-v1.5" : null,
    chunk_tokens: chunk.tokens, chunk_token_overlap: chunk.tokenOverlap,
    chunk_chars: chunk.chars, chunk_overlap: chunk.overlap, limit: null,
  };
  const formKey = JSON.stringify(form);

  // Live role counts over the first rows while rules change.
  useEffect(() => {
    if (step !== 1 || !source || (!folder && !map.text)) return;
    const timer = setTimeout(() => {
      api.preview(form, 1000).then((p) => {
        const c = { demand: 0, coverage: 0, neutral: 0 };
        p.rows.forEach((r) => { c[r.role] += 1; });
        setCounts(c);
      }).catch(() => setCounts(undefined));
    }, 300);
    return () => clearTimeout(timer);
  }, [step, formKey]);

  const sampleOf = (col: string) => {
    const i = source?.columns.indexOf(col) ?? -1;
    return i >= 0 ? source?.sample[0]?.[i] ?? "" : "";
  };
  const validName = /^[A-Za-z0-9_-]+$/.test(collection);
  const exists = ctx.project.collections.some((c) => c.collection_id === collection);
  const total = counts ? counts.demand + counts.coverage + counts.neutral : 0;
  const rulesValues = useMemo(() => {
    // Distinct values seen in the role column (list cells split), to suggest rule values.
    const i = source?.columns.indexOf(roleColumn) ?? -1;
    if (i < 0 || !source) return [];
    const seen = new Set<string>();
    source.sample.forEach((row) => (row[i] ?? "").replace(/^\[|\]$/g, "").split(/[;,]/).forEach((v) => { const t = v.trim().replace(/^'|'$/g, ""); if (t) seen.add(t); }));
    return [...seen].slice(0, 12);
  }, [source, roleColumn]);

  const doPreview = () => run(async () => { setPreview(await api.preview(form, 20)); setStep(2); });
  const doImport = () => run(async () => {
    const out = await api.importCollection(form);
    if (out.job) jobs.started(out.job);
    setOutcome(out); setStep(3);
    await ctx.refresh();
  });
  const indexJob = outcome?.job && (jobs.current?.id === outcome.job.id ? jobs.current : outcome.job);

  const pill = (i: number) => {
    const at = step === 0 ? [0] : step === 1 ? [1, 2] : step === 2 ? [3] : [4];
    const done = i < Math.min(...at);
    return <li key={i} className={`step ${at.includes(i) ? "on" : done ? "done" : ""}`}>{i + 1} {STEPS[i]}{done ? " ✓" : ""}</li>;
  };

  return <div className="page narrow">
    <div className="title-row"><h2 className="h1">Add to Dataset</h2><span className="label">into {ctx.project.name}</span></div>
    <ol aria-label="Steps" className="steps">{STEPS.map((_, i) => pill(i))}</ol>
    <ErrorLine error={error} />

    {step === 0 && <>
      <label className="w-420">Collection name
        <input value={collection} onChange={(e) => setCollection(e.target.value)} />
        <span className={`small ${collection && !validName ? "error" : "muted"}`}>
          {exists ? "This collection exists: importing again updates changed rows and adds new ones." : "Letters, digits, - and _. One project can hold several collections."}
        </span>
      </label>
      <section aria-label="Choose data" className={`dropzone ${dragging ? "over" : ""}`}>
        <svg aria-hidden="true" width="40" height="40" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" strokeLinejoin="round"><path d="M12 16V4" /><path d="M7 9l5-5 5 5" /><path d="M4 16v3a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-3" /></svg>
        <div className="strong big">Drop a file or folder here</div>
        <div className="muted">CSV or TSV, Parquet, or a folder of .txt / .md files</div>
        <div className="row wrap center">
          <button type="button" className="btn primary" disabled={busy} onClick={() => choose(false)}>Choose file…</button>
          <button type="button" className="btn" disabled={busy} onClick={() => choose(true)}>Choose folder…</button>
        </div>
        <div className="row wrap center w-full">
          <input aria-label="Path to a file or folder" className="grow" value={path} onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => e.key === "Enter" && path.trim() && look(path.trim())} placeholder="/Users/you/data/papers.csv" />
          <button type="button" className="btn" disabled={!path.trim() || busy} onClick={() => look(path.trim())}>{busy ? "Reading…" : "Use this path"}</button>
        </div>
      </section>
      {source && <div className="panel row wrap">
        <span className="strong">{path.split("/").pop()}</span>
        <span className="muted">{source.kind.toUpperCase()} · {source.rows_read >= 1000 ? "1,000+" : fmt(source.rows_read)} {folder ? "files" : "rows"} · {source.columns.length} columns{source.unreadable ? ` · ${source.unreadable} unreadable` : ""}</span>
      </div>}
      <div className="row end">
        <button type="button" className="btn" onClick={onCancel}>Cancel</button>
        <button type="button" className="btn primary" disabled={!source || !validName} onClick={() => setStep(1)}>Next</button>
      </div>
    </>}

    {step === 1 && source && <>
      <div className="panel row wrap">
        <span className="strong">{path.split("/").pop()}</span>
        <span className="muted">{source.kind.toUpperCase()} · into “{collection}”</span>
        <button type="button" className="link push" onClick={() => setStep(0)}>Change source</button>
      </div>
      {folder
        ? <section className="panel"><h2>Columns</h2><p className="muted">Each file becomes one item: its name is the title, its text is searched, and its path is kept.</p></section>
        : <section aria-labelledby="cols" className="panel stack">
          <h2 id="cols">Which column holds what?</h2>
          <div className="muted small">Only Text is required. We guessed from the column names.</div>
          <div className="scroll-x">
            <div className="mapping">
              <div className="th">Field</div><div className="th">Column</div><div className="th sample">Sample from row 1</div>
              {([["text", "Text"], ["title", "Title"], ["id", "ID"], ["date", "Date"], ["url", "URL"]] as const).map(([k, label]) => <Fragment key={k}>
                <label htmlFor={`m-${k}`} className="strong">{label}{k === "text" && <span className="req"> *</span>}</label>
                <select id={`m-${k}`} value={map[k]} onChange={(e) => setMap({ ...map, [k]: e.target.value })}>
                  <option value="">{k === "text" ? "Choose a column" : k === "id" ? "Row number" : "None"}</option>
                  {source.columns.map((c) => <option key={c}>{c}</option>)}
                </select>
                <div className="muted clip sample">{map[k] ? sampleOf(map[k]) || "(empty)" : ""}</div>
              </Fragment>)}
            </div>
          </div>
          <div className="muted small">All other columns are kept as metadata.</div>
        </section>}
      <section className="panel">
        {exists ? <div className="muted small">This collection keeps its saved chunking settings.</div> : <details>
            <summary>Long items: {chunk.mode === "tokens" ? `${fmt(chunk.tokens)} tokens (${fmt(chunk.tokenOverlap)} overlap)` : `${fmt(chunk.chars)} characters (${fmt(chunk.overlap)} overlap)`}</summary>
            <label>Chunking<select value={chunk.mode} onChange={(e) => setChunk({ ...chunk, mode: e.target.value })}>
              <option value="tokens">Tokens (recommended)</option><option value="characters">Characters</option>
            </select></label>
            <div className="row wrap">
              {chunk.mode === "tokens" ? <>
                <label>Chunk above (tokens)<input type="number" min={1} value={chunk.tokens} onChange={(e) => setChunk({ ...chunk, tokens: Number(e.target.value) })} /></label>
                <label>Overlap (tokens)<input type="number" min={0} value={chunk.tokenOverlap} onChange={(e) => setChunk({ ...chunk, tokenOverlap: Number(e.target.value) })} /></label>
              </> : <>
              <label>Chunk above (characters, 0 = never)<input type="number" min={0} value={chunk.chars} onChange={(e) => setChunk({ ...chunk, chars: Number(e.target.value) })} /></label>
              <label>Overlap (characters)<input type="number" min={0} value={chunk.overlap} onChange={(e) => setChunk({ ...chunk, overlap: Number(e.target.value) })} /></label>
              </>}
            </div>
            <div className="muted small">Chunks are searched separately and roll up to their item in results. Token chunking requires BGE-small to be installed in Models.</div>
          </details>}
      </section>

      <section aria-labelledby="roles" className="panel stack gap">
        <div>
          <h2 id="roles">Roles</h2>
          <div className="muted small">Demand = something asking or needing. Coverage = something answering it. Gaps compare the two.</div>
        </div>
        <label className="inline">Everything else is
          <select value={defaultRole} onChange={(e) => setDefaultRole(e.target.value as Role)}>{ROLES.map((r) => <option key={r}>{r}</option>)}</select>
        </label>
        {!folder && <>
          <label className="inline">Rules read the column
            <select value={roleColumn} onChange={(e) => setRoleColumn(e.target.value)}>
              <option value="">Choose a column</option>
              {source.columns.map((c) => <option key={c}>{c}</option>)}
            </select>
          </label>
          {rules.map((r, i) => <div key={i} className="rule">
            <span>If it is or lists</span>
            <input aria-label={`Rule ${i + 1} value`} list="rule-values" value={r.value} onChange={(e) => setRules(rules.map((x, j) => j === i ? { ...x, value: e.target.value } : x))} />
            <span>→</span>
            <select aria-label={`Rule ${i + 1} role`} value={r.role} onChange={(e) => setRules(rules.map((x, j) => j === i ? { ...x, role: e.target.value as Role } : x))}>
              {ROLES.map((x) => <option key={x}>{x}</option>)}
            </select>
            <button type="button" className="icon push" aria-label={`Remove rule ${i + 1}`} onClick={() => setRules(rules.filter((_, j) => j !== i))}>✕</button>
          </div>)}
          <datalist id="rule-values">{rulesValues.map((v) => <option key={v} value={v} />)}</datalist>
          <button type="button" className="btn dashed start" disabled={!roleColumn} onClick={() => setRules([...rules, { value: "", role: "coverage" }])}>+ Add rule</button>
          <div className="muted small">Matching ignores case; for list values such as “Journal Article; Review” any element can match. The first matching rule wins.</div>
        </>}
        {counts && total > 0 && <div aria-label="Role counts" className="stack gap-s">
          <div className="rolebar">
            <div className="demand" style={{ width: `${(counts.demand / total) * 100}%` }} />
            <div className="coverage" style={{ width: `${(counts.coverage / total) * 100}%` }} />
            <div className="neutral grow" />
          </div>
          <div className="row wrap small">
            <span><b>{fmt(counts.demand)}</b> demand</span><span><b>{fmt(counts.coverage)}</b> coverage</span><span><b>{fmt(counts.neutral)}</b> neutral</span>
            <span className="muted">in the first {fmt(total)} rows · updates as rules change</span>
          </div>
        </div>}
      </section>
      <div className="row end">
        <button type="button" className="btn" onClick={() => setStep(0)}>Back</button>
        <button type="button" className="btn primary" disabled={(!folder && !map.text) || busy} onClick={doPreview}>{busy ? "Reading…" : "Preview"}</button>
      </div>
    </>}

    {step === 2 && preview && <>
      <section aria-labelledby="pv" className="panel flush">
        <div className="panel-head"><h2 id="pv">Preview · first {preview.rows.length} items</h2>
          {Object.keys(preview.skipped).length > 0 && <span className="muted small">skipped: {Object.entries(preview.skipped).map(([k, v]) => `${v} ${k}`).join(", ")}</span>}</div>
        <div className="scroll-x">
          <table className="table">
            <thead><tr><th>Role</th><th>Title</th><th>Date</th><th>Text</th><th className="right">Chunks</th></tr></thead>
            <tbody>{preview.rows.map((r, i) => <tr key={r.source_id}>
              <td><span className={`chip role ${r.role}`}>{r.role}</span></td>
              <td className="strong">{r.title ?? <span className="muted">{r.source_id}</span>}</td>
              <td className="muted nowrap">{r.date ?? (r.unparsed_date ? <span className="warn-text" title="Could not read this date">{r.unparsed_date}?</span> : "")}</td>
              <td className="muted clip w-320">{r.text.slice(0, 200)}</td>
              <td className="right">{preview.chunk_counts[i]}</td>
            </tr>)}</tbody>
          </table>
        </div>
      </section>
      <div className="row end">
        <button type="button" className="btn" onClick={() => setStep(1)}>Back</button>
        <button type="button" className="btn primary" disabled={busy} onClick={doImport}>{busy ? "Importing…" : `Import into “${collection}”`}</button>
      </div>
    </>}

    {step === 3 && report && <section aria-labelledby="res" className="panel stack gap">
      <h2 id="res">Import finished</h2>
      <div className="stats">
        <div className="stat"><b>{fmt(report.inserted)}</b><span>added</span></div>
        <div className="stat"><b>{fmt(report.updated)}</b><span>updated</span></div>
        <div className="stat"><b>{fmt(report.unchanged)}</b><span>unchanged</span></div>
        <div className="stat warnish"><b>{fmt(Object.values(report.skipped).reduce((a, b) => a + b, 0))}</b><span>skipped</span></div>
      </div>
      <div className="muted small">{fmt(report.items)} items in “{report.collection}” · {fmt(report.chunks_written)} chunks written{report.not_in_source ? ` · ${fmt(report.not_in_source)} earlier items not in this source were kept` : ""}</div>
      {Object.keys(report.skipped).length > 0 && <details><summary>Why items were skipped</summary>
        <ul>{Object.entries(report.skipped).map(([k, v]) => <li key={k}>{fmt(v)}: {k}</li>)}</ul></details>}
      {report.warnings.map((w) => <p key={w} className="note">{w}</p>)}
      <div className="muted small">Importing the same source again later only updates what changed.</div>
      <div className="label">Indexing</div>
      {indexJob && <div className="activity">
        <div className="small"><b>{isRunning(indexJob) ? "Indexing" : indexJob.state === "completed" ? "Indexed" : indexJob.state === "cancelled" ? "Indexing cancelled" : "Indexing failed"}</b> with {indexJob.models.join(", ")}</div>
        {isRunning(indexJob) && <JobProgress job={indexJob} />}
        {indexJob.state === "failed" && <div className="error small">{indexJob.error}</div>}
        {isRunning(indexJob) && <div className="muted small">It runs in the background; you can leave this page.</div>}
      </div>}
      {outcome!.index_error && <p className="error">Imported, but indexing did not start: {outcome!.index_error}. Retry it from the collection.</p>}
      {!outcome!.job && !outcome!.index_error && outcome!.plan.missing.length > 0 && <div className="stack gap-s">
        <p className="note">Imported, but not indexed: no suitable embedding model is installed. Search, regions and gaps need an index.</p>
        {outcome!.plan.missing.map((m) => <DownloadConsent key={m.key} model={m} project={ctx.project} />)}
        <div className="muted small">After the download, index it from Collections.</div>
      </div>}
      {outcome!.plan.notes.map((n) => <div key={n} className="muted small">{n}</div>)}
      <div className="row end">
        <button type="button" className="btn" onClick={() => { setStep(0); setSource(undefined); setPath(""); setOutcome(undefined); setError(undefined); }}>Import another</button>
        <button type="button" className="btn primary" onClick={() => onDone(report.collection)}>Go to the collection</button>
      </div>
    </section>}
  </div>;
}
