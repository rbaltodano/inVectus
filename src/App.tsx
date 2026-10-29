import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { api, inTauri, local, pickProject, pickSource, type GapView, type JobInfo, type MapData, type MapPoint, type ProjectInfo, type TableInfo, type TableRef } from "./api";
import { JobsProvider, isRunning, useJobs } from "./jobs";
import DataCircle, { type Marks } from "./DataCircle";
import ProjectsPage, { forget, recentProjects, remember, renameRecent, type Recent } from "./screens/Projects";
import ProjectName from "./ProjectName";
import Panel from "./screens/Panel";
import IndexingIndicator from "./IndexingIndicator";
import WorkspaceSplit from "./WorkspaceSplit";
import Settings from "./screens/Settings";
import { ArrowLeft, Folder, FolderOpen, Plus, Search, Settings as SettingsIcon, type LucideIcon } from "lucide-react";
import { PROJECT_TABS, pageTab, navigateTo, returnFromDetail, tabKeyIndex, type ProjectTab, type TabStacks } from "./navigation";
import "./App.css";

/** A page of the menu beside the circle; Back skips details: to the gap list from a gap, otherwise to Search. */
export type Page =
  | { kind: "search"; query?: string } | { kind: "import"; path?: string }
  | { kind: "index"; collection?: string } | { kind: "gaps" }
  | { kind: "gap"; id: number } | { kind: "item"; point: MapPoint };

export type Route = { view: "projects" } | { view: "new" } | { view: "settings" } | { view: "project" };

/** Older views navigate with these; `go` turns them into menu pages. */
type Legacy = { view: "project"; tab: "overview" | "search" | "gaps" | "collections" | "map"; gap?: number; region?: number; query?: string; collection?: string; adding?: boolean };

export interface Ctx {
  project: ProjectInfo;
  table?: TableInfo;
  tref?: TableRef;
  map?: MapData;
  gaps: GapView[];
  gapsLoading: boolean;
  gapsError?: string;
  go: (r: Route | Legacy) => void;
  push: (p: Page) => void;
  back: () => void;
  backFromDetail: () => void;
  setMarks: (m: Marks) => void;
  showRegion: (region: number) => void;
  reloadGaps: () => Promise<void>;
  refresh: () => Promise<void>;
  setProject: (p: ProjectInfo) => void;
  pickTable: (t: TableInfo) => void;
  view: { showGaps: boolean; showLabels: boolean; showOutlines: boolean; set: (k: "showGaps" | "showLabels" | "showOutlines", v: boolean) => void; reset: () => void };
}

export const tableKey = (t: TableInfo) => `${t.collection}\u0000${t.space_id}`;
const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");

export default function App() {
  const [project, setProject] = useState<ProjectInfo>();
  const refresh = async () => { try { setProject(await api.info()); } catch { /* no project open */ } };
  const onFinished = (j: JobInfo) => { if (j.project) refresh(); };
  return <JobsProvider onFinished={onFinished}>
    <Shell project={project} setProject={setProject} refresh={refresh} />
  </JobsProvider>;
}

