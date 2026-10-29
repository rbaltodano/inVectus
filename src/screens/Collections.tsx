import { useEffect, useState } from "react";
import { api, fmt, when, STATUS_LABEL, type Collection, type IndexStatus, type ModelInfo, type LlmInfo, type TableInfo } from "../api";
import { ErrorLine, useStartJob, type Ctx } from "../App";
import { JobProgress, isRunning, useJobs } from "../jobs";
import { DownloadConsent, useIndexPlan } from "../models";
import Import from "./Import";

/** A status mark: filled = done, half = partial or running, ring = nothing yet, cross = failed. */
export function StatusMark({ status }: { status: IndexStatus | "none" }) {
  const shape = status === "indexed" || status === "imported" ? "full"
    : status === "indexing" || status === "outdated" || status === "interrupted" || status === "cancelled" || status === "unknown" ? "half"
      : status === "failed" ? "cross" : "ring";
  return <span className={`mark ${shape}`} aria-hidden="true" />;
}

export default function Collections({ ctx, focus, adding: startAdding }: { ctx: Ctx; focus?: string; adding?: boolean }) {
  const { project } = ctx;
  const [adding, setAdding] = useState(!!startAdding);
  const [open, setOpen] = useState<string | undefined>(focus ?? (project.collections.length === 1 ? project.collections[0].collection_id : undefined));
  useEffect(() => { setAdding(!!startAdding); }, [startAdding]);

  if (adding) return <Import ctx={ctx} onCancel={() => setAdding(false)}
    onDone={(c) => { setAdding(false); setOpen(c); }} />;

  return <div className="page">
    <div className="title-row">
      <h2 className="h1">Collections</h2>
      <span className="label">{project.collections.length} in this project</span>
      <button type="button" className="btn primary push" onClick={() => setAdding(true)}>+ Add collection</button>
    </div>
    <p className="muted small">Source files are not watched. To bring in changes, use “Update from source”, which imports the same file or folder again and re-indexes only what changed.</p>
    {project.collections.length === 0 && <section className="panel empty">
      <span className="ring" aria-hidden="true" />
      <h2>No collections yet</h2>
      <p className="muted">A collection is a set of items (papers, notes, products) imported from a CSV, Parquet file or folder of text files. It is indexed automatically after import.</p>
      <button type="button" className="btn primary" onClick={() => setAdding(true)}>Add the first collection</button>
    </section>}
    {project.collections.map((c) => <CollectionPanel key={c.collection_id} ctx={ctx} c={c}
      open={open === c.collection_id} onToggle={() => setOpen(open === c.collection_id ? undefined : c.collection_id)} />)}
  </div>;
}

