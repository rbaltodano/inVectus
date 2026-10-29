import { useEffect, useState } from "react";
import { api, fmt, type IndexPlan, type MissingModel, type ProjectInfo } from "./api";
import { ErrorLine, useStartJob } from "./App";
import { isRunning } from "./jobs";

/** Explicit consent to download an embedding model: what, from where, how big, and why the
 *  network is needed. Nothing downloads without the click, and never while offline. */
export function DownloadConsent({ model, project, onClose }: { model: MissingModel; project?: ProjectInfo; onClose?: () => void }) {
  const { start, busy, error, current } = useStartJob();
  const offline = !project || !project.network_enabled || project.offline_env;
  const downloading = isRunning(current) && current!.kind === "download" && current!.models.includes(model.key);
  return <section role="dialog" aria-modal="false" aria-labelledby={`dl-${model.key}`} className="panel stack gap consent">
    <h3 id={`dl-${model.key}`}>Download {model.key}?</h3>
    <dl className="facts mono small">
      <dt>Size</dt><dd>≈{fmt(model.megabytes)} MB</dd>
      <dt>From</dt><dd>huggingface.co/{model.repo}</dd>
      <dt>Version</dt><dd>pinned revision; every file's SHA-256 is checked</dd>
    </dl>
    <div className="small">This goes online once for this model. Indexing and search then run offline. Models are shared by all projects.</div>
    {offline && <div className="note small">{project?.offline_env ? "INVECTUS_OFFLINE is set, so downloads are refused." : project ? "This project is offline. Switch the network on (top right) to download." : "Open a project and switch its network on to download."}</div>}
    <ErrorLine error={error} />
    <div className="row end">
      {onClose && <button type="button" className="btn" onClick={onClose}>Not now</button>}
      <button type="button" className="btn primary" disabled={offline || busy || downloading || isRunning(current)} onClick={() => start(() => api.installModel(model.key))}>
        {downloading ? "Downloading…" : "Download"}</button>
    </div>
    {isRunning(current) && !downloading && <div className="muted small">Another job is running; download when it finishes.</div>}
  </section>;
}

/** The models indexing a collection will use (from the backend's plan). */
export function useIndexPlan(collection: string | undefined, deps: unknown[] = []) {
  const [plan, setPlan] = useState<IndexPlan>();
  const [error, setError] = useState<string>();
  useEffect(() => {
    if (!collection) return;
    api.indexPlan(collection).then((p) => { setPlan(p); setError(undefined); }).catch((e) => setError(String(e)));
  }, [collection, ...deps]);
  return { plan, error };
}
