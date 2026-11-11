import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { api, inTauri, local, pickProject, pickSource, type GapView, type JobInfo, type MapData, type MapPoint, type ProjectInfo, type TableInfo, type TableRef } from "./api";
import { listen } from "@tauri-apps/api/event";
import { JobsProvider, isRunning, useJobs } from "./jobs";
import { AgentsProvider, AgentStatus, ProjectRowAgent, ProjectRowDetail } from "./agents";
import { changesProject, type AgentActivity } from "./agent-activity";
import DataCircle, { type Marks } from "./DataCircle";
import ProjectsPage, { forget, recentProjects, remember, renameRecent, type Recent } from "./screens/Projects";
import ProjectName from "./ProjectName";
import brandLogo from "./assets/branding/invectus-logo.png";
import brandLogoDarkText from "./assets/branding/invectus-logo-dark-text.png";
import Panel from "./screens/Panel";
import IndexingIndicator from "./IndexingIndicator";
import WorkspaceSplit from "./WorkspaceSplit";
import Integrations from "./screens/Integrations";
import Agents from "./screens/Agents";
import Settings from "./screens/Settings";
import Home from "./screens/Home";
import NewProject from "./screens/NewProject";
import AgentChip from "./AgentChip";
import TabIndicator, { useTabIndicator } from "./TabIndicator";
import StarPrompt from "./StarPrompt";
import { ArrowLeft, Blocks, Bot, Folder, FolderOpen, House, Plus, Search, Settings as SettingsIcon, type LucideIcon } from "lucide-react";
import { PROJECT_TABS, pageTab, navigateTo, openingTab, returnFromDetail, tabKeyIndex, type ProjectTab, type TabStacks } from "./navigation";
import { baseName } from "./paths";
import "./App.css";

/** A page beside the circle; Saved and Gaps retain their own detail return paths. */
export type Page =
  | { kind: "search"; query?: string } | { kind: "import"; path?: string; next?: string[] } | { kind: "add" }
  | { kind: "index"; collection?: string } | { kind: "gaps" } | { kind: "saved" }
  | { kind: "gap"; id: number } | { kind: "item"; point: MapPoint };

export type Route = { view: "home" } | { view: "projects" } | { view: "new" } | { view: "integrations" } | { view: "agents" } | { view: "settings" } | { view: "project" };

/** Older views navigate with these; `go` turns them into menu pages. */
type Legacy = { view: "project"; tab: "overview" | "search" | "gaps" | "collections" | "map"; gap?: number; region?: number; query?: string; collection?: string; adding?: boolean };

