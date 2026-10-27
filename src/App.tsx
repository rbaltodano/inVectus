import { useEffect, useState, type ChangeEvent } from "react";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import "./App.css";

type Role = "demand" | "coverage" | "neutral";
interface Collection { collection_id: string; items: number; chunks: number; by_role: Record<string, number> }
interface TableInfo { collection: string; space_id: string; model_key: string; runtime: string; dimensions: number; origin: string; vectors: number; ann_partitions: number | null }
interface ProjectInfo { root: string; name: string; warning: string | null; collections: Collection[]; tables: TableInfo[] }
interface ModelInfo { key: string; repo: string; dimensions: number; megabytes: number; installed: boolean; default: boolean; note: string }
interface MappedRow { source_id: string; title: string | null; text: string; date: string | null; role: Role }
interface Preview { columns: string[]; rows: MappedRow[]; skipped: Record<string, number> }
interface ImportReport { collection: string; source_rows: number; items: number; inserted: number; updated: number; unchanged: number; chunks_written: number; skipped: Record<string, number>; warnings: string[] }
interface Item { item_id: number; source_id: string; title: string | null; url: string | null; date: string | null; role: Role }
interface Hit { rank: number; similarity: number; via_chunk: number | null; item: Item }
interface SearchResult { mode: string; exhaustive: boolean; vectors_compared: number; compatibility: string; model_key: string; hits: Hit[]; notes: string[]; embed_ms: number; retrieve_ms: number }
type EmbedEvent =
  | { status: "progress"; done: number; total: number }
  | { status: "finished"; report: { embedded: number; already_embedded: number; items_per_second: number; truncated: number; cancelled: boolean; vectors_in_table: number } }
  | { status: "failed"; error: string };

const inTauri = "__TAURI_INTERNALS__" in window;

export default function App() {
  const [version, setVersion] = useState<string>();
  const [path, setPath] = useState("");
  const [project, setProject] = useState<ProjectInfo>();
  const [error, setError] = useState<string>();

  useEffect(() => {
    if (!inTauri) return;
    invoke<string>("engine_version").then(setVersion).catch(() => undefined);
    invoke<string>("default_projects_dir").then((d) => setPath(`${d}/My project.invectus`));
  }, []);

  const run = async <T,>(f: () => Promise<T>) => {
    setError(undefined);
    try { return await f(); } catch (e) { setError(String(e)); }
  };
  const open = (create: boolean) => run(async () => setProject(await invoke<ProjectInfo>("open_project", { path, create })));
  const refresh = () => run(async () => setProject(await invoke<ProjectInfo>("project_info")));

  return <main>
    <p className="eyebrow">inVectus · engine {version ?? "(browser preview)"}</p>
    <h1>Find what’s missing.</h1>
    <p>P1 builds the collection and its nearest-neighbour search. Similarity here is evidence, not an absence verdict: calibrated inVectus search verdicts arrive in P2.</p>
    {!inTauri && <p className="warn">Open the desktop app to use the engine.</p>}
    {error && <p className="error" role="alert">{error}</p>}

    <section>
      <h2>Project</h2>
      <div className="row">
        <input aria-label="Project folder" value={path} onChange={(e) => setPath(e.target.value)} />
        <button onClick={() => open(false)}>Open</button>
        <button onClick={() => open(true)}>Create</button>
      </div>
      {project && <ProjectSummary p={project} />}
    </section>

    {project && <>
      <ImportPanel run={run} onDone={refresh} />
      <EmbedPanel project={project} onDone={refresh} />
      <SearchPanel project={project} run={run} />
    </>}
    <footer>Local first · network off · macOS · Windows · Linux</footer>
  </main>;
}

function ProjectSummary({ p }: { p: ProjectInfo }) {
  return <div>
    <p><strong>{p.name}</strong> — {p.root}</p>
    {p.warning && <p className="warn">{p.warning}</p>}
    {p.collections.length === 0 && <p>No collections yet.</p>}
    <ul>
      {p.collections.map((c) => <li key={c.collection_id}>
        <strong>{c.collection_id}</strong>: {c.items} items, {c.chunks} chunks · {Object.entries(c.by_role).map(([r, n]) => `${n} ${r}`).join(", ")}
        <ul>{p.tables.filter((t) => t.collection === c.collection_id).map((t) =>
          <li key={t.space_id}>{t.model_key} · space {t.space_id.slice(0, 12)} · {t.dimensions}d · {t.vectors} vectors · {t.origin}{t.ann_partitions ? ` · ANN ${t.ann_partitions} partitions` : ""}</li>)}
        </ul>
      </li>)}
    </ul>
  </div>;
}

