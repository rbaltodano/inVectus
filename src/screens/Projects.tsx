import { useEffect, useState } from "react";
import { api, fmt, inTauri, local, when, type ProjectInfo } from "../api";
import { ErrorLine, useAction } from "../App";

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

interface Props {
  mode?: "create" | "open";
  current?: ProjectInfo;
  recent: Recent[];
  onOpened: (p: ProjectInfo, created?: boolean) => void;
  onForget: (root: string) => void;
  onPick: (r: Recent) => void;
}

export default function Home({ mode, current, recent, onOpened, onForget, onPick }: Props) {
  const [name, setName] = useState("");
  const [dir, setDir] = useState("");
  const [openPath, setOpenPath] = useState("");
  const create = useAction();
  const open = useAction();

  useEffect(() => { if (inTauri) api.defaultDir().then(setDir).catch(() => undefined); }, []);
  useEffect(() => { document.getElementById(mode === "open" ? "open-path" : "new-name")?.focus(); }, [mode]);

  const newPath = `${dir.replace(/\/$/, "")}/${name.trim()}`;
  const synced = /Mobile Documents|CloudStorage|iCloud|OneDrive|Dropbox|Google Drive/.test(dir);

  return <div className="page">
    <div className="title-row"><h1>Projects</h1><span className="label">Local · one folder per project</span></div>

    {recent.length === 0 && <section className="panel empty">
      <span className="ring" aria-hidden="true" />
      <h2>No projects yet</h2>
      <p className="muted">A project is a folder on this computer that holds your collections, their indexes, regions, gaps and decisions. Create one, or open an existing <code>.invectus</code> folder.</p>
    </section>}

    <div className="split">
      <section aria-labelledby="np" className="panel stack gap">
        <h2 id="np">New project</h2>
        <label>Name<input id="new-name" value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. retina-reviews" /></label>
        <label>Location<input value={dir} onChange={(e) => setDir(e.target.value)} /></label>
        <div className={`small ${synced ? "note" : "muted"}`}>{synced ? "This looks like a cloud-synced folder; sync can damage project databases. Prefer a local folder." : "Everything stays on this computer. Avoid iCloud, Dropbox and OneDrive folders."}</div>
        <div className="muted small mono clip">{name.trim() ? `${newPath}.invectus` : ""}</div>
        <ErrorLine error={create.error} />
        <button type="button" className="btn primary" disabled={!name.trim() || !dir.trim() || create.busy}
          onClick={() => create.run(async () => onOpened(await api.open(newPath, true), true))}>{create.busy ? "Creating…" : "Create project"}</button>
        <div className="muted small">Next you can add the first collection, or do it later.</div>
      </section>

      <section aria-labelledby="op" className="panel stack gap">
        <h2 id="op">Open existing</h2>
        <label>Project folder<input id="open-path" value={openPath} onChange={(e) => setOpenPath(e.target.value)} placeholder="~/inVectus Projects/demo.invectus"
          onKeyDown={(e) => e.key === "Enter" && openPath.trim() && open.run(async () => onOpened(await api.open(openPath.trim(), false)))} /></label>
        <ErrorLine error={open.error} />
        <button type="button" className="btn" disabled={!openPath.trim() || open.busy}
          onClick={() => open.run(async () => onOpened(await api.open(openPath.trim(), false)))}>{open.busy ? "Opening…" : "Open"}</button>
      </section>
    </div>

    {recent.length > 0 && <section aria-labelledby="rp" className="panel flush">
      <div className="panel-head"><h2 id="rp">Recent</h2></div>
      <div className="scroll-x"><table className="table">
        <thead><tr><th>Name</th><th>Contents</th><th>Last opened</th><th><span className="sr-only">Actions</span></th></tr></thead>
        <tbody>{recent.map((r) => <tr key={r.root}>
          <td><button type="button" className="link strong" onClick={() => onPick(r)}>{r.name}</button>
            <div className="muted small mono clip path" title={r.root}>{r.root}</div>
            {r.warning && <div className="tag">Cloud-synced folder: sync can damage the database</div>}</td>
          <td className="mono small">{r.collections} coll. · {fmt(r.items)} items<div className="muted">{r.models.length ? `${r.models.length} model${r.models.length === 1 ? "" : "s"}` : "not indexed"}</div></td>
          <td className="small">{when(r.opened)}{current?.root === r.root && <div className="muted">open now</div>}</td>
          <td className="right"><button type="button" className="link muted small" onClick={() => onForget(r.root)}>Remove from list</button></td>
        </tr>)}</tbody>
      </table></div>
    </section>}
  </div>;
}
