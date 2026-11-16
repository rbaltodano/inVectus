// Settings → Storage (this computer): where new projects go and what AI agents may read; and,
// under Network, the open project's saved answers from outside sources.
import { useEffect, useState } from "react";
import { File, Folder } from "lucide-react";
import { api, inTauri, pickDirectory, type AgentGrant, type CacheUsage, type ProjectsDir, type PulledCopy } from "../api";
import { ago } from "../ago";
import { ErrorLine, useAction } from "../App";
import FolderPicker from "../FolderPicker";
import { FlickerPresence } from "../Modal";
import SettingsGroup from "../SettingsGroup";

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
    <SettingsGroup id="s-projects-dir" title="Projects folder" description="New projects go here. AI agents can always read this folder, so keep only projects in it.">
      <div className="storage-path-row">
        <Folder size={15} strokeWidth={1.5} aria-hidden="true" />
        <span className="mono storage-path" title={dir?.path}>{dir ? home(dir.path) : "…"}</span>
        {dir && !dir.standard && <button type="button" className="text-btn" disabled={action.busy} onClick={resetDir}>Use ~/inVectus Projects</button>}
        <button type="button" className="btn small" disabled={!inTauri || action.busy} onClick={changeDir}>Change…</button>
      </div>
      {dir?.warning && <div className="note small">{dir.warning}</div>}
    </SettingsGroup>
    <SettingsGroup id="s-agent-folders" title="Agent folders" description="Files and folders AI agents may read through inVectus, besides the projects folder and the open project. What they read can reach their provider.">
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
    </SettingsGroup>
    <FlickerPresence show={picking}>{picking && <FolderPicker onAllowed={refresh} onClose={() => setPicking(false)} />}</FlickerPresence>
  </>;
}

/** The open project's saved answers from outside sources, with a way to clear them; Settings
 *  explains them in the group heading. */
export function CheckCache() {
  const action = useAction();
  const [usage, setUsage] = useState<CacheUsage>();
  const [cleared, setCleared] = useState<CacheUsage>();
  useEffect(() => { api.checkCacheUsage().then(setUsage).catch(() => undefined); }, []);
  const clear = () => action.run(async () => { setCleared(await api.clearCheckCache()); setUsage(await api.checkCacheUsage()); });
  return <div className="storage-cache stack gap-s">
    <div className="row wrap storage-cache-row">
      <span className="small">{usage === undefined ? <span className="muted">Loading…</span> : usage.files
        ? `${usage.files} saved from outside sources · ${bytes(usage.bytes)}`
        : <span className="muted">None saved</span>}</span>
      <button type="button" className="btn small push" disabled={!usage?.files || action.busy} onClick={clear}>Clear</button>
    </div>
    {cleared?.files ? <div className="muted small" role="status">Cleared {cleared.files} ({bytes(cleared.bytes)}).</div> : null}
    <ErrorLine error={action.error} />
  </div>;
}

/** Copies pulled from integrations into the open project. They stay until removed here (or the
 *  project is deleted); a copy a collection was imported from is kept until that collection
 *  is removed. */
export function PulledCopies() {
  const action = useAction();
  const [copies, setCopies] = useState<PulledCopy[]>();
  useEffect(() => { api.pulledCopies().then(setCopies).catch(() => setCopies([])); }, []);
  const forget = (name: string) => action.run(async () => setCopies(await api.forgetPulledCopy(name)));
  if (copies === undefined) return <p className="muted small" role="status">Loading…</p>;
  return <div className="stack gap-s">
    {copies.length
      ? <ul className="storage-list">
        {copies.map((c) => <li key={c.name}>
          <File size={15} strokeWidth={1.5} aria-hidden="true" />
          <span className="storage-path" title={c.name}>
            {c.origin ?? c.name}
            <span className="dim small"> · {c.rows === null ? "" : `${c.rows} rows · `}{bytes(c.bytes)}{c.pulled_at ? ` · pulled ${ago(c.pulled_at * 1000)}` : ""}</span>
          </span>
          {c.used_by.length
            ? <span className="dim small" title={`Remove ${c.used_by.join(", ")} first to remove this copy`}>used by {c.used_by.join(", ")}</span>
            : <button type="button" className="text-btn" aria-label={`Remove the pulled copy ${c.origin ?? c.name}`} disabled={action.busy} onClick={() => forget(c.name)}>Remove</button>}
        </li>)}
      </ul>
      : <p className="dim small">None: nothing has been pulled from your integrations into this project.</p>}
    <ErrorLine error={action.error} />
  </div>;
}