type Runner = <T>(f: () => Promise<T>) => Promise<T | undefined>;

function ImportPanel({ run, onDone }: { run: Runner; onDone: () => void }) {
  const [f, setF] = useState({ collection: "papers", source_path: "", text: "abstract", title: "title", id: "id", date: "", url: "", role: "neutral" as Role, role_column: "", role_rules: "", chunk_chars: 2000, chunk_overlap: 200 });
  const [preview, setPreview] = useState<Preview>();
  const [report, setReport] = useState<ImportReport>();
  const form = { ...f, text: f.text.split(",").map((s) => s.trim()), limit: null };
  const set = (k: keyof typeof f) => (e: ChangeEvent<HTMLInputElement | HTMLSelectElement>) =>
    setF({ ...f, [k]: e.target.type === "number" ? Number(e.target.value) : e.target.value });
  const field = (k: keyof typeof f, label: string) =>
    <label>{label}<input value={String(f[k])} onChange={set(k)} type={typeof f[k] === "number" ? "number" : "text"} /></label>;

  return <section>
    <h2>Import</h2>
    <p>A folder of .txt/.md files, a CSV/TSV or a Parquet file. Re-importing the same source is a no-op; changed rows are updated.</p>
    <div className="grid">
      {field("source_path", "Source path")}
      {field("collection", "Collection")}
      {field("text", "Text column(s)")}
      {field("title", "Title column")}
      {field("id", "Id column")}
      {field("date", "Date column")}
      {field("url", "URL column")}
      <label>Role (constant or default)<select value={f.role} onChange={set("role")}>
        <option>neutral</option><option>demand</option><option>coverage</option></select></label>
      {field("role_column", "Role column (optional)")}
      {field("role_rules", "Role rules, e.g. Systematic Review=coverage")}
      {field("chunk_chars", "Chunk above (chars, 0 = never)")}
      {field("chunk_overlap", "Chunk overlap (chars)")}
    </div>
    <div className="row">
      <button onClick={() => run(async () => setPreview(await invoke<Preview>("preview_import", { form })))}>Preview 20</button>
      <button onClick={() => run(async () => { setReport(await invoke<ImportReport>("run_import", { form })); onDone(); })}>Import</button>
    </div>
    {preview && <div>
      <p className="muted">Columns: {preview.columns.join(", ")}{Object.keys(preview.skipped).length ? ` · skipped ${JSON.stringify(preview.skipped)}` : ""}</p>
      <table><thead><tr><th>id</th><th>role</th><th>title</th><th>date</th><th>text</th></tr></thead>
        <tbody>{preview.rows.map((r) => <tr key={r.source_id}><td>{r.source_id}</td><td>{r.role}</td><td>{r.title}</td><td>{r.date}</td><td>{r.text.slice(0, 120)}…</td></tr>)}</tbody></table>
    </div>}
    {report && <p>{report.collection}: {report.items} items (+{report.inserted} new, {report.updated} updated, {report.unchanged} unchanged), {report.chunks_written} chunks{Object.keys(report.skipped).length ? `; skipped ${JSON.stringify(report.skipped)}` : ""}{report.warnings.map((w) => <span key={w} className="warn"><br />{w}</span>)}</p>}
  </section>;
}

