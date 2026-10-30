import { useEffect, useState } from "react";
import { api, fmt, when, STATUS_LABEL, type Collection, type IndexStatus, type ModelInfo, type LlmInfo, type TableInfo } from "../api";
import { ErrorLine, useStartJob, type Ctx } from "../App";
import { JobProgress, isRunning, useJobs } from "../jobs";
import { DownloadConsent, useIndexPlan } from "../models";
import LineArt from "../LineArt";

/** A status mark: filled = done, half = partial or running, ring = nothing yet, cross = failed. */
export function StatusMark({ status }: { status: IndexStatus | "none" }) {
  const shape = status === "indexed" || status === "imported" ? "full"
    : status === "indexing" || status === "outdated" || status === "interrupted" || status === "cancelled" || status === "unknown" ? "half"
      : status === "failed" ? "cross" : "ring";
  return <span className={`mark ${shape}`} aria-hidden="true" />;
}

export default function Collections({ ctx, focus }: { ctx: Ctx; focus?: string }) {
  const { project } = ctx;
  const [open, setOpen] = useState<string | undefined>(focus ?? (project.collections.length === 1 ? project.collections[0].collection_id : undefined));
  return <div className="page collections">
    <div className="row">
      <span className="section">Collections{project.collections.length ? ` · ${fmt(project.collections.length)}` : ""}</span>
    </div>
    {project.collections.length === 0 && <div className="empty-state">
      <LineArt kind="grid" />
      <span className="dim">A collection is a set of items (papers, notes, products) imported from a CSV, Parquet file or folder of text files. It is indexed automatically after import.</span>
      <span className="dim small">Choose “Add to Dataset” above to import your first collection.</span>
    </div>}
    {project.collections.length > 0 && <div className="collection-list">
    {project.collections.map((c) => <CollectionPanel key={c.collection_id} ctx={ctx} c={c}
      open={open === c.collection_id} onToggle={() => setOpen(open === c.collection_id ? undefined : c.collection_id)} />)}
    </div>}
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

  const index = (models: string[] | null, map = false) => start(() => api.startIndex(c.collection_id, models, map));
  const reimport = () => run(async () => {
    const out = await api.reimport(c.collection_id);
    if (out.job) jobs.started(out.job);
    const r = out.report;
    setReimported(`${fmt(r.inserted)} new, ${fmt(r.updated)} changed, ${fmt(r.unchanged)} unchanged${out.index_error ? ` · indexing did not start: ${out.index_error}` : ""}${!out.job && out.plan.missing.length ? " · indexing needs a model (below)" : ""}`);
    await ctx.refresh();
  });
  const overall: IndexStatus | "none" = indexingHere ? "indexing" : tables.length === 0 ? "none" : tables.some((t) => t.index_status === "indexed" || t.index_status === "imported") ? (tables.every((t) => t.index_status === "indexed" || t.index_status === "imported") ? "indexed" : "outdated") : tables[0].index_status;

  return <section className="collection-row stack gap" data-open={open} aria-labelledby={`c-${c.collection_id}`}>
    <div className="collection-title">
      <h3 id={`c-${c.collection_id}`}>{c.collection_id}</h3>
      {overall !== "indexed" && overall !== "imported" && <span className="index-status"><StatusMark status={overall} />{overall === "none" ? "Not indexed" : overall === "outdated" ? "Partly indexed" : STATUS_LABEL[overall]}</span>}
    </div>
    <dl className="figures">
      <div><dt>Items</dt><dd>{fmt(c.items)}</dd></div>
      <div><dt>Demand</dt><dd>{fmt(c.by_role.demand ?? 0)}</dd></div>
      <div><dt>Coverage</dt><dd>{fmt(c.by_role.coverage ?? 0)}</dd></div>
      {(c.by_role.neutral ?? 0) > 0 && <div><dt>Neutral</dt><dd>{fmt(c.by_role.neutral!)}</dd></div>}
    </dl>

    {indexingHere && <div className="activity"><div className="small">Indexing with <span className="mono">{job!.models.join(", ")}</span></div><JobProgress job={job!} />
      <button type="button" className="text-btn" onClick={() => api.cancel()}>Cancel</button></div>}
    {lastHere?.state === "failed" && <div className="error stack gap-s"><span>Indexing failed: {lastHere.error}</span>
      <button type="button" className="text-btn" disabled={busy || isRunning(job)} onClick={() => index(lastHere.models, true)}>Retry</button></div>}
    {lastHere?.state === "cancelled" && <div className="note small">Indexing was cancelled. Finished batches were kept; indexing again resumes where it stopped.</div>}

    {tables.length === 0 && !indexingHere && <p className="dim">Not indexed yet. Search, regions and gaps need an index.</p>}
    {tables.map((t) => <SpaceRow key={t.space_id} t={t} busy={busy || isRunning(job)} indexingHere={indexingHere} onIndex={() => index([t.model_key])} />)}

    {plan && plan.missing.length > 0 && !indexingHere && <div className="stack gap-s">
      <div className="note small">{tables.length === 0 ? "No embedding model is installed, so this collection was not indexed." : `This collection has an index made with ${plan.missing.map((m) => m.key).join(", ")}, which is not installed here; that index cannot be updated.`} Nothing downloads without your consent.</div>
      {plan.missing.map((m) => consent === m.key ? <DownloadConsent key={m.key} model={m} project={project} onClose={() => setConsent(undefined)} />
        : <button key={m.key} type="button" className="btn start" onClick={() => setConsent(m.key)}>Download {m.key}…</button>)}
    </div>}
    {plan?.notes.map((n) => <div key={n} className="muted small">{n}</div>)}

    {tables.length === 0 && plan && plan.models.length > 0 && <button type="button" className="btn primary start" disabled={busy || isRunning(job)} onClick={() => index(null, true)}>Index and map with {plan.models.join(", ")}</button>}
    <div className="collection-actions">
      {c.reimportable && <button type="button" className="text-btn" disabled={busy || isRunning(job)} onClick={reimport}
        title={c.source_path ? `Imports ${c.source_path} again with the same columns and roles` : undefined}>Update from source</button>}
      {tables.some((t) => t.origin === "embedded") && <button type="button" className="text-btn" onClick={() => { const t = tables[0]; ctx.pickTable(t); ctx.go({ view: "project", tab: "search" }); }}>Search this collection</button>}
      <button type="button" className="text-btn push" aria-expanded={open} onClick={onToggle}>{open ? "Hide details" : "Details"}</button>
    </div>
    {reimported && <div className="dim small">Updated: {reimported}</div>}
    <ErrorLine error={error} />
    {open && <Details ctx={ctx} c={c} tables={tables} />}
  </section>;
}

