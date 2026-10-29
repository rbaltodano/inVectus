import { useEffect, useState } from "react";
import { api, fmt, inTauri, local, type ProjectInfo } from "../api";
import { ErrorLine, useAction } from "../App";

interface Recent { root: string; name: string; collections: number; items: number; models: string[]; warning: string | null; opened: number }
const KEY = "invectus.recent";

/** Add a project to the recent list (browser storage; the list is a convenience only). */
export function remember(p: ProjectInfo) {
  const entry: Recent = {
    root: p.root, name: p.name, collections: p.collections.length,
    items: p.collections.reduce((n, c) => n + c.items, 0),
    models: [...new Set(p.tables.map((t) => t.model_key))], warning: p.warning, opened: Date.now(),
  };
  local.set(KEY, [entry, ...local.get<Recent[]>(KEY, []).filter((r) => r.root !== p.root)].slice(0, 20));
}

function when(ms: number) {
  const days = Math.floor((Date.now() - ms) / 86_400_000);
  return days === 0 ? "Today" : days === 1 ? "Yesterday" : new Date(ms).toLocaleDateString();
}

export default function Projects({ onOpen, current }: { onOpen: (p: ProjectInfo) => void; current?: ProjectInfo }) {
  const [recent, setRecent] = useState(() => local.get<Recent[]>(KEY, []));
  const [name, setName] = useState("");
  const [dir, setDir] = useState("");
  const [openPath, setOpenPath] = useState("");
  const [showOpen, setShowOpen] = useState(false);
  const { busy, error, run } = useAction();

  useEffect(() => { if (inTauri) api.defaultDir().then(setDir).catch(() => undefined); }, []);
  useEffect(() => { setRecent(local.get<Recent[]>(KEY, [])); }, [current]);

  const open = (path: string, create: boolean) => run(async () => onOpen(await api.open(path, create)));
  const forget = (root: string) => { const next = recent.filter((r) => r.root !== root); local.set(KEY, next); setRecent(next); };
  const newPath = `${dir.replace(/\/$/, "")}/${name.trim()}`;

  return <div className="page">
    <div className="title-row">
      <h1>Projects</h1>
      <div className="row">
        <button type="button" className="btn" onClick={() => setShowOpen(!showOpen)} aria-expanded={showOpen}>Open existing…</button>
      </div>
    </div>
    <ErrorLine error={error} />
    {showOpen && <section className="card row wrap">
      <label className="grow">Project folder
        <input value={openPath} onChange={(e) => setOpenPath(e.target.value)} placeholder="~/inVectus Projects/demo.invectus" />
      </label>
      <button type="button" className="btn primary" disabled={!openPath.trim() || busy} onClick={() => open(openPath.trim(), false)}>Open</button>
    </section>}

    <div className="split">
      <section aria-label="Recent projects" className="card flush grow-2 scroll-x">
        {recent.length === 0
          ? <p className="pad muted">No recent projects. Create one, or open an existing <code>.invectus</code> folder.</p>
          : <table className="table fluid">
            <thead><tr><th>Name</th><th>Contents</th><th>Last opened</th><th><span className="sr-only">Actions</span></th></tr></thead>
            <tbody>{recent.map((r) => <tr key={r.root}>
              <td>
                <button type="button" className="link strong" onClick={() => open(r.root, false)}>{r.name}</button>
                <div className="muted small clip path" title={r.root}>{r.root}</div>
                {r.warning && <div className="chip warn-chip">In a cloud-synced folder: sync can damage the database</div>}
              </td>
              <td>{r.collections} collection{r.collections === 1 ? "" : "s"} · {fmt(r.items)} items<div className="muted small">{r.models.length ? `${r.models.length} model${r.models.length === 1 ? "" : "s"}` : "not indexed"}</div></td>
              <td>{when(r.opened)}{current?.root === r.root && <div className="muted small">open now</div>}</td>
              <td className="right nowrap">
                <button type="button" className="link" onClick={() => open(r.root, false)}>Open</button>{" · "}
                <button type="button" className="link muted" onClick={() => forget(r.root)}>Forget</button>
              </td>
            </tr>)}</tbody>
          </table>}
      </section>

      <section aria-labelledby="np" className="card stack gap">
        <h2 id="np">New project</h2>
        <label>Name<input value={name} onChange={(e) => setName(e.target.value)} placeholder="e.g. retina-reviews" /></label>
        <label>Location<input value={dir} onChange={(e) => setDir(e.target.value)} /></label>
        <div className="muted small">Everything stays on this computer. Avoid iCloud, Dropbox and OneDrive folders.</div>
        <button type="button" className="btn primary" disabled={!name.trim() || !dir.trim() || busy} onClick={() => open(newPath, true)}>
          {busy ? "Creating…" : "Create and import data"}
        </button>
      </section>
    </div>
  </div>;
}