export interface Ctx {
  project: ProjectInfo;
  table?: TableInfo;
  tref?: TableRef;
  map?: MapData;
  gaps: GapView[];
  dataRevision: number;
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

/** The table a project opens on: the remembered one, else the first embedded table with a map. */
export function defaultTable(project: ProjectInfo, selected?: string): TableInfo | undefined {
  const tables = project.tables;
  return tables.find((t) => tableKey(t) === selected) ?? tables.find((t) => t.origin === "embedded" && t.regions) ?? tables.find((t) => t.origin === "embedded") ?? tables[0];
}

export default function App() {
  const [project, setProject] = useState<ProjectInfo>();
  const refresh = async () => { try {
    const next = await api.info();
    setProject((current) => current?.root === next.root ? next : current);
  } catch { /* no project open */ } };
  const onFinished = (j: JobInfo) => { if (j.project) refresh(); };
  return <JobsProvider onFinished={onFinished}>
    <AgentsProvider>
      <Shell project={project} setProject={setProject} refresh={refresh} />
    </AgentsProvider>
  </JobsProvider>;
}

function Shell({ project, setProject, refresh }: { project?: ProjectInfo; setProject: (p: ProjectInfo) => void; refresh: () => Promise<void> }) {
  const [route, setRoute] = useState<Route>({ view: "home" });
  const [recent, setRecent] = useState<Recent[]>(recentProjects);
  const [commandHeld, setCommandHeld] = useState(false);
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

  // Home shows the last project; it opens quietly (not counted as opened until you enter it).
  const quietRoot = useRef<string>();
  const [homeLoading, setHomeLoading] = useState(false);
  useEffect(() => {
    if (route.view !== "home" || project || !inTauri || opening) return;
    const last = recentProjects()[0];
    if (!last) return;
    let cancelled = false;
    setHomeLoading(true);
    api.open(last.root, false)
      .then((p) => { if (!cancelled) { quietRoot.current = p.root; setProject(p); } })
      .catch((e) => { if (!cancelled) setError(String(e)); })
      .finally(() => { if (!cancelled) setHomeLoading(false); });
    return () => { cancelled = true; };
  }, [route.view]);
  const remembered = useRef<ProjectInfo>();
  useEffect(() => {
    if (!project) return;
    if (quietRoot.current === project.root) {
      if (route.view !== "project") return;
      quietRoot.current = undefined;
    } else if (remembered.current === project) return;
    remembered.current = project;
    remember(project); setRecent(recentProjects());
  }, [project, route.view]);
  const enter = (first?: Page) => {
    setQuery(""); setError(undefined);
    setStack(first ? [{ kind: "search" }, first] : [{ kind: "search" }]);
    setRoute({ view: "project" });
  };

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
  useEffect(() => {
    const down = (event: KeyboardEvent) => {
      setCommandHeld(event.metaKey || event.key === "Meta" || event.code === "MetaLeft" || event.code === "MetaRight");
      if (event.isComposing || event.altKey || event.shiftKey) return;
      if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === "k") {
        event.preventDefault();
        const input = document.querySelector<HTMLInputElement>("#side-q");
        input?.focus(); input?.select();
      } else if (event.metaKey && /^[1-9]$/.test(event.key)) {
        const target = matches[Number(event.key) - 1];
        if (target) {
          event.preventDefault();
          if (!event.repeat) void openRecent(target);
        }
      }
    };
    const up = (event: KeyboardEvent) => setCommandHeld(event.metaKey);
    const reset = () => setCommandHeld(false);
    // Recover modifier state when Command was pressed before the window gained focus.
    const pointerModifiers = (event: MouseEvent) => setCommandHeld(event.metaKey);
    window.addEventListener("keydown", down, true);
    window.addEventListener("keyup", up, true);
    window.addEventListener("mousemove", pointerModifiers);
    window.addEventListener("mousedown", pointerModifiers);
    window.addEventListener("blur", reset);
    document.addEventListener("visibilitychange", reset);
    return () => {
      window.removeEventListener("keydown", down, true);
      window.removeEventListener("keyup", up, true);
      window.removeEventListener("mousemove", pointerModifiers);
      window.removeEventListener("mousedown", pointerModifiers);
      window.removeEventListener("blur", reset);
      document.removeEventListener("visibilitychange", reset);
    };
  }, [matches, opening, switchingBlocked, job?.project]);
  const nav = (label: string, Icon: LucideIcon, current: boolean, onClick: () => void, disabled = false) =>
    <button type="button" className="side-link" aria-current={current ? "page" : undefined} onClick={onClick} disabled={disabled}>
      <Icon size={16} strokeWidth={1.5} aria-hidden="true" /><span>{label}</span></button>;