function EmbedPanel({ project, onDone }: { project: ProjectInfo; onDone: () => void }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [collection, setCollection] = useState(project.collections[0]?.collection_id ?? "");
  const [model, setModel] = useState("");
  const [state, setState] = useState<EmbedEvent>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    invoke<ModelInfo[]>("list_models").then((m) => { setModels(m); setModel(m.find((x) => x.default)?.key ?? m[0]?.key ?? ""); });
    const un = listen<EmbedEvent>("embed-event", (e) => { setState(e.payload); if (e.payload.status !== "progress") onDone(); });
    return () => { un.then((f) => f()); };
  }, []);
  const running = state?.status === "progress";
  const start = () => { setError(undefined); invoke("start_embed", { collection, model }).catch((e) => setError(String(e))); };
  return <section>
    <h2>Embed</h2>
    <p>Runs on the CPU, offline, from an installed model. Cancelling keeps finished batches; running again resumes and skips unchanged items.</p>
    <div className="row">
      <select aria-label="Collection" value={collection} onChange={(e) => setCollection(e.target.value)}>
        {project.collections.map((c) => <option key={c.collection_id}>{c.collection_id}</option>)}</select>
      <select aria-label="Model" value={model} onChange={(e) => setModel(e.target.value)}>
        {models.map((m) => <option key={m.key} value={m.key} disabled={!m.installed}>{m.key}{m.default ? " (default)" : ""}{m.installed ? "" : " — not installed"}</option>)}</select>
      <button disabled={running || !collection} onClick={start}>Embed</button>
      <button disabled={!running} onClick={() => invoke("cancel_job")}>Cancel</button>
    </div>
    {error && <p className="error">{error}</p>}
    {state?.status === "progress" && <p><progress value={state.done} max={Math.max(state.total, 1)} /> {state.done}/{state.total}</p>}
    {state?.status === "finished" && <p>{state.report.cancelled ? "Cancelled" : "Done"}: {state.report.embedded} embedded ({state.report.items_per_second.toFixed(1)}/s), {state.report.already_embedded} already embedded, {state.report.truncated} truncated; {state.report.vectors_in_table} vectors.</p>}
    {state?.status === "failed" && <p className="error">{state.error}</p>}
  </section>;
}

function SearchPanel({ project, run }: { project: ProjectInfo; run: Runner }) {
  const [collection, setCollection] = useState(project.tables[0]?.collection ?? "");
  const [query, setQuery] = useState("");
  const [role, setRole] = useState<Role | "">("");
  const [exhaustive, setExhaustive] = useState(true);
  const [result, setResult] = useState<SearchResult>();
  const go = () => run(async () => setResult(await invoke<SearchResult>("search", {
    form: { collection, query, k: 10, role: role || null, exhaustive, space: null },
  })));
  return <section>
    <h2>Nearest items</h2>
    <div className="row">
      <select aria-label="Collection" value={collection} onChange={(e) => setCollection(e.target.value)}>
        {[...new Set(project.tables.map((t) => t.collection))].map((c) => <option key={c}>{c}</option>)}</select>
      <input aria-label="Query" placeholder="Describe an idea…" value={query} onChange={(e) => setQuery(e.target.value)} onKeyDown={(e) => e.key === "Enter" && go()} />
      <select aria-label="Role filter" value={role} onChange={(e) => setRole(e.target.value as Role | "")}>
        <option value="">any role</option><option>coverage</option><option>demand</option><option>neutral</option></select>
      <label className="check"><input type="checkbox" checked={exhaustive} onChange={(e) => setExhaustive(e.target.checked)} /> exhaustive</label>
      <button onClick={go} disabled={!query.trim() || !collection}>Search</button>
    </div>
    {result && <div>
      <p className="muted">{result.mode} · {result.vectors_compared} vectors · {result.model_key} · {result.compatibility} · {result.embed_ms.toFixed(0)} + {result.retrieve_ms.toFixed(0)} ms</p>
      <ol>{result.hits.map((h) => <li key={h.item.item_id}>
        <span className="sim">{h.similarity.toFixed(3)}</span> <span className={`role ${h.item.role}`}>{h.item.role}</span>{" "}
        {h.item.url ? <a href={h.item.url} target="_blank" rel="noreferrer">{h.item.title ?? h.item.source_id}</a> : (h.item.title ?? h.item.source_id)}
        {h.item.date && <span className="muted"> · {h.item.date}</span>}
        {h.via_chunk !== null && <span className="muted"> · via chunk {h.via_chunk}</span>}
      </li>)}</ol>
      {result.notes.map((n) => <p key={n} className="muted">{n}</p>)}
    </div>}
  </section>;
}
