import { useEffect, useState } from "react";
import { api, inTauri, local, type JobInfo, type ProjectInfo, type TableInfo, type TableRef } from "./api";
import { JobProgress, JobsProvider, isRunning, jobStateLabel, useJobs } from "./jobs";
import Home, { forget, recentProjects, remember, type Recent } from "./screens/Projects";
import Overview from "./screens/Overview";
import Collections from "./screens/Collections";
import Search from "./screens/Search";
import MapView from "./screens/Map";
import Gaps from "./screens/Gaps";
import GapDetailView from "./screens/GapDetail";
import Settings from "./screens/Settings";
import "./App.css";

export type Tab = "overview" | "search" | "gaps" | "collections" | "map";
export type Route =
  | { view: "home"; mode?: "create" | "open" }
  | { view: "settings" }
  | { view: "project"; tab: Tab; gap?: number; region?: number; query?: string; collection?: string; adding?: boolean };

const TABS: [Tab, string][] = [["overview", "Overview"], ["search", "Search"], ["gaps", "Gaps"], ["collections", "Collections"], ["map", "Map"]];

/** What every project view gets from the shell. */
export interface Ctx {
  project: ProjectInfo;
  /** The vector space the views work on (a collection embedded with one model), if any. */
  table?: TableInfo;
  tref?: TableRef;
  go: (r: Route) => void;
  refresh: () => Promise<void>;
  setProject: (p: ProjectInfo) => void;
  pickTable: (t: TableInfo) => void;
}

export const tableKey = (t: TableInfo) => `${t.collection}\u0000${t.space_id}`;

export default function App() {
  const [project, setProject] = useState<ProjectInfo>();
  const refresh = async () => { try { setProject(await api.info()); } catch { /* no project open */ } };
  // A finished job changes readiness: reload the open project (its own jobs only change it).
  const onFinished = (j: JobInfo) => { if (j.project) refresh(); };
  return <JobsProvider onFinished={onFinished}>
    <Shell project={project} setProject={setProject} refresh={refresh} />
  </JobsProvider>;
}

