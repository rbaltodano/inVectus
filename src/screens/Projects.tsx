import type { CSSProperties } from "react";
import { ago } from "../ago";
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

/** All projects as a compact table, most recently opened first. */
export default function ProjectsPage({ current, recent, job, onPick, onNew, onOpen, onForget }: Props) {
  // A job in a project blocks switching to another one (the backend refuses); say so up front.
  const blocking = job?.state === "running" && job.project ? job : undefined;
  return <div className="page projects-page">
    <div className="page-head">
      <div className="projects-heading">
        <p className="eyebrow">{recent.length > 0 ? `${fmt(recent.length)} on this computer` : "On this computer"}</p>
        <h1>Projects</h1>
      </div>
      <div className="row">
        <button type="button" className="btn" onClick={onOpen} disabled={!!blocking}>Open project…</button>
        <button type="button" className="btn primary" onClick={onNew} disabled={!!blocking}><img src={plusIcon} alt="" aria-hidden="true" />New project</button>
      </div>
    </div>
    {blocking && <p className="note">“{blocking.label}” is running; other projects open when it finishes or is cancelled.</p>}
    {recent.length === 0 && <div className="empty-state"><LineArt kind="orbit" />
      <span className="dim">No projects yet. Open an existing .invectus folder, or create one. A project is a folder on this computer holding one dataset, its index and map, and your decisions.</span>
    </div>}
    {recent.length > 0 && <div className="project-table" role="table" aria-label="Projects">
      <div className="project-table-head" role="row">
        <span role="columnheader">Name</span>
        <span role="columnheader" className="num">Items</span>
        <span role="columnheader" className="num col-collections">Collections</span>
        <span role="columnheader" className="col-models">Models</span>
        <span role="columnheader">Opened</span>
        <span role="columnheader"><span className="sr-only">Actions</span></span>
      </div>
      {recent.map((r, i) => {
        const open = current?.root === r.root;
        const disabled = !!blocking && blocking.project !== r.root;
        const status = open ? "Open now" : r.models.length ? "Indexed" : "Not indexed";
        return <div key={r.root} role="row" className="project-trow" data-open={open || undefined} data-disabled={disabled || undefined}
          style={{ "--i": Math.min(i, 12) } as CSSProperties}>
          <div role="cell" className="project-tname">
            <span className={`mark ${open || r.models.length ? "full" : "ring"}`} title={status} aria-hidden="true" />
            <div className="project-tname-text">
              <button type="button" className="project-trow-open" onClick={() => onPick(r)} disabled={disabled}
                aria-current={open ? "true" : undefined} aria-label={`${r.name}, ${status}`}>
                {r.name}
              </button>
              <span className="path">{r.root.replace(/^\/Users\/[^/]+/, "~")}</span>
              {r.warning && <span className="note small">Cloud-synced folder: sync can damage the database</span>}
            </div>
            {open && <span className="project-row-state">Open</span>}
          </div>
          <span role="cell" className="num">{r.items ? fmt(r.items) : "—"}</span>
          <span role="cell" className="num col-collections">{fmt(r.collections)}</span>
          <span role="cell" className={`col-models ${r.models.length ? "models" : "dim"}`} title={r.models.join(", ") || undefined}>
            {r.models.length ? r.models.join(", ") : "—"}</span>
          <span role="cell" className="dim">{ago(r.opened)}</span>
          <span role="cell" className="project-tactions">
            <button type="button" className="text-btn project-row-forget" onClick={() => onForget(r.root)} aria-label={`Remove ${r.name} from the list`}>Remove</button>
          </span>
        </div>;
      })}
    </div>}
  </div>;
}
