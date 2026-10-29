import { useEffect, useState } from "react";
import { api, fmt, inTauri, local, when, type LlmInfo, type ModelInfo, type Weights } from "../api";
import { ErrorLine, useAction, useStartJob, type Ctx } from "../App";
import { isRunning } from "../jobs";
import { DownloadConsent } from "../models";
import { weightsKey } from "./Gaps";

const WEIGHT_LABEL: [keyof Weights, string][] = [
  ["coverage_ratio", "Demand per coverage item"], ["demand_since", "Demand since the last coverage"], ["momentum", "Momentum"],
  ["stale_share", "Share of demand newer than the coverage"], ["bridge", "Bridges: closeness with little in between"], ["saturation", "Recent coverage penalty"],
];

export default function Settings({ ctx }: { ctx?: Ctx }) {
  const [models, setModels] = useState<ModelInfo[]>([]);
  const [llms, setLlms] = useState<LlmInfo[]>([]);
  const [defaults, setDefaults] = useState<Weights>();
  const [weights, setWeights] = useState<Weights>();
  const [consent, setConsent] = useState<ModelInfo>();
  const jobs = useStartJob();
  const { busy, error, run } = useAction();
  const project = ctx?.project;
  const running = isRunning(jobs.current);

  useEffect(() => {
    if (!inTauri) return;
    api.models().then(setModels).catch(() => undefined);
    api.llms().then(setLlms).catch(() => undefined);
    api.defaultWeights().then((d) => { setDefaults(d); setWeights(project ? local.get(weightsKey(project.root), d) ?? d : d); }).catch(() => undefined);
  }, [project?.root, jobs.current?.state]);

  const setWeight = (k: keyof Weights, v: number) => {
    if (!weights || !project) return;
    const next = { ...weights, [k]: v };
    setWeights(next); local.set(weightsKey(project.root), next);
  };
  const reset = () => { if (defaults && project) { setWeights(defaults); local.set(weightsKey(project.root), null); } };
  const llmReady = llms.some((l) => l.installed);
  const changed = weights && defaults && JSON.stringify(weights) !== JSON.stringify(defaults);

  return <div className="page narrow">
    <h1>Settings</h1>
    <ErrorLine error={error ?? jobs.error} />

    <div className="label section-label">App · all projects</div>
    <section aria-labelledby="s-models" className="panel stack gap">
      <h2 id="s-models">Embedding models</h2>
      {models.map((m) => <div key={m.key} className="spread setting-row">
        <div><b className="mono">{m.key}</b>{m.default && <span className="muted"> · default</span>}<div className="muted small">≈{fmt(m.megabytes)} MB · {m.dimensions} dimensions · {m.installed ? "installed, checksums verified" : "not installed"}</div></div>
        {!m.installed && <button type="button" className="btn small" onClick={() => setConsent(m)}>Download…</button>}
      </div>)}
      {consent && <DownloadConsent model={{ key: consent.key, repo: consent.repo, megabytes: consent.megabytes }} project={project} onClose={() => setConsent(undefined)} />}
      <div className="muted small">Models are shared by all projects and loaded only from verified local files. The CLI's <code>invectus models</code> manages them too.</div>
    </section>

    <section aria-labelledby="s-llm" className="panel stack gap">
      <h2 id="s-llm">Local language model</h2>
      <div className="muted small">Optional. Writes gap descriptions, names regions and paraphrases queries for calibration, on this computer.</div>
      {llms.map((l) => <div key={l.key} className="spread setting-row">
        <div><b className="mono">{l.key}</b><div className="muted small">≈{fmt(l.megabytes)} MB · {l.installed ? "installed" : "not installed"} · {l.note}</div></div>
      </div>)}
      {!llmReady && <div className="muted small">Not installed: descriptions and language-model calibration are off. <code>invectus llm list</code> shows where the pinned file goes.</div>}
    </section>

    <div className="label section-label">Project · {project ? project.name : "none open"}</div>
    {!project && <p className="muted">Open a project to change its network switch, gap weights and calibration.</p>}

    {project && <section aria-labelledby="s-net" className="panel stack gap">
      <h2 id="s-net">Network</h2>
      <label className="check top">
        <input type="checkbox" checked={!project.network_enabled} disabled={busy}
          onChange={() => ctx && run(async () => ctx.setProject(await api.setNetwork(!project.network_enabled)))} />
        <span><b>Offline</b><div className="muted small">Blocks model downloads and outside-source checks for this project. Import, indexing, search, map, gaps and descriptions always run offline.</div></span>
      </label>
      {project.offline_env && <div className="note small">INVECTUS_OFFLINE is set: every network use is refused whatever this switch says.</div>}
      <div className="muted small">Outside sources for checking gaps: PubMed and OpenAlex. Results are cached in the project for 7 days.</div>
    </section>}

    {project && weights && <section aria-labelledby="s-w" className="panel stack gap">
      <h2 id="s-w">Gap scoring</h2>
      <div className="muted small">Weights on each standardised term; they apply the next time gaps are scanned. Stored on this computer for this project.</div>
      {WEIGHT_LABEL.map(([k, label]) => <label key={k} className="slider">
        <span>{label}</span>
        <input type="range" min={0} max={3} step={0.25} value={weights[k]} onChange={(e) => setWeight(k, Number(e.target.value))} />
        <b className="mono">{weights[k].toFixed(2)}</b>
      </label>)}
      <div className="row"><button type="button" className="btn small" disabled={!changed} onClick={reset}>Reset to defaults</button>
        <span className="muted small">Defaults were tuned on a 591k-paper benchmark (they still trail simple baselines there).</span></div>
    </section>}

    {project && <section aria-labelledby="s-cal" className="panel stack gap">
      <h2 id="s-cal">Verdict calibration</h2>
      {project.tables.length === 0 && <p className="muted">Nothing indexed yet.</p>}
      {project.tables.map((t) => <div key={t.space_id} className="spread setting-row">
        <div><b className="mono">{t.collection} · {t.model_key}</b>
          <div className="muted small">{t.calibration
            ? `${t.calibration.method} · ${fmt(t.calibration.sample)} items${t.calibration.holdout_auc ? ` · held-out AUC ${t.calibration.holdout_auc.toFixed(3)}` : ""} · ${when(t.calibration.fitted_at)}${t.calibration.vectors !== t.vectors ? " · the collection changed since" : ""}`
            : "Not calibrated: search shows similarity without a verdict"}</div></div>
        <div className="row">
          {llmReady && <button type="button" className="btn small" disabled={running || jobs.busy || t.vectors === 0} onClick={() => jobs.start(() => api.startCalibration({ collection: t.collection, space: t.space_id }, true))}>With language model</button>}
          <button type="button" className="btn small" disabled={running || jobs.busy || t.vectors === 0} onClick={() => jobs.start(() => api.startCalibration({ collection: t.collection, space: t.space_id }, false))}>{t.calibration ? "Recalibrate" : "Calibrate"}</button>
        </div>
      </div>)}
      {running && <div className="muted small">A job is running; calibration can start when it finishes.</div>}
    </section>}

    {project && project.tables.length > 0 && <details className="panel">
      <summary>Embedding details</summary>
      {project.tables.map((t) => <dl key={t.space_id} className="facts mono small">
        <dt>Collection</dt><dd>{t.collection}</dd>
        <dt>Space ID</dt><dd>{t.space_id}</dd>
        <dt>Model</dt><dd>{t.model_key} · {t.dimensions} dimensions</dd>
        <dt>Runtime</dt><dd>{t.runtime}</dd>
        <dt>Origin</dt><dd>{t.origin}</dd>
        <dt>Vectors</dt><dd>{fmt(t.vectors)}{t.ann_partitions ? ` · ANN ${t.ann_partitions} partitions` : ""}</dd>
      </dl>)}
    </details>}
    {project && <p className="muted small mono">Project folder: {project.root}</p>}
  </div>;
}