function Shell({ project, setProject, refresh }: { project?: ProjectInfo; setProject: (p: ProjectInfo) => void; refresh: () => Promise<void> }) {
  const [route, setRoute] = useState<Route>({ view: "home" });
  const [selected, setSelected] = useState<string>();
  const [version, setVersion] = useState<string>();
  const [filter, setFilter] = useState("");
  const [recent, setRecent] = useState<Recent[]>(recentProjects);
  const [switchError, setSwitchError] = useState<string>();
  const [switching, setSwitching] = useState<string>();
  const jobs = useJobs();
  const job = jobs.current;

  useEffect(() => { if (inTauri) api.version().then(setVersion).catch(() => undefined); }, []);
  useEffect(() => { if (project) { remember(project); setRecent(recentProjects()); } }, [project]);
  useEffect(() => { if (project) setSelected(local.get<string | undefined>(`invectus.scope.${project.root}`, undefined)); }, [project?.root]);

  const tables = project?.tables ?? [];
  const table = tables.find((t) => tableKey(t) === selected) ?? tables.find((t) => t.origin === "embedded") ?? tables[0];
  const tref = table ? { collection: table.collection, space: table.space_id } : undefined;
  const pickTable = (t: TableInfo) => { setSelected(tableKey(t)); if (project) local.set(`invectus.scope.${project.root}`, tableKey(t)); };

  const opened = (p: ProjectInfo, created = false) => {
    setProject(p); setSwitchError(undefined);
    setRoute({ view: "project", tab: created || p.collections.length === 0 ? "overview" : "search" });
  };
  const switchTo = async (r: Recent) => {
    if (project?.root === r.root) { setRoute({ view: "project", tab: "overview" }); return; }
    setSwitchError(undefined); setSwitching(r.root);
    try { opened(await api.open(r.root, false)); } catch (e) { setSwitchError(String(e)); } finally { setSwitching(undefined); }
  };
  const toggleOffline = async () => {
    if (!project) return;
    try { setProject(await api.setNetwork(!project.network_enabled)); } catch (e) { setSwitchError(String(e)); }
  };

  const ctx: Ctx | undefined = project && { project, table, tref, go: setRoute, refresh, setProject, pickTable };
  // Alphabetical, so opening a project never moves the list under the pointer.
  const shown = recent.filter((r) => r.name.toLowerCase().includes(filter.trim().toLowerCase()))
    .sort((a, b) => a.name.localeCompare(b.name) || a.root.localeCompare(b.root));
  // A job in another project blocks switching (the backend refuses); say so before a click fails.
  const blocking = isRunning(job) && job!.project ? job : undefined;
  const jobProject = (j: JobInfo) => recent.find((r) => r.root === j.project)?.name ?? j.project?.split("/").pop();

  return <div className="shell">
    <nav aria-label="Projects" className="sidebar">
      <div className="brand"><span className="brand-mark" aria-hidden="true" /><span>inVectus</span></div>
      <label className="sr-only" htmlFor="project-filter">Filter projects</label>
      <input id="project-filter" type="search" className="filter" placeholder="Filter projects" value={filter} onChange={(e) => setFilter(e.target.value)} />
      <div className="label">Projects</div>
      <ul className="project-list">
        {shown.map((r) => <li key={r.root}>
          <button type="button" className="project-link" aria-current={project?.root === r.root && route.view === "project" ? "page" : undefined}
            disabled={!!switching || (!!blocking && blocking.project !== r.root)} onClick={() => switchTo(r)} title={r.root}>
            <span className="dot" aria-hidden="true" data-on={project?.root === r.root} />
            <span className="clip">{r.name}</span>
            {switching === r.root && <span className="muted small"> opening…</span>}
          </button>
        </li>)}
        {recent.length === 0 && <li className="muted small pad-s">No projects yet.</li>}
        {recent.length > 0 && shown.length === 0 && <li className="muted small pad-s">No project matches “{filter}”.</li>}
      </ul>
      {blocking && <p className="note small">Switching is paused while “{blocking.label}” runs in {jobProject(blocking)}.</p>}
      {switchError && <p className="error small" role="alert">{switchError}</p>}
      <div className="stack gap-s">
        <button type="button" className="btn block" onClick={() => setRoute({ view: "home", mode: "create" })}>+ New project</button>
        <button type="button" className="btn block" onClick={() => setRoute({ view: "home", mode: "open" })}>Open existing…</button>
      </div>

      <section aria-label="Background activity" className="activity-box">
        <div className="label">Activity</div>
        {!job && <div className="muted small">Idle</div>}
        {job && <>
          <div className="small"><b>{job.label}</b></div>
          <div className="muted small mono">{job.project ? jobProject(job) : "all projects"} · {jobStateLabel(job)}</div>
          {isRunning(job) && <JobProgress job={job} />}
          {job.state === "failed" && <div className="error small">{job.error}</div>}
          {isRunning(job) && job.kind !== "download" && <button type="button" className="btn small" onClick={() => api.cancel()}>Cancel</button>}
        </>}
      </section>

      <div className="sidebar-foot">
        <button type="button" className="nav-link" aria-current={route.view === "settings" ? "page" : undefined} onClick={() => setRoute({ view: "settings" })}>Settings</button>
        <div className="muted small mono">engine {version ?? "—"}</div>
      </div>
    </nav>

    <main className="main">
      {!inTauri && <p className="note banner">Browser preview: the engine runs only in the desktop app.</p>}
      {route.view === "home" && <Home mode={route.mode} current={project} onOpened={opened}
        recent={recent} onForget={(root) => { forget(root); setRecent(recentProjects()); }} onPick={switchTo} />}
      {route.view === "settings" && <Settings ctx={ctx} />}
      {route.view === "project" && ctx && <>
        <header className="workspace-head">
          <div className="title-row">
            <h1>{project!.name}</h1>
            <span className="muted small mono clip path" title={project!.root}>{project!.root}</span>
            <div className="push row wrap">
              {tables.length > 1 && <label className="inline small"><span className="label">Scope</span>
                <select value={table ? tableKey(table) : ""} onChange={(e) => { const t = tables.find((x) => tableKey(x) === e.target.value); if (t) pickTable(t); }}>
                  {tables.map((t) => <option key={tableKey(t)} value={tableKey(t)}>{t.collection} · {t.model_key}</option>)}
                </select></label>}
              <button type="button" className="pill" aria-pressed={!project!.network_enabled || project!.offline_env} onClick={toggleOffline}
                title={project!.offline_env ? "INVECTUS_OFFLINE is set: the network stays off" : "Network use: model downloads and outside-source checks only"}>
                {project!.network_enabled && !project!.offline_env ? "Network on" : "Offline"}
              </button>
            </div>
          </div>
          {project!.warning && <p className="note">{project!.warning}</p>}
          <nav aria-label="Project views" className="tabs">
            {TABS.map(([t, label]) => <button key={t} type="button" aria-current={route.tab === t ? "page" : undefined}
              onClick={() => setRoute({ view: "project", tab: t })}>{label}</button>)}
          </nav>
        </header>
        <div className="content">
          {route.tab === "overview" && <Overview ctx={ctx} />}
          {route.tab === "collections" && <Collections ctx={ctx} focus={route.collection} adding={route.adding} />}
          {route.tab === "search" && <Search ctx={ctx} initial={route.query} />}
          {route.tab === "map" && <MapView ctx={ctx} focus={route.region} />}
          {route.tab === "gaps" && route.gap === undefined && <Gaps ctx={ctx} />}
          {route.tab === "gaps" && route.gap !== undefined && <GapDetailView ctx={ctx} id={route.gap} />}
        </div>
      </>}
    </main>
  </div>;
}

/** Shown when a view needs something the scope lacks yet; says what and offers the fix. */
export function Needs({ title, body, action, onAction, busy }: { title: string; body: string; action?: string; onAction?: () => void; busy?: boolean }) {
  return <section className="panel empty">
    <span className="ring" aria-hidden="true" />
    <h2>{title}</h2>
    <p className="muted">{body}</p>
    {action && <button type="button" className="btn primary" disabled={busy} onClick={onAction}>{action}</button>}
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

/** Start a backend job and show it at once. */
export function useStartJob() {
  const jobs = useJobs();
  const action = useAction();
  const start = (f: () => Promise<JobInfo>) => action.run(async () => { jobs.started(await f()); });
  return { ...action, start, current: jobs.current };
}