function Shell({ project, setProject, refresh }: { project?: ProjectInfo; setProject: (p: ProjectInfo) => void; refresh: () => Promise<void> }) {
  const [route, setRoute] = useState<Route>({ view: "projects" });
  const [recent, setRecent] = useState<Recent[]>(recentProjects);
  const [stack, setStack] = useState<Page[]>([{ kind: "search" }]);
  const [query, setQuery] = useState("");
  const [error, setError] = useState<string>();
  const jobs = useJobs();
  const job = jobs.current;
  const switchingBlocked = isRunning(job) && !!job?.project;
  const [opening, setOpening] = useState(false);
  const projectRef = useRef(project);
  projectRef.current = project;
  const renameProject = async (root: string, name: string) => {
    const saved = await api.rename(root, name);
    renameRecent(root, saved); setRecent(recentProjects());
    const current = projectRef.current;
    if (current?.root === root) setProject({ ...current, name: saved });
  };

  useEffect(() => { if (project) { remember(project); setRecent(recentProjects()); } }, [project]);

  const opened = (p: ProjectInfo, first?: Page) => {
    setProject(p); setError(undefined);
    setStack(first ? [{ kind: "search" }, first] : [{ kind: "search" }]);
    setRoute({ view: "project" });
  };
  const openRecent = async (r: Recent) => {
    if (opening || (switchingBlocked && job?.project !== r.root)) return;
    setQuery(""); setError(undefined); setStack([{ kind: "search" }]);
    if (project?.root === r.root) { setRoute({ view: "project" }); return; }
    setOpening(true);
    try { opened(await api.open(r.root, false)); } catch (e) { setError(String(e)); }
    finally { setOpening(false); }
  };
  const openProject = async () => {
    if (opening || switchingBlocked) return;
    setError(undefined); setOpening(true);
    try { const path = await pickProject(); if (path) opened(await api.open(path, false)); }
    catch (e) { setError(String(e)); }
    finally { setOpening(false); }
  };
  const matches = recent.filter((r) => r.name.toLowerCase().includes(query.trim().toLowerCase()));
  const nav = (label: string, Icon: LucideIcon, current: boolean, onClick: () => void, disabled = false) =>
    <button type="button" className="side-link" aria-current={current ? "page" : undefined} onClick={onClick} disabled={disabled}>
      <Icon size={16} strokeWidth={1.5} aria-hidden="true" /><span>{label}</span></button>;

  return <div className="shell">
    <nav aria-label="Main" className="sidebar">
      <div className="side-top">
        <div className="logo">inVectus</div>
        <div className="side-search">
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
          <label htmlFor="side-q" className="sr-only">Find a project</label>
          <input id="side-q" placeholder="Find a project" value={query} onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && matches[0]) openRecent(matches[0]); if (e.key === "Escape") setQuery(""); }} />
        </div>
        <div className="side-actions">
          {nav("New project", Plus, route.view === "new", () => setRoute({ view: "new" }), opening || switchingBlocked)}
          {nav("Open project…", FolderOpen, false, openProject, opening || switchingBlocked)}
        </div>
        <div className="sidebar-projects">
          {nav("Projects", Folder, route.view === "projects", () => setRoute({ view: "projects" }))}
          <ul className="project-list" aria-label="Projects">
            {matches.map((r) => <li key={r.root}>
              <div className="project-sidebar-row" data-current={route.view === "project" && project?.root === r.root}>
                <button type="button" className="project-open" aria-label={`Open project ${r.name}`} title={tilde(r.root)}
                  disabled={opening || (switchingBlocked && job?.project !== r.root)} onClick={() => openRecent(r)}><Folder size={14} aria-hidden="true" /></button>
                <ProjectName name={r.name} className="sidebar-project-name" disabled={opening} onSave={(name) => renameProject(r.root, name)} />
              </div>
            </li>)}
          </ul>
          {matches.length === 0 && <p className="side-empty">{query.trim() ? "No matching projects" : "No projects yet"}</p>}
        </div>
        {error && <p className="side-error" role="alert">{error}</p>}
      </div>
      <div className="side-bottom">
        {job && <JobLine job={job} recent={recent} />}
        {nav("Settings", SettingsIcon, route.view === "settings", () => setRoute({ view: "settings" }))}
      </div>
    </nav>

    <main className="main">
      {!inTauri && <p className="banner">Browser preview: the engine runs only in the desktop app.</p>}
      {route.view === "projects" && error && <p className="banner err" role="alert">{error}</p>}
      {route.view === "projects" && <ProjectsPage current={project} recent={recent} job={job}
        onPick={openRecent} onNew={() => setRoute({ view: "new" })} onOpen={openProject} onForget={(root) => { forget(root); setRecent(recentProjects()); }} />}
      {route.view === "new" && <NewProject onCreated={opened} blocked={isRunning(job) && !!job!.project ? job : undefined} />}
      {route.view === "settings" && <Settings project={project} setProject={setProject} />}
      {route.view === "project" && project && <Workspace key={project.root} project={project} setProject={setProject} refresh={refresh}
        initialStack={stack} goRoute={setRoute} onRename={(name) => renameProject(project.root, name)} />}
    </main>
  </div>;
}