function CollectionPanel({ ctx, c, open, onToggle }: { ctx: Ctx; c: Collection; open: boolean; onToggle: () => void }) {
  const { project } = ctx;
  const jobs = useJobs();
  const job = jobs.current;
  const { start, busy, error, run } = useStartJob();
  const tables = project.tables.filter((t) => t.collection === c.collection_id);
  const indexingHere = isRunning(job) && job!.kind === "index" && job!.project === project.root && job!.collection === c.collection_id;
  const lastHere = !isRunning(job) && job?.kind === "index" && job.project === project.root && job.collection === c.collection_id ? job : undefined;
  const { plan } = useIndexPlan(c.collection_id, [project.tables.length, job?.state]);
  const [consent, setConsent] = useState<string>();
  const [reimported, setReimported] = useState<string>();

  const index = (models: string[] | null) => start(() => api.startIndex(c.collection_id, models));
  const reimport = () => run(async () => {
    const out = await api.reimport(c.collection_id);
    if (out.job) jobs.started(out.job);
    const r = out.report;
    setReimported(`${fmt(r.inserted)} new, ${fmt(r.updated)} changed, ${fmt(r.unchanged)} unchanged${out.index_error ? ` · indexing did not start: ${out.index_error}` : ""}${!out.job && out.plan.missing.length ? " · indexing needs a model (below)" : ""}`);
    await ctx.refresh();
  });
  const total = c.items + c.chunks;
  const overall: IndexStatus | "none" = indexingHere ? "indexing" : tables.length === 0 ? "none" : tables.some((t) => t.index_status === "indexed" || t.index_status === "imported") ? (tables.every((t) => t.index_status === "indexed" || t.index_status === "imported") ? "indexed" : "outdated") : tables[0].index_status;

  return <section className="panel stack gap" aria-labelledby={`c-${c.collection_id}`}>
    <div className="row wrap">
      <StatusMark status={overall} />
      <h3 id={`c-${c.collection_id}`} className="mono">{c.collection_id}</h3>
      <span className="muted small mono">{fmt(c.items)} items · {fmt(total)} texts with chunks</span>
      <button type="button" className="link push" aria-expanded={open} onClick={onToggle}>{open ? "Hide details" : "Details"}</button>
    </div>
    <div className="muted small mono">
      {c.source_kind ? `${c.source_kind}` : "source unknown"}{c.source_path ? ` · ${c.source_path}` : ""}{c.updated_at ? ` · last imported ${when(c.updated_at)}` : ""}
    </div>
    <RoleBar c={c} />

    {indexingHere && <div className="activity"><div className="small"><b>Indexing</b> with {job!.models.join(", ")}</div><JobProgress job={job!} />
      <button type="button" className="btn small start" onClick={() => api.cancel()}>Cancel</button></div>}
    {lastHere?.state === "failed" && <div className="error stack gap-s"><span>Indexing failed: {lastHere.error}</span>
      <button type="button" className="btn small start" disabled={busy || isRunning(job)} onClick={() => index(lastHere.models)}>Retry</button></div>}
    {lastHere?.state === "cancelled" && <div className="note small">Indexing was cancelled. Finished batches were kept; indexing again resumes where it stopped.</div>}

    <table className="table compact">
      <thead><tr><th>Index</th><th>Model</th><th>Vectors</th><th>State</th><th><span className="sr-only">Actions</span></th></tr></thead>
      <tbody>
        {tables.map((t) => <SpaceRow key={t.space_id} t={t} total={total} busy={busy || isRunning(job)} indexingHere={indexingHere} onIndex={() => index([t.model_key])} />)}
        {tables.length === 0 && !indexingHere && <tr><td colSpan={5} className="muted small">Not indexed yet. Search, regions and gaps need an index.</td></tr>}
      </tbody>
    </table>

    {plan && plan.missing.length > 0 && !indexingHere && <div className="stack gap-s">
      <div className="note small">{tables.length === 0 ? "No embedding model is installed, so this collection was not indexed." : `This collection has an index made with ${plan.missing.map((m) => m.key).join(", ")}, which is not installed here; that index cannot be updated.`} Nothing downloads without your consent.</div>
      {plan.missing.map((m) => consent === m.key ? <DownloadConsent key={m.key} model={m} project={project} onClose={() => setConsent(undefined)} />
        : <button key={m.key} type="button" className="btn start" onClick={() => setConsent(m.key)}>Download {m.key}…</button>)}
    </div>}
    {plan?.notes.map((n) => <div key={n} className="muted small">{n}</div>)}

    <div className="row wrap">
      {tables.length === 0 && plan && plan.models.length > 0 && <button type="button" className="btn primary" disabled={busy || isRunning(job)} onClick={() => index(null)}>Index with {plan.models.join(", ")}</button>}
      {c.reimportable && <button type="button" className="btn" disabled={busy || isRunning(job)} onClick={reimport}
        title={c.source_path ? `Imports ${c.source_path} again with the same columns and roles` : undefined}>Update from source</button>}
      {tables.some((t) => t.origin === "embedded") && <button type="button" className="btn" onClick={() => { const t = tables[0]; ctx.pickTable(t); ctx.go({ view: "project", tab: "search" }); }}>Search this collection</button>}
    </div>
    {reimported && <div className="small mono">Updated: {reimported}</div>}
    <ErrorLine error={error} />
    {open && <Details ctx={ctx} c={c} tables={tables} />}
  </section>;
}

function SpaceRow({ t, total, busy, indexingHere, onIndex }: { t: TableInfo; total: number; busy: boolean; indexingHere: boolean; onIndex: () => void }) {
  const status: IndexStatus = indexingHere && t.index_status !== "imported" ? "indexing" : t.index_status;
  const resumable = ["interrupted", "cancelled", "outdated", "failed", "unknown"].includes(status) && t.origin === "embedded";
  return <tr>
    <td><StatusMark status={status} /></td>
    <td className="mono small">{t.model_key}<div className="muted">{t.dimensions}-d · space {t.space_id.slice(0, 12)}</div></td>
    <td className="mono small">{fmt(t.vectors)}{t.origin === "embedded" && <span className="muted"> / {fmt(total)}</span>}</td>
    <td className="small">{STATUS_LABEL[status]}{t.indexed_at && status !== "indexing" && <div className="muted">{when(t.indexed_at)}</div>}
      {status === "failed" && t.index_error && <div className="error small">{t.index_error}</div>}
      {status === "unknown" && <div className="muted">No index job is recorded for these vectors.</div>}</td>
    <td className="right">{resumable && <button type="button" className="btn small" disabled={busy} onClick={onIndex}>{status === "failed" ? "Retry" : status === "outdated" ? "Index changes" : "Resume"}</button>}</td>
  </tr>;
}

