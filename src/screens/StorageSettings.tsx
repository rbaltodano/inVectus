// Settings → Storage (this computer): where new projects go and what AI agents may read; and,
// under Network, the open project's saved answers from outside sources.
import { useEffect, useState } from "react";
import { File, Folder } from "lucide-react";
import { api, inTauri, pickDirectory, type AgentGrant, type CacheUsage, type KeyStatus, type ProjectsDir, type PulledCopy } from "../api";
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

/** The open project's data key: reveal its recovery code, or restore the key from one. Nothing
 *  is encrypted with it yet; the code matters once encryption is on. */
export function RecoveryCode() {
  const action = useAction();
  const [status, setStatus] = useState<KeyStatus>();
  const [code, setCode] = useState<string>();
  const [typed, setTyped] = useState("");
  const [copied, setCopied] = useState(false);
  useEffect(() => { api.projectKeyStatus().then(setStatus).catch(() => undefined); }, []);
  const reveal = () => action.run(async () => setCode(await api.projectRecoveryCode()));
  const copy = () => action.run(async () => { if (code) { await navigator.clipboard.writeText(code); setCopied(true); } });
  const restore = () => action.run(async () => { setStatus(await api.restoreProjectKey(typed)); setTyped(""); });
  if (!status) return <p className="muted small" role="status">Loading…</p>;
  return <div className="stack gap-s">
    {status.state === "no-key" && <p className="dim small">This project was made before projects had keys. It will get one when encryption is turned on for existing projects.</p>}
    {status.state === "available" && <>
      <p className="small">The key is {status.source === "keychain" ? "in this computer’s keychain" : status.source === "key-file" ? "in the developer key folder (INVECTUS_KEYSTORE_DIR)" : "set by INVECTUS_PROJECT_KEY"}.</p>
      {code
        ? <div className="row wrap storage-cache-row">
          <code className="mono small recovery-code" aria-label="Recovery code">{code}</code>
          <button type="button" className="btn small" disabled={action.busy} onClick={copy}>{copied ? "Copied" : "Copy"}</button>
          <button type="button" className="text-btn" onClick={() => { setCode(undefined); setCopied(false); }}>Hide</button>
        </div>
        : <div><button type="button" className="btn small" disabled={action.busy} onClick={reveal}>Show recovery code</button></div>}
    </>}
    {(status.state === "missing" || status.state === "unusable") && <>
      <p className="small">{status.state === "missing" ? "This project’s key is not on this computer." : `The stored key can’t be used: ${status.reason}`} Enter the project’s recovery code to keep its key here.</p>
      <div className="row wrap storage-cache-row">
        <input className="mono" aria-label="Recovery code" placeholder="XXXXX-XXXXX-…" value={typed} onChange={(e) => setTyped(e.target.value)} autoComplete="off" spellCheck={false} />
        <button type="button" className="btn small" disabled={action.busy || !typed.trim()} onClick={restore}>Restore</button>
      </div>
    </>}
    <ErrorLine error={action.error} />
  </div>;
}