/** Shown only when an index needs attention; a healthy index needs no row. */
function SpaceRow({ t, busy, indexingHere, onIndex }: { t: TableInfo; busy: boolean; indexingHere: boolean; onIndex: () => void }) {
  const status: IndexStatus = indexingHere && t.index_status !== "imported" ? "indexing" : t.index_status;
  const resumable = ["interrupted", "cancelled", "outdated", "failed", "unknown"].includes(status) && t.origin === "embedded";
  if (!resumable) return null;
  return <div className="attention">
    <span className="index-status"><StatusMark status={status} />{STATUS_LABEL[status]}</span>
    {status === "failed" && t.index_error && <span className="error small">{t.index_error}</span>}
    <button type="button" className="text-btn push" disabled={busy} onClick={onIndex}>{status === "failed" ? "Retry" : status === "outdated" ? "Index changes" : "Resume"}</button>
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

  const cal = (t: TableInfo, llm: boolean) => start(() => api.startCalibration({ collection: c.collection_id, space: t.space_id }, llm));
  return <div className="details stack gap">
    <p className="dim small">Source files are not watched. “Update from source” imports the same file or folder again and re-indexes only what changed.</p>
    {tables.map((t) => <div key={t.space_id} className="stack gap-s">
      <span className="section">{t.model_key} · {t.space_id.slice(0, 12)}</span>
      <div className="detail-line">
        <span className="eyebrow">Calibration</span>
        <span>{t.calibration ? `${t.calibration.method}, ${fmt(t.calibration.sample)} items${t.calibration.holdout_auc ? `, held-out AUC ${t.calibration.holdout_auc.toFixed(3)}` : ""}${t.calibration.vectors !== t.vectors ? " — the collection changed since" : ""}` : "None. Search shows similarity only, no verdict."}</span>
        <span className="row wrap">
          <button type="button" className="text-btn" disabled={running || busy || t.vectors === 0} onClick={() => cal(t, false)}>{t.calibration ? "Recalibrate" : "Calibrate"}</button>
          {llmReady && <button type="button" className="text-btn" disabled={running || busy || t.vectors === 0} onClick={() => cal(t, true)}>With language model</button>}
        </span>
      </div>
      <div className="detail-line">
        <span className="eyebrow">Regions</span>
        <span>{t.regions ? `${fmt(Number(t.regions.stats.fine ?? 0))} regions, ${when(t.regions.created_at)}${t.regions.scanned ? "; gaps scored" : ""}` : "None. The map and gaps need them."}</span>
        <button type="button" className="text-btn" disabled={running || busy || t.vectors === 0} onClick={() => start(() => api.startRegions({ collection: c.collection_id, space: t.space_id }))}>{t.regions ? "Find again" : "Find regions"}</button>
      </div>
      {isRunning(current) && current!.collection === c.collection_id && current!.space === t.space_id && <JobProgress job={current!} />}
    </div>)}
    {others.length > 0 && <div className="stack gap-s">
      <span className="section">Index with another model</span>
      <span className="dim small">Each model keeps its own index; nothing is combined or overwritten.</span>
      <div className="index-list">
        {others.map((m) => <div key={m.key} className="index-row">
          <div className="rank-title"><span className="mono small">{m.key}</span><span className="dim">{m.dimensions}-d · ≈{fmt(m.megabytes)} MB · {m.installed ? "installed" : "not installed"}</span></div>
          {m.installed ? <button type="button" className="text-btn" disabled={running || busy} onClick={() => start(() => api.startIndex(c.collection_id, [m.key]))}>Index</button>
            : <button type="button" className="text-btn" onClick={() => setConsent(m)}>Download…</button>}
        </div>)}
      </div>
      {consent && <DownloadConsent model={{ key: consent.key, repo: consent.repo, megabytes: consent.megabytes }} project={project} onClose={() => setConsent(undefined)} />}
    </div>}
    <ErrorLine error={error} />
  </div>;
}