function RoleBar({ c }: { c: Collection }) {
  const d = c.by_role.demand ?? 0, v = c.by_role.coverage ?? 0, n = c.by_role.neutral ?? 0, all = Math.max(1, d + v + n);
  return <div className="stack gap-s">
    <div className="rolebar" aria-hidden="true">
      <div className="demand" style={{ width: `${(d / all) * 100}%` }} /><div className="coverage" style={{ width: `${(v / all) * 100}%` }} />
    </div>
    <div className="row wrap small mono"><span><i className="swatch demand" />{fmt(d)} demand</span><span><i className="swatch coverage" />{fmt(v)} coverage</span><span><i className="swatch neutral" />{fmt(n)} neutral</span></div>
  </div>;
}

/** Advanced controls: another model's index, calibration and regions per space. */
function Details({ ctx, c, tables }: { ctx: Ctx; c: Collection; tables: TableInfo[] }) {
  const { project } = ctx;
  const { start, busy, error, current } = useStartJob();
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [llms, setLlms] = useState<LlmInfo[]>([]);
  const [consent, setConsent] = useState<ModelInfo>();
  useEffect(() => { api.models().then(setModels).catch(() => undefined); api.llms().then(setLlms).catch(() => undefined); }, [current?.state]);
  const running = isRunning(current);
  const llmReady = llms.some((l) => l.installed);
  const others = models.filter((m) => !tables.some((t) => t.model_key === m.key));

  return <div className="details stack gap">
    {tables.length > 0 && <div className="label">Per index</div>}
    {tables.map((t) => <div key={t.space_id} className="stack gap-s sub">
      <div className="mono small strong">{t.model_key} · {t.space_id.slice(0, 12)}</div>
      <div className="row wrap small">
        <span className="grow">Calibration: {t.calibration ? `${t.calibration.method}, ${fmt(t.calibration.sample)} items${t.calibration.holdout_auc ? `, held-out AUC ${t.calibration.holdout_auc.toFixed(3)}` : ""}${t.calibration.vectors !== t.vectors ? " — the collection changed since" : ""}` : "none (search shows similarity only, no verdict)"}</span>
        <button type="button" className="btn small" disabled={running || busy || t.vectors === 0} onClick={() => start(() => api.startCalibration({ collection: c.collection_id, space: t.space_id }, false))}>{t.calibration ? "Recalibrate" : "Calibrate"}</button>
        {llmReady && <button type="button" className="btn small" disabled={running || busy || t.vectors === 0} onClick={() => start(() => api.startCalibration({ collection: c.collection_id, space: t.space_id }, true))}>With language model</button>}
      </div>
      <div className="row wrap small">
        <span className="grow">Regions: {t.regions ? `${fmt(Number(t.regions.stats.fine ?? 0))} regions, ${when(t.regions.created_at)}${t.regions.scanned ? "; gaps scored" : ""}` : "none (the map and gaps need them)"}</span>
        <button type="button" className="btn small" disabled={running || busy || t.vectors === 0} onClick={() => start(() => api.startRegions({ collection: c.collection_id, space: t.space_id }))}>{t.regions ? "Find again" : "Find regions"}</button>
      </div>
      {isRunning(current) && current!.collection === c.collection_id && current!.space === t.space_id && <JobProgress job={current!} />}
    </div>)}
    {others.length > 0 && <>
      <div className="label">Index with another model</div>
      <div className="muted small">Each model keeps its own index; nothing is combined or overwritten.</div>
      {others.map((m) => <div key={m.key} className="row wrap small">
        <span className="mono grow">{m.key} <span className="muted">· {m.dimensions}-d · ≈{fmt(m.megabytes)} MB · {m.installed ? "installed" : "not installed"}</span></span>
        {m.installed ? <button type="button" className="btn small" disabled={running || busy} onClick={() => start(() => api.startIndex(c.collection_id, [m.key]))}>Index</button>
          : <button type="button" className="btn small" onClick={() => setConsent(m)}>Download…</button>}
      </div>)}
      {consent && <DownloadConsent model={{ key: consent.key, repo: consent.repo, megabytes: consent.megabytes }} project={project} onClose={() => setConsent(undefined)} />}
    </>}
    <ErrorLine error={error} />
  </div>;
}
