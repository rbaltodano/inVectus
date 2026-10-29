import { fmt, local, type JobInfo, type ProjectInfo } from "../api";
import plusIcon from "../assets/icons/plus.svg";
import LineArt from "../LineArt";

export interface Recent { root: string; name: string; collections: number; items: number; models: string[]; warning: string | null; opened: number }
const KEY = "invectus.recent";

/** Recent projects (browser storage: a convenience; the projects themselves are folders). */
export const recentProjects = () => local.get<Recent[]>(KEY, []);

export function remember(p: ProjectInfo) {
  const entry: Recent = {
    root: p.root, name: p.name, collections: p.collections.length,
    items: p.collections.reduce((n, c) => n + c.items, 0),
    models: [...new Set(p.tables.map((t) => t.model_key))], warning: p.warning, opened: Date.now(),
  };
  local.set(KEY, [entry, ...recentProjects().filter((r) => r.root !== p.root)].slice(0, 30));
}

export function forget(root: string) { local.set(KEY, recentProjects().filter((r) => r.root !== root)); }

export function renameRecent(root: string, name: string) {
  local.set(KEY, recentProjects().map((r) => r.root === root ? { ...r, name } : r));
}

interface Props {
  current?: ProjectInfo;
  recent: Recent[];
  job?: JobInfo;
  onPick: (r: Recent) => void;
  onNew: () => void;
  onOpen: () => void;
  onForget: (root: string) => void;
}

/** All projects as cards, most recently opened first. */
export default function ProjectsPage({ current, recent, job, onPick, onNew, onOpen, onForget }: Props) {
  // A job in a project blocks switching to another one (the backend refuses); say so up front.
  const blocking = job?.state === "running" && job.project ? job : undefined;
  return <div className="page">
    <div className="page-head">
      <h1>Projects</h1>
      <div className="row">
        <button type="button" className="btn" onClick={onOpen} disabled={!!blocking}>Open project…</button>
        <button type="button" className="btn primary" onClick={onNew} disabled={!!blocking}><img src={plusIcon} alt="" aria-hidden="true" />New project</button>
      </div>
    </div>
    {blocking && <p className="note">“{blocking.label}” is running; other projects open when it finishes or is cancelled.</p>}
    {recent.length === 0 && <div className="empty-note empty-state"><LineArt kind="orbit" />
      <span className="dim">No projects yet. Open an existing .invectus folder, or create one. A project is a folder on this computer holding one dataset, its index and map, and your decisions.</span>
    </div>}
    <ul className="cards">
      {recent.map((r) => {
        const open = current?.root === r.root;
        const disabled = !!blocking && blocking.project !== r.root;
        return <li key={r.root} className="card">
          <button type="button" className="card-open" onClick={() => onPick(r)} disabled={disabled} aria-current={open ? "true" : undefined}>
            <span className="card-title">{r.name}</span>
            <span className="path">{r.root.replace(/^\/Users\/[^/]+/, "~")}</span>
            {r.warning && <span className="note small">Cloud-synced folder: sync can damage the database</span>}
          </button>
          <div className="card-foot">
            {open && <span className="rule-line" aria-hidden="true" />}
            <span>{fmt(r.items)} items</span>
            <span className="index-status"><span className={`square ${r.models.length ? "on" : ""}`} aria-hidden="true" />{r.models.length ? "Indexed" : "Not indexed"}</span>
          </div>
          <button type="button" className="card-forget" onClick={() => onForget(r.root)} aria-label={`Remove ${r.name} from the list`}>Remove</button>
        </li>;
      })}
    </ul>
  </div>;
}
