// Settings → Storage (this computer): where new projects go and what AI agents may read; and,
// under Network, the open project's saved answers from outside sources.
import { useEffect, useState } from "react";
import { File, Folder } from "lucide-react";
import { api, inTauri, pickDirectory, type AgentGrant, type CacheUsage, type ProjectsDir } from "../api";
import { ErrorLine, useAction } from "../App";
import FolderPicker from "../FolderPicker";
import { FlickerPresence } from "../Modal";

const home = (p: string) => p.replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, "~");

export function bytes(n: number) {
  if (n < 1024) return `${n} B`;
  if (n < 1024 ** 2) return `${(n / 1024).toFixed(1)} KB`;
  if (n < 1024 ** 3) return `${(n / 1024 ** 2).toFixed(1)} MB`;
  return `${(n / 1024 ** 3).toFixed(2)} GB`;
}

export default function StorageSettings() {
  const action = useAction();
  const [dir, setDir] = useState<ProjectsDir>();
  const [folders, setFolders] = useState<AgentGrant[]>([]);
  const [picking, setPicking] = useState(false);
  const refresh = () => {
    api.projectsDir().then(setDir).catch(() => undefined);
    api.agentFolders().then(setFolders).catch(() => undefined);
  };
  useEffect(refresh, []);

  const changeDir = () => action.run(async () => {
    const p = await pickDirectory(dir?.path, "Choose where new projects go");
    if (p) setDir(await api.setProjectsDir(p));
  });
  const resetDir = () => action.run(async () => setDir(await api.setProjectsDir(null)));
  const remove = (path: string) => action.run(async () => { await api.revokeAgentFolder(path); refresh(); });

  return <>
    <section aria-labelledby="s-projects-dir" className="panel settings-group stack gap">
      <div className="settings-group-heading"><h3 id="s-projects-dir">Projects folder</h3>
        <p>New projects go here. AI agents can always read this folder, so keep only projects in it.</p></div>
      <div className="storage-path-row">
        <Folder size={15} strokeWidth={1.5} aria-hidden="true" />
        <span className="mono storage-path" title={dir?.path}>{dir ? home(dir.path) : "…"}</span>
        {dir && !dir.standard && <button type="button" className="text-btn" disabled={action.busy} onClick={resetDir}>Use ~/inVectus Projects</button>}
        <button type="button" className="btn small" disabled={!inTauri || action.busy} onClick={changeDir}>Change…</button>
      </div>
      {dir?.warning && <div className="note small">{dir.warning}</div>}
    </section>
    <section aria-labelledby="s-agent-folders" className="panel settings-group stack gap">
      <div className="settings-group-heading"><h3 id="s-agent-folders">Agent folders</h3>
        <p>Files and folders AI agents may read through inVectus, besides the projects folder and the open project. What they read can reach their provider.</p></div>
      {folders.length
        ? <ul className="storage-list">
          {folders.map((g) => <li key={g.path}>
            {g.dir || g.missing ? <Folder size={15} strokeWidth={1.5} aria-hidden="true" /> : <File size={15} strokeWidth={1.5} aria-hidden="true" />}
            <span className="mono storage-path" title={g.path}>{home(g.path)}</span>
            {g.missing && <span className="dim small">not found</span>}
            <button type="button" className="text-btn" aria-label={`Stop agents reading ${home(g.path)}`} disabled={action.busy} onClick={() => remove(g.path)}>Remove</button>
          </li>)}
        </ul>
        : <p className="dim small">None yet: agents can read only the projects folder and the open project.</p>}
      <div><button type="button" className="btn small" disabled={!inTauri} onClick={() => setPicking(true)}>Add a file or folder…</button></div>
      <ErrorLine error={action.error} />
    </section>
    <FlickerPresence show={picking}>{picking && <FolderPicker onAllowed={refresh} onClose={() => setPicking(false)} />}</FlickerPresence>
  </>;
}

/** The open project's saved answers from outside sources, with a way to clear them. */
export function CheckCache() {
  const action = useAction();
  const [usage, setUsage] = useState<CacheUsage>();
  const [cleared, setCleared] = useState<CacheUsage>();
  useEffect(() => { api.checkCacheUsage().then(setUsage).catch(() => undefined); }, []);
  const clear = () => action.run(async () => { setCleared(await api.clearCheckCache()); setUsage(await api.checkCacheUsage()); });
  return <div className="storage-cache stack gap-s">
    <div className="row wrap storage-cache-row">
      <span className="small"><b>Saved answers</b> <span className="muted">{usage === undefined ? "…" : usage.files
        ? `${usage.files} from outside sources · ${bytes(usage.bytes)}`
        : "none"}</span></span>
      <button type="button" className="btn small push" disabled={!usage?.files || action.busy} onClick={clear}>Clear</button>
    </div>
    <div className="muted small">Answers are kept in the project for 7 days, so checking a gap again needs no network. Clear them to ask the sources again.{cleared?.files ? ` Cleared ${cleared.files} (${bytes(cleared.bytes)}).` : ""}</div>
    <ErrorLine error={action.error} />
  </div>;
}