  return <div className="shell">
    <nav aria-label="Main" className="sidebar">
      <div className="sidebar-window-drag" data-tauri-drag-region aria-hidden="true" />
      <div className="side-top">
        <div className="logo"><img className="logo-dark" src={brandLogo} alt="inVectus" /><img className="logo-light" src={brandLogoDarkText} alt="inVectus" />
          {/* Dev mode (npm run tauri dev): screen and style changes appear without a rebuild. */}
          {import.meta.env.DEV && <span className="dev-badge" title="Live dev build: changes to screens and styles appear here without a rebuild">Live</span>}</div>
        <div className="side-search">
          <Search size={16} strokeWidth={1.5} aria-hidden="true" />
          <label htmlFor="side-q" className="sr-only">Find a project</label>
          <input id="side-q" placeholder="Find a project" value={query} onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && matches[0]) openRecent(matches[0]); if (e.key === "Escape") setQuery(""); }} />
        </div>
        <div className="side-actions">
          {nav("Home", House, route.view === "home", () => setRoute({ view: "home" }))}
          {nav("New project", Plus, route.view === "new", () => setRoute({ view: "new" }), opening || switchingBlocked)}
          {nav("Open project…", FolderOpen, false, openProject, opening || switchingBlocked)}
          {nav("Integrations", Blocks, route.view === "integrations", () => setRoute({ view: "integrations" }))}
          {nav("Agents", Bot, route.view === "agents", () => setRoute({ view: "agents" }))}
        </div>
        <div className="sidebar-projects">
          {nav("Projects", Folder, route.view === "projects", () => setRoute({ view: "projects" }))}
          <ul className="project-list" aria-label="Projects">
            {matches.map((r, index) => <li key={r.root}>
              <div className="project-sidebar-row" aria-keyshortcuts={index < 9 ? `Meta+${index + 1}` : undefined} data-current={route.view === "project" && project?.root === r.root} title={tilde(r.root)}
                onClick={(e) => { if (e.detail <= 1 && !(e.target as HTMLElement).closest("input, button, a")) void openRecent(r); }}>
                <div className="project-row-text">
                  <ProjectName name={r.name} className="sidebar-project-name" disabled={opening || (switchingBlocked && job?.project !== r.root)} onSelect={() => void openRecent(r)} onSave={(name) => renameProject(r.root, name)} />
                  <ProjectRowDetail root={r.root} fallback={rowFact(r, job)} working={job && job.project === r.root && isRunning(job) ? jobLine(job) : undefined} />
                </div>
                {commandHeld && index < 9 ? <kbd className="project-shortcut" aria-hidden="true">{index + 1}</kbd> : <ProjectRowAgent root={r.root} />}
              </div>
            </li>)}
          </ul>
          {matches.length === 0 && <p className="side-empty">{query.trim() ? "No matching projects" : "No projects yet"}</p>}
        </div>
        {error && <p className="side-error" role="alert">{error}</p>}
      </div>
      <div className="side-bottom">
        {/* Only work in progress shows here; finished jobs leave the sidebar. */}
        {job && isRunning(job) && <JobLine job={job} recent={recent} />}
        {nav("Settings", SettingsIcon, route.view === "settings", () => setRoute({ view: "settings" }))}
      </div>
    </nav>

    <main className="main">
      {!inTauri && <p className="banner">Browser preview: the engine runs only in the desktop app.</p>}
      {(route.view === "projects" || route.view === "home") && error && <p className="banner err" role="alert">{error}</p>}
      {route.view === "home" && <Home project={project} recent={recent} job={job} loading={homeLoading || opening}
        onEnter={enter} onPick={openRecent} onNew={() => setRoute({ view: "new" })} onOpen={openProject} onProjects={() => setRoute({ view: "projects" })} />}
      {route.view === "projects" && <ProjectsPage current={project} recent={recent} job={job}
        onPick={openRecent} onNew={() => setRoute({ view: "new" })} onOpen={openProject} onForget={(root) => { forget(root); setRecent(recentProjects()); }} />}
      {route.view === "new" && <NewProject onCreated={opened} onIntegrations={() => setRoute({ view: "integrations" })} blocked={isRunning(job) && !!job!.project ? job : undefined} />}
      {route.view === "integrations" && <Integrations />}
      {route.view === "agents" && <Agents />}
      {route.view === "settings" && <Settings project={project} setProject={setProject} />}
      {route.view === "project" && project && <Workspace key={project.root} project={project} setProject={setProject} refresh={refresh}
        initialStack={stack} goRoute={setRoute} onRename={(name) => renameProject(project.root, name)} />}
    </main>
    <StarPrompt />
  </div>;
}

/** A running job in a few words for the sidebar: "Pulling Google Sheet…", "Indexing papers…".
 *  Long identifiers (sheet ids, paths, links) are left out. */
