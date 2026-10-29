import { useEffect, useState } from "react";
import { api, inTauri, type ProjectInfo, type TableInfo, type TableRef } from "./api";
import { JobProgress, JobsProvider, useJobs } from "./jobs";
import Projects, { remember } from "./screens/Projects";
import Import from "./screens/Import";
import Index from "./screens/Index";
import Search from "./screens/Search";
import MapView from "./screens/Map";
import Gaps from "./screens/Gaps";
import GapDetailView from "./screens/GapDetail";
import Settings from "./screens/Settings";
import "./App.css";

export type Screen =
  | { name: "projects" } | { name: "import" } | { name: "index"; collection?: string }
  | { name: "search"; query?: string } | { name: "map"; region?: number }
  | { name: "gaps" } | { name: "gap"; id: number } | { name: "settings" };

const NAV: [Screen["name"], string][] = [
  ["projects", "Projects"], ["import", "Import"], ["index", "Index"], ["search", "Search"],
  ["map", "Map"], ["gaps", "Gaps"], ["settings", "Settings"],
];

/** What every screen gets from the shell. */
export interface Ctx {
  project: ProjectInfo;
  /** The selected collection's vector table, if it has one. */
  table?: TableInfo;
  tref?: TableRef;
  collection?: string;
  go: (s: Screen) => void;
  refresh: () => Promise<void>;
  setProject: (p: ProjectInfo) => void;
}

export const tableKey = (t: TableInfo) => `${t.collection}\u0000${t.space_id}`;

export default function App() {
  const [project, setProject] = useState<ProjectInfo>();
  const refresh = async () => { if (project) setProject(await api.info()); };
  return <JobsProvider onFinished={() => { api.info().then(setProject).catch(() => undefined); }}>
    <Shell project={project} setProject={setProject} refresh={refresh} />
  </JobsProvider>;
}

function Shell({ project, setProject, refresh }: { project?: ProjectInfo; setProject: (p: ProjectInfo) => void; refresh: () => Promise<void> }) {
  const [screen, setScreen] = useState<Screen>({ name: "projects" });
  const [selected, setSelected] = useState<string>();
  const [version, setVersion] = useState<string>();
  const [netError, setNetError] = useState<string>();
  const jobs = useJobs();

  useEffect(() => { if (inTauri) api.version().then(setVersion).catch(() => undefined); }, []);

  // Keep a valid table selected: the last one picked, else the first table, else none.
  const tables = project?.tables ?? [];
  const table = tables.find((t) => tableKey(t) === selected) ?? tables[0];
  const collection = table?.collection ?? project?.collections[0]?.collection_id;
  const tref = table ? { collection: table.collection, space: table.space_id } : undefined;

  const open = (p: ProjectInfo) => { setProject(p); remember(p); setSelected(undefined); setScreen(p.collections.length ? { name: "search" } : { name: "import" }); };
  const toggleOffline = async () => {
    if (!project) return;
    setNetError(undefined);
    try { setProject(await api.setNetwork(!project.network_enabled)); } catch (e) { setNetError(String(e)); }
  };

  const ctx: Ctx | undefined = project && { project, table, tref, collection, go: setScreen, refresh, setProject };
  const active = screen.name === "gap" ? "gaps" : screen.name;
  const job = jobs.current;

  return <div className="shell">
    <nav aria-label="Main" className="sidebar">
      <div className="brand">inVectus</div>
      {NAV.map(([name, label]) => {
        const disabled = name !== "projects" && name !== "settings" && !project;
        return <button key={name} type="button" className="nav-link" aria-current={active === name ? "page" : undefined}
          disabled={disabled} onClick={() => setScreen({ name } as Screen)}>{label}</button>;
      })}
      <section aria-label="Jobs" className="jobs">
        <div className="eyebrow">Jobs</div>
        {!job && <div className="muted small">No jobs running</div>}
        {job && <>
          <div className="small"><b>{job.label}</b> · {job.status === "running" ? "running" : job.status === "finished" ? (job.report?.cancelled ? "cancelled" : "done") : "failed"}</div>
          {job.status === "running" && <JobProgress job={job} />}
          {job.status === "failed" && <div className="error small">{job.message}</div>}
          <div className="row">
            {job.status === "running" && job.kind !== "download" && <button type="button" className="btn small" onClick={() => api.cancel()}>Cancel</button>}
            {job.status !== "running" && <button type="button" className="btn small" onClick={jobs.clear}>Dismiss</button>}
          </div>
        </>}
      </section>
      <div className="muted small version">engine {version ?? "(browser preview)"}</div>
    </nav>

    <main className="main">
      <header className="topbar">
        {project
          ? <div className="stack"><span className="muted small">Project</span><span className="strong">{project.name}</span></div>
          : <div className="strong muted">No project open</div>}
        <div className="topbar-right">
          {project && tables.length > 0 && <label className="inline">
            <span className="muted small">Collection</span>
            <select value={table ? tableKey(table) : ""} onChange={(e) => setSelected(e.target.value)}>
              {tables.map((t) => <option key={tableKey(t)} value={tableKey(t)}>{t.collection} · {t.model_key} · {t.dimensions}-d</option>)}
            </select>
          </label>}
          {project && <button type="button" className="pill" aria-pressed={!project.network_enabled} onClick={toggleOffline}
            title={project.offline_env ? "INVECTUS_OFFLINE is set: the network stays off whatever this says" : undefined}>
            Offline mode: {project.network_enabled && !project.offline_env ? "off" : "on"}
          </button>}
        </div>
      </header>
      {netError && <p className="error banner" role="alert">{netError}</p>}
      {!inTauri && <p className="warn banner">Open the desktop app to use the engine. This browser preview cannot reach it.</p>}
      {project?.warning && <p className="warn banner">{project.warning}</p>}

      <div className="content">
        {screen.name === "projects" && <Projects onOpen={open} current={project} />}
        {screen.name === "settings" && <Settings ctx={ctx} />}
        {ctx && screen.name === "import" && <Import ctx={ctx} />}
        {ctx && screen.name === "index" && <Index ctx={ctx} initial={screen.collection} />}
        {ctx && screen.name === "search" && <Search ctx={ctx} initial={screen.query} />}
        {ctx && screen.name === "map" && <MapView ctx={ctx} focus={screen.region} />}
        {ctx && screen.name === "gaps" && <Gaps ctx={ctx} />}
        {ctx && screen.name === "gap" && <GapDetailView ctx={ctx} id={screen.id} />}
      </div>
    </main>
  </div>;
}

/** Shown on screens that need something the selected collection lacks yet. */
export function Needs({ title, body, action, onAction }: { title: string; body: string; action?: string; onAction?: () => void }) {
  return <section className="card empty">
    <h2>{title}</h2>
    <p className="muted">{body}</p>
    {action && <button type="button" className="btn primary" onClick={onAction}>{action}</button>}
  </section>;
}

export function ErrorLine({ error }: { error?: string }) {
  return error ? <p className="error" role="alert">{error}</p> : null;
}

/** Run an async action with an error slot and a busy flag. */
export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const run = async <T,>(f: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true); setError(undefined);
    try { return await f(); } catch (e) { setError(String(e)); return undefined; } finally { setBusy(false); }
  };
  return { busy, error, setError, run };
}