function JobLine({ job, recent }: { job: JobInfo; recent: Recent[] }) {
  const name = job.project ? recent.find((r) => r.root === job.project)?.name ?? job.project.split("/").pop() : "all projects";
  const pct = job.total ? Math.round((job.done / job.total) * 100) : 0;
  return <div className="job-line" aria-live="polite">
    <span>{job.label}</span>
    <span className="dim">{name} · {job.state === "running" ? `${job.phase ?? "starting"} ${job.total ? `${pct}%` : ""}` : job.state === "completed" ? "done" : job.state}</span>
    {job.state === "failed" && <span className="err">{job.error}</span>}
    {isRunning(job) && job.kind !== "download" && <button type="button" className="text-btn" onClick={() => api.cancel()}>Cancel</button>}
  </div>;
}

/** New project: name and place on the right; the empty circle picks the dataset. */
function NewProject({ onCreated, blocked }: { onCreated: (p: ProjectInfo, first?: Page) => void; blocked?: JobInfo }) {
  const [name, setName] = useState("");
  const [dir, setDir] = useState("");
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  useEffect(() => { if (inTauri) api.defaultDir().then(setDir).catch(() => undefined); }, []);
  const create = async (source?: string) => {
    setError(undefined); setBusy(true);
    try {
      const stem = source?.split("/").pop()?.replace(/\.[^.]+$/, "") ?? "";
      const n = name.trim() || stem;
      if (!n) { setError("Name the project, or pick a dataset to name it after."); return; }
      const p = await api.open(`${dir.replace(/\/$/, "")}/${n}`, true);
      onCreated(p, source ? { kind: "import", path: source } : undefined);
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };
  const pick = async (folder = false) => {
    if (blocked) return;
    const path = await pickSource(folder).catch((e) => { setError(String(e)); return null; });
    if (path) create(path);
  };
  return <>
    <header className="head"><div><div className="eyebrow">Setup</div><h1>New project</h1></div></header>
    <div className="stage">
      <aside className="menu" aria-label="New project">
        <div className="menu-card stack gap">
          <label className="field">Name<input value={name} onChange={(e) => setName(e.target.value)} placeholder="Medical school research topic" /></label>
          <label className="field">Location<input value={dir} onChange={(e) => setDir(e.target.value)} /></label>
          <span className="dim small">{name.trim() ? `${tilde(dir.replace(/\/$/, ""))}/${name.trim()}.invectus` : "Everything stays on this computer. Avoid iCloud, Dropbox and OneDrive folders."}</span>
        </div>
        <button type="button" className="menu-row" onClick={() => pick(true)} disabled={busy || !!blocked}>Choose a folder instead</button>
        <button type="button" className="menu-row" onClick={() => create()} disabled={busy || !name.trim() || !!blocked}>Create without data</button>
        {blocked && <p className="note">“{blocked.label}” is running; a new project can open when it finishes.</p>}
        {error && <p className="err" role="alert">{error}</p>}
      </aside>
      <DataCircle gaps={[]} marks={{}} showGaps={false} showLabels={false} showOutlines={false} onGap={() => undefined} onItem={() => undefined}
        empty={{ title: "Click to add a dataset", hint: "CSV · TSV · Parquet · or a folder of .txt / .md", onPick: () => pick(false) }} />
    </div>
  </>;
}

function Workspace({ project, setProject, refresh, initialStack, goRoute, onRename }: {
  project: ProjectInfo; setProject: (p: ProjectInfo) => void; refresh: () => Promise<void>;
  initialStack: Page[]; goRoute: (r: Route) => void;
  onRename: (name: string) => Promise<void>;
}) {
  const jobs = useJobs();
  const [selected, setSelected] = useState(() => local.get<string | undefined>(`invectus.scope.${project.root}`, undefined));
  const [map, setMap] = useState<MapData>();
  const [gaps, setGaps] = useState<GapView[]>([]);
  const [gapsLoading, setGapsLoading] = useState(false);
  const [gapsError, setGapsError] = useState<string>();
  const gapsRequest = useRef(0);
  const initialTab = pageTab(initialStack[initialStack.length - 1]);
  const [activeTab, setActiveTab] = useState<ProjectTab>(initialTab);
  const tabsRef = useRef<HTMLDivElement>(null);
  const [tabIndicator, setTabIndicator] = useState<{ left: number; width: number; top: number }>();
  useLayoutEffect(() => {
    const tabs = tabsRef.current;
    if (!tabs) return;
    const measure = () => {
      const selected = tabs.querySelector<HTMLElement>('[aria-selected="true"]');
      if (!selected) return;
      const style = getComputedStyle(selected);
      const leftPadding = parseFloat(style.paddingLeft), rightPadding = parseFloat(style.paddingRight);
      const next = { left: selected.offsetLeft + leftPadding, width: selected.offsetWidth - leftPadding - rightPadding,
        top: selected.offsetTop + selected.offsetHeight - parseFloat(style.paddingBottom) + 6 };
      setTabIndicator((previous) => previous?.left === next.left && previous.width === next.width && previous.top === next.top ? previous : next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(tabs);
    tabs.querySelectorAll('button[role="tab"]').forEach((button) => observer.observe(button));
    return () => observer.disconnect();
  }, [activeTab]);
  const [tabStacks, setTabStacks] = useState<TabStacks>(() => ({
    [initialTab]: initialTab === "collections" && initialStack[initialStack.length - 1].kind === "import"
      ? [{ kind: "index" }, initialStack[initialStack.length - 1]] : [initialStack[initialStack.length - 1]],
  }));
  const [tabMarks, setTabMarks] = useState<Partial<Record<ProjectTab, Marks>>>({});
  const marks = tabMarks[activeTab] ?? {};
  const stack = tabStacks[activeTab]!;
  const setMarks = (m: Marks) => setTabMarks((prev) => ({ ...prev, [activeTab]: m }));
  const selectTab = (id: ProjectTab) => {
    setTabStacks((prev) => prev[id] ? prev : { ...prev, [id]: [PROJECT_TABS.find((t) => t.id === id)!.page] });
    setActiveTab(id);
  };
  const setStack = (pages: Page[] | ((s: Page[]) => Page[]), tab = activeTab) =>
    setTabStacks((prev) => ({ ...prev, [tab]: typeof pages === "function" ? pages(prev[tab]!) : pages }));
  const [opts, setOpts] = useState({ showGaps: true, showLabels: false, showOutlines: false });
  const [resetKey, setResetKey] = useState(0);
  const [mapFocus, setMapFocus] = useState<{ region: number; request: number }>();
  const [mapError, setMapError] = useState<string>();
  const [netError, setNetError] = useState<string>();

  const tables = project.tables;
  const table = tables.find((t) => tableKey(t) === selected) ?? tables.find((t) => t.origin === "embedded" && t.regions) ?? tables.find((t) => t.origin === "embedded") ?? tables[0];
  const tref = useMemo(() => table ? { collection: table.collection, space: table.space_id } : undefined, [table?.collection, table?.space_id]);
  const job = jobs.current && jobs.current.project === project.root ? jobs.current : undefined;

  useEffect(() => {
    setMap(undefined); setMapError(undefined); setMapFocus(undefined);
    if (tref && table?.regions) api.map(tref).then(setMap).catch((e) => setMapError(String(e)));
  }, [tref?.collection, tref?.space, table?.regions?.run_id]);
  const reloadGaps = useCallback(async () => {
    const request = ++gapsRequest.current;
    if (!tref || !table?.regions) { setGaps([]); setGapsError(undefined); setGapsLoading(false); return; }
    setGapsLoading(true); setGapsError(undefined);
    try {
      const saved = await api.gaps(tref);
      if (request === gapsRequest.current) setGaps(saved.gaps);
    } catch (e) {
      if (request === gapsRequest.current) setGapsError(String(e));
    } finally {
      if (request === gapsRequest.current) setGapsLoading(false);
    }
  }, [project.root, tref, table?.regions?.run_id]);
  const viewingGaps = activeTab === "gaps";
  useEffect(() => {
    setGaps([]);
    if (table?.regions?.scanned) void reloadGaps();
    else { setGapsError(undefined); setGapsLoading(false); }
    return () => { ++gapsRequest.current; };
  }, [reloadGaps, table?.regions?.scanned]);
  useEffect(() => {
    if (viewingGaps && table?.regions?.scanned) void reloadGaps();
  }, [viewingGaps]);

  const pushFrom = (tab: ProjectTab, p: Page) => {
    const target = p.kind === "item" || p.kind === "gap" ? tab : pageTab(p);
    setTabStacks((prev) => navigateTo(prev, tab, p).stacks);
    setActiveTab(target);
  };
  const push = (p: Page) => pushFrom(activeTab, p);
  const back = () => setStack((s) => (s.length > 1 ? s.slice(0, -1) : s));
  const backFromDetail = (from = activeTab) => {
    const next = returnFromDetail(tabStacks, from);
    setTabStacks(next.stacks);
    setActiveTab(next.tab);
  };
  const showRegion = (region: number) => {
    setMarks({ region });
    setMapFocus((previous) => ({ region, request: (previous?.request ?? 0) + 1 }));
  };
  const go = (r: Route | Legacy) => {
    if (!("tab" in r)) { if (r.view !== "project") goRoute(r); return; }
    if (r.tab === "overview") selectTab("search");
    else if (r.tab === "search") push({ kind: "search", query: r.query });
    else if (r.tab === "gaps") push(r.gap !== undefined ? { kind: "gap", id: r.gap } : { kind: "gaps" });
    else if (r.tab === "collections") push(r.adding ? { kind: "import" } : { kind: "index", collection: r.collection });
    else if (r.tab === "map") { if (r.region !== undefined) showRegion(r.region); }
  };
  const pickTable = (t: TableInfo) => { setSelected(tableKey(t)); local.set(`invectus.scope.${project.root}`, tableKey(t)); };

  const ctx: Ctx = {
    project, table, tref, map, gaps, gapsLoading, gapsError, go, push, back, backFromDetail, setMarks, showRegion, reloadGaps, refresh, setProject, pickTable,
    view: { ...opts, set: (k, v) => setOpts((o) => ({ ...o, [k]: v })), reset: () => setResetKey((k) => k + 1) },
  };
  const addData = async () => {
    const path = await pickSource(false).catch(() => null);
    if (path) push({ kind: "import", path });
  };
  const currentPage = stack[stack.length - 1];
  const importing = currentPage.kind === "import";
  const noData = project.collections.length === 0 && !isRunning(job) && !importing;
  const anyIndex = tables.some((t) => t.origin === "embedded");
  // A finished job reloads the project, which reloads the map; the circle glides to it.
  const prevJob = useRef<number>();
  useEffect(() => { if (job && job.state !== "running" && prevJob.current === job.id) refresh(); prevJob.current = job?.id; }, [job?.state]);

  return <>
    <header className="head">
      <div className="project-heading"><button type="button" className="eyebrow project-back" onClick={() => goRoute({ view: "projects" })}><ArrowLeft size={14} aria-hidden="true" />Back to Projects</button><h1><ProjectName name={project.name} onSave={onRename} /></h1><div className="path">{tilde(project.root)}</div></div>
      <button type="button" className={`net ${project.network_enabled && !project.offline_env ? "on" : ""}`} aria-pressed={!project.network_enabled}
        onClick={async () => { try { setProject(await api.setNetwork(!project.network_enabled)); } catch (e) { setNetError(String(e)); } }}
        title={project.offline_env ? "INVECTUS_OFFLINE is set: the network stays off" : "Network is used only for model downloads and outside-source checks"}>
        {project.network_enabled && !project.offline_env ? "Network on" : "Offline"}</button>
    </header>
    <WorkspaceSplit>
      <section className="project-console" aria-label="Project workspace">
        {project.warning && <p className="banner">{project.warning}</p>}
        {netError && <p className="banner err">{netError}</p>}
        <div className="project-toolbar">
          <div ref={tabsRef} className="project-tabs" role="tablist" aria-label="Project sections">
            {PROJECT_TABS.map((tab, i) => <button key={tab.id} id={`tab-${tab.id}`} type="button" role="tab"
              aria-selected={activeTab === tab.id} aria-controls={tabStacks[tab.id] ? `panel-${tab.id}` : undefined} tabIndex={activeTab === tab.id ? 0 : -1}
              onClick={() => selectTab(tab.id)} onKeyDown={(e) => {
                const next = tabKeyIndex(e.key, i);
                if (next !== undefined) { e.preventDefault(); selectTab(PROJECT_TABS[next].id); document.getElementById(`tab-${PROJECT_TABS[next].id}`)?.focus(); }
              }}><span className="tab-code">{String(i + 1).padStart(2, "0")}</span>{tab.label}</button>)}
            {tabIndicator && <span className="project-tab-indicator" aria-hidden="true" style={{ width: tabIndicator.width, top: tabIndicator.top, transform: `translateX(${tabIndicator.left}px)` }} />}
          </div>
        <IndexingIndicator job={job} preview />
        </div>
          {PROJECT_TABS.filter((tab) => tabStacks[tab.id]).map((tab) => <Panel key={tab.id} active={activeTab === tab.id} tabId={tab.id}
            ctx={{ ...ctx, push: (p) => pushFrom(tab.id, p), back: () => setStack((s) => s.length > 1 ? s.slice(0, -1) : s, tab.id),
              backFromDetail: () => backFromDetail(tab.id),
              setMarks: (m) => setTabMarks((prev) => ({ ...prev, [tab.id]: m })) }} stack={tabStacks[tab.id]!} />)}
      </section>
      <DataCircle data={map} gaps={gaps} marks={marks} job={job} resetKey={resetKey} focusRegion={mapFocus}
        gapsActive={activeTab === "gaps"}
        selectedGapId={currentPage.kind === "gap" ? currentPage.id : undefined}
        showGaps={opts.showGaps} showLabels={opts.showLabels} showOutlines={opts.showOutlines}
        empty={noData ? { title: "Click to add a dataset", hint: "CSV · TSV · Parquet · or a folder of .txt / .md", onPick: addData } : undefined}
        idle={mapError ? "Map unavailable" : importing && project.collections.length === 0 ? "Choose columns in the import panel" : !anyIndex ? "Not indexed yet" : table && !table.regions ? "No map yet · see Collections" : map ? undefined : "Loading map…"}
        onGap={(id) => push({ kind: "gap", id })} onItem={(p) => push({ kind: "item", point: p })} />
    </WorkspaceSplit>
  </>;
}

/** Shown when a page needs something the scope lacks yet; says what and offers the fix. */
export function Needs({ title, body, action, onAction, busy }: { title: string; body: string; action?: string; onAction?: () => void; busy?: boolean }) {
  return <div className="menu-card stack gap-s">
    <b>{title}</b>
    <span className="dim small">{body}</span>
    {action && <button type="button" className="btn primary start" disabled={busy} onClick={onAction}>{action}</button>}
  </div>;
}

export function ErrorLine({ error }: { error?: string }) {
  return error ? <p className="err" role="alert">{error}</p> : null;
}

export function useAction() {
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const run = async <T,>(f: () => Promise<T>): Promise<T | undefined> => {
    setBusy(true); setError(undefined);
    try { return await f(); } catch (e) { setError(String(e)); return undefined; } finally { setBusy(false); }
  };
  return { busy, error, setError, run };
}

export function useStartJob() {
  const jobs = useJobs();
  const action = useAction();
  const start = (f: () => Promise<JobInfo>) => action.run(async () => { jobs.started(await f()); });
  return { ...action, start, current: jobs.current };
}
