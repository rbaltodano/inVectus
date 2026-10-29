import { useEffect, useState } from "react";
import { api, fmt, type LlmInfo, type ModelInfo, type TableInfo } from "../api";
import { ErrorLine, useAction, type Ctx } from "../App";
import { JobProgress, useJobs } from "../jobs";

export default function Index({ ctx, initial }: { ctx: Ctx; initial?: string }) {
  const { project } = ctx;
  const [collection, setCollection] = useState(initial ?? ctx.collection ?? project.collections[0]?.collection_id ?? "");
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [llms, setLlms] = useState<LlmInfo[]>([]);
  const [model, setModel] = useState("");
  const [asking, setAsking] = useState<ModelInfo>();
  const [useLlm, setUseLlm] = useState(false);
  const jobs = useJobs();
  const { busy, error, run } = useAction();

  const loadModels = () => api.models().then((m) => {
    setModels(m);
    setModel((cur) => cur || (m.find((x) => x.default) ?? m[0])?.key || "");
  });
  useEffect(() => { loadModels().catch(() => undefined); api.llms().then(setLlms).catch(() => undefined); }, []);
  useEffect(() => { if (jobs.current?.kind === "download" && jobs.current.status !== "running") loadModels(); }, [jobs.current?.status]);

  const coll = project.collections.find((c) => c.collection_id === collection);
  const tables = project.tables.filter((t) => t.collection === collection);
  const spec = models.find((m) => m.key === model);
  const table: TableInfo | undefined = tables.find((t) => t.model_key === model);
  const tref = table && { collection, space: table.space_id };
  const job = jobs.current;
  const running = job?.status === "running";
  const llmReady = llms.some((l) => l.installed);

  if (!coll) return <div className="page narrow"><h1>Index</h1>
    <section className="card empty"><h2>Nothing to index yet</h2><p className="muted">Import a collection first.</p>
      <button type="button" className="btn primary" onClick={() => ctx.go({ name: "import" })}>Import data</button></section></div>;

  const pick = (m: ModelInfo) => { setModel(m.key); if (!m.installed) setAsking(m); };
  // Show the job at once; drop it again if the backend refuses to start.
  const start = (kind: Parameters<typeof jobs.begin>[0], label: string, f: () => Promise<void>) =>
    run(async () => { jobs.begin(kind, label); try { await f(); } catch (e) { jobs.clear(); throw e; } });
  const embed = () => start("embed", `Embedding ${collection}`, () => api.startEmbed(collection, model));
  const calibrate = () => tref && start("calibrate", `Calibrating ${collection}`, () => api.startCalibration(tref, useLlm));
  const regions = () => tref && start("regions", `Finding regions in ${collection}`, () => api.startRegions(tref));
  const scan = () => tref && run(async () => { await api.scan(tref, null); await ctx.refresh(); ctx.go({ name: "gaps" }); });
  const download = (m: ModelInfo) => start("download", `Downloading ${m.key}`, async () => { await api.installModel(m.key); setAsking(undefined); });

  const stepState = (kind: string, done: boolean, waiting: string) =>
    job?.kind === kind && running ? <JobProgress job={job} />
      : job?.kind === kind && job.status === "failed" ? <span className="error small">{job.message}</span>
        : <span className="muted small">{done ? "done" : waiting}</span>;
  const embedReport = job?.kind === "embed" && job.status === "finished" ? job.report : undefined;
  const regionStats = table?.regions?.stats as { fine?: number; stable?: number } | undefined;

  return <div className="page">
    <div className="title-row">
      <h1>Index “{collection}”</h1>
      <span className="muted">{fmt(coll.items)} items · {fmt(coll.items + coll.chunks)} texts with chunks</span>
      {project.collections.length > 1 && <select aria-label="Collection" className="push" value={collection} onChange={(e) => setCollection(e.target.value)}>
        {project.collections.map((c) => <option key={c.collection_id}>{c.collection_id}</option>)}</select>}
    </div>
    <ErrorLine error={error} />

    <section aria-labelledby="mdl" className="card stack gap">
      <h2 id="mdl">Embedding model</h2>
      <div role="radiogroup" aria-labelledby="mdl" className="stack gap-s">
        {models.map((m) => {
          const t = tables.find((x) => x.model_key === m.key);
          return <label key={m.key} className={`choice ${model === m.key ? "on" : ""}`}>
            <input type="radio" name="model" checked={model === m.key} onChange={() => pick(m)} />
            <span className="stack">
              <span className="strong">{m.key}{m.default && <span className="muted normal"> · default</span>}</span>
              <span className="muted small">{m.note}</span>
              <span className="muted small">{m.dimensions} dimensions · ≈{fmt(m.megabytes)} MB</span>
              <span className={`small ${m.installed ? "ok-text" : "warn-text"}`}>
                {m.installed ? "Installed" : "Not installed · download needed"}{t ? ` · ${fmt(t.vectors)} vectors in this collection` : ""}
              </span>
            </span>
          </label>;
        })}
      </div>
      <div className="muted small">Each model keeps its own index. Switching later adds a second one; nothing is overwritten.</div>
    </section>

    {asking && <section role="dialog" aria-labelledby="dl" aria-modal="false" className="card stack gap dialog">
      <h2 id="dl">Download {asking.key}?</h2>
      <dl className="facts"><dt>Size</dt><dd>≈{fmt(asking.megabytes)} MB</dd><dt>From</dt><dd>huggingface.co/{asking.repo}</dd><dt>Version</dt><dd>pinned, checksum verified</dd></dl>
      <div className="muted small">This is the only time inVectus goes online for a model. After this, indexing and search run offline.</div>
      {!project.network_enabled && <div className="warn small">Offline mode is on for this project. Turn it off at the top right to download.</div>}
      <div className="row end">
        <button type="button" className="btn" onClick={() => setAsking(undefined)}>Not now</button>
        <button type="button" className="btn primary" disabled={!project.network_enabled || busy || running} onClick={() => download(asking)}>Download</button>
      </div>
    </section>}

    <section aria-labelledby="job" className="card stack gap">
      <div className="title-row"><h2 id="job">Indexing with {model}</h2>
        {running && <span className="muted small">{job!.label} · running</span>}</div>
      <ol className="pipeline">
        <li>
          <div className="spread"><span className="strong">1 · Embed texts</span>
            <button type="button" className="btn small" disabled={!spec?.installed || running || busy} onClick={embed}>{table ? "Update" : "Embed"}</button></div>
          {stepState("embed", !!table && !(job?.kind === "embed" && running), spec?.installed ? "not started" : "install the model first")}
          {embedReport && <div className="muted small">{embedReport.cancelled ? "Cancelled; finished batches were kept. " : ""}{fmt(embedReport.embedded)} embedded ({embedReport.items_per_second.toFixed(0)}/s), {fmt(embedReport.already_embedded)} already done{embedReport.truncated ? `, ${fmt(embedReport.truncated)} long texts cut at the model's limit` : ""}.</div>}
          {table?.ann_partitions && <div className="muted small">Approximate search index: {table.ann_partitions} partitions.</div>}
        </li>
        <li>
          <div className="spread"><span className="strong">2 · Calibrate verdicts</span>
            <span className="row">
              {llmReady && <label className="check small"><input type="checkbox" checked={useLlm} onChange={(e) => setUseLlm(e.target.checked)} /> Use the language model</label>}
              <button type="button" className="btn small" disabled={!table || running || busy} onClick={calibrate}>{table?.calibration ? "Recalibrate" : "Calibrate"}</button>
            </span></div>
          {stepState("calibrate", !!table?.calibration, table ? "needed for “does this exist?” verdicts" : "after embedding")}
          {table?.calibration && <div className="muted small">{table.calibration.method} · {fmt(table.calibration.sample)} items{table.calibration.holdout_auc ? ` · held-out AUC ${table.calibration.holdout_auc.toFixed(3)}` : ""}{table.calibration.vectors !== table.vectors ? " · the collection changed since: recalibrate" : ""}</div>}
        </li>
        <li>
          <div className="spread"><span className="strong">3 · Find regions and lay out the map</span>
            <button type="button" className="btn small" disabled={!table || running || busy} onClick={regions}>{table?.regions ? "Run again" : "Find regions"}</button></div>
          {stepState("regions", !!table?.regions, table ? "not started" : "after embedding")}
          {regionStats?.fine !== undefined && <div className="muted small">{fmt(regionStats.fine)} regions{regionStats.stable !== undefined ? `, ${fmt(regionStats.stable)} stable` : ""}.</div>}
        </li>
        <li>
          <div className="spread"><span className="strong">4 · Score gaps</span>
            <button type="button" className="btn small" disabled={!table?.regions || running || busy} onClick={scan}>{busy ? "Scoring…" : table?.regions?.scanned ? "Score again" : "Score gaps"}</button></div>
          <span className="muted small">{table?.regions?.scanned ? "done" : table?.regions ? "not started" : "after regions"}</span>
        </li>
      </ol>
      <div className="muted small">Safe to quit. Embedding saves progress as it goes and picks up where it stopped.</div>
      <div className="row end">
        {table && <button type="button" className="btn" onClick={() => ctx.go({ name: "search" })}>Search what’s done so far</button>}
      </div>
    </section>
  </div>;
}