function jobLine(job: JobInfo): string {
  const words: string[] = [];
  for (const w of job.label.split(/\s+/)) {
    if (w.length > 20 || /[/\\:]/.test(w)) break;
    words.push(w);
  }
  return `${(words.join(" ") || job.label).replace(/[.…]+$/, "")}…`;
}

/** The quiet second line of a sidebar project: a running job, else its size. */
function rowFact(r: Recent, job?: JobInfo): string {
  if (job && job.project === r.root && isRunning(job)) return job.phase ?? job.label;
  return r.items ? `${r.items.toLocaleString()} items` : "Empty";
}

function JobLine({ job, recent }: { job: JobInfo; recent: Recent[] }) {
  const cancel = useAction();
  const name = job.project ? recent.find((r) => r.root === job.project)?.name ?? baseName(job.project) : "all projects";
  const pct = job.total ? Math.round((job.done / job.total) * 100) : 0;
  return <div className="job-line" aria-live="polite">
    <span>{job.label}</span>
    <span className="dim">{name} · {job.state === "running" ? `${job.phase ?? "starting"} ${job.total ? `${pct}%` : ""}` : job.state === "completed" ? "done" : job.state}</span>
    {job.state === "failed" && <span className="err">{job.error}</span>}
    <ErrorLine error={cancel.error} />
    {isRunning(job) && job.kind !== "download" && <button type="button" className="text-btn" disabled={cancel.busy} onClick={() => cancel.run(() => api.cancel(job))}>Cancel</button>}
  </div>;
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
  const [dataRevision, setDataRevision] = useState(0);
  const [mapRevision, setMapRevision] = useState(0);
  const initialPage = initialStack[initialStack.length - 1] ?? { kind: "search" as const };
  const initialTab = openingTab(initialStack, local.get(`invectus.tab.${project.root}`, ""));
  const [activeTab, setActiveTab] = useState<ProjectTab>(initialTab);
  const { tabsRef, tabIndicator } = useTabIndicator(activeTab);
  const [tabStacks, setTabStacks] = useState<TabStacks>(() => {
    const page = pageTab(initialPage) === initialTab ? initialPage : PROJECT_TABS.find((tab) => tab.id === initialTab)!.page;
    return { [initialTab]: initialTab === "collections" && page.kind === "import" ? [{ kind: "index" }, page] : [page] };
  });
  const [tabMarks, setTabMarks] = useState<Partial<Record<ProjectTab, Marks>>>({});
  const marks = tabMarks[activeTab] ?? {};
  const stack = tabStacks[activeTab]!;
  const setMarks = (m: Marks) => setTabMarks((prev) => ({ ...prev, [activeTab]: m }));
  const selectTab = (id: ProjectTab) => {
    setTabStacks((prev) => prev[id] ? prev : { ...prev, [id]: [PROJECT_TABS.find((t) => t.id === id)!.page] });
    setActiveTab(id);
  };
  useEffect(() => { local.set(`invectus.tab.${project.root}`, activeTab); }, [project.root, activeTab]);
  const setStack = (pages: Page[] | ((s: Page[]) => Page[]), tab = activeTab) =>
    setTabStacks((prev) => ({ ...prev, [tab]: typeof pages === "function" ? pages(prev[tab]!) : pages }));
  const [opts, setOpts] = useState({ showGaps: true, showLabels: false, showOutlines: false });
  const [resetKey, setResetKey] = useState(0);
  const [mapFocus, setMapFocus] = useState<{ region: number; request: number }>();
  const [mapError, setMapError] = useState<string>();

  const tables = project.tables;
  const table = defaultTable(project, selected);
  const tref = useMemo(() => table ? { project: project.root, collection: table.collection, space: table.space_id } : undefined, [project.root, table?.collection, table?.space_id]);
  const job = jobs.current && jobs.current.project === project.root ? jobs.current : undefined;

  useEffect(() => {
    setMapError(undefined);
    let cancelled = false;
    if (tref && table?.regions) api.map(tref).then((data) => { if (!cancelled) setMap(data); })
      .catch((e) => { if (!cancelled) setMapError(String(e)); });
    return () => { cancelled = true; };
  }, [project.root, tref?.collection, tref?.space, table?.regions?.run_id, mapRevision]);
  useEffect(() => { setMap(undefined); setMapFocus(undefined); }, [project.root, tref?.collection, tref?.space, table?.regions?.run_id]);
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
  // Reload only completed writes for this project. Detail views keep drafts while refreshing.
  const reloadRef = useRef({ refresh, reloadGaps, root: project.root });
  reloadRef.current = { refresh, reloadGaps, root: project.root };
  useEffect(() => {
    if (!inTauri) return;
    const un = listen<AgentActivity>("agent-activity", (e) => {
      if (!changesProject(e.payload, reloadRef.current.root)) return;
      setDataRevision((v) => v + 1);
      if (e.payload.tool === "submit_region_names") setMapRevision((v) => v + 1);
      void reloadRef.current.refresh();
      void reloadRef.current.reloadGaps();
    });
    return () => { un.then((f) => f()); };
  }, []);
  useEffect(() => {
    if (!job || job.state === "running") return;
    setDataRevision((v) => v + 1);
    setMapRevision((v) => v + 1);
    void reloadGaps();
  }, [job?.id, job?.state]);

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
    else if (r.tab === "collections") push(r.adding ? { kind: "add" } : { kind: "index", collection: r.collection });
    else if (r.tab === "map") { if (r.region !== undefined) showRegion(r.region); }
  };
  const pickTable = (t: TableInfo) => { setSelected(tableKey(t)); local.set(`invectus.scope.${project.root}`, tableKey(t)); };

  const ctx: Ctx = {
    project, table, tref, map, gaps, dataRevision, gapsLoading, gapsError, go, push, back, backFromDetail, setMarks, showRegion, reloadGaps, refresh, setProject, pickTable,
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
      {/* The network switch lives in Settings → Network. */}
      <div className="head-actions"><AgentChip project={project} setProject={setProject} /></div>
    </header>
    <WorkspaceSplit>
      <section className="project-console" aria-label="Project workspace">
        {project.warning && <p className="banner">{project.warning}</p>}
        <AgentStatus root={project.root} />
        <div className="project-toolbar">
          <div ref={tabsRef} className="project-tabs" role="tablist" aria-label="Project sections">
            {PROJECT_TABS.map((tab, i) => <button key={tab.id} id={`tab-${tab.id}`} type="button" role="tab"
              aria-selected={activeTab === tab.id} aria-controls={tabStacks[tab.id] ? `panel-${tab.id}` : undefined} tabIndex={activeTab === tab.id ? 0 : -1}
              onClick={() => selectTab(tab.id)} onKeyDown={(e) => {
                const next = tabKeyIndex(e.key, i);
                if (next !== undefined) { e.preventDefault(); selectTab(PROJECT_TABS[next].id); document.getElementById(`tab-${PROJECT_TABS[next].id}`)?.focus(); }
              }}><span className="tab-code">{String(i + 1).padStart(2, "0")}</span>{tab.label}</button>)}
            <TabIndicator geometry={tabIndicator} />
          </div>
        <IndexingIndicator job={job} />
        </div>
          {PROJECT_TABS.filter((tab) => tabStacks[tab.id]).map((tab) => <Panel key={tab.id} active={activeTab === tab.id} tabId={tab.id}
            ctx={{ ...ctx, push: (p) => pushFrom(tab.id, p), back: () => setStack((s) => s.length > 1 ? s.slice(0, -1) : s, tab.id),
              backFromDetail: () => backFromDetail(tab.id),
              setMarks: (m) => setTabMarks((prev) => ({ ...prev, [tab.id]: m })) }} stack={tabStacks[tab.id]!} />)}
      </section>
      <DataCircle data={map} gaps={gaps} marks={marks} job={job} resetKey={resetKey} focusRegion={mapFocus}
        gapsActive={activeTab === "gaps" || (activeTab === "saved" && currentPage.kind === "gap")}
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
