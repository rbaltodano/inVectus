import { useEffect, useState } from "react";
import { api, fmt, inTauri, local, type LlmInfo, type ModelInfo, type Weights } from "../api";
import { ErrorLine, useAction, type Ctx } from "../App";
import { useJobs } from "../jobs";
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
  const jobs = useJobs();
  const { busy, error, run } = useAction();
  const project = ctx?.project;

  useEffect(() => {
    if (!inTauri) return;
    api.models().then(setModels).catch(() => undefined);
    api.llms().then(setLlms).catch(() => undefined);
    api.defaultWeights().then((d) => { setDefaults(d); setWeights(project ? local.get(weightsKey(project.root), d) : d); }).catch(() => undefined);
  }, [project?.root, jobs.current?.status]);

  const setWeight = (k: keyof Weights, v: number) => {
    if (!weights || !project) return;
    const next = { ...weights, [k]: v };
    setWeights(next); local.set(weightsKey(project.root), next);
  };
  const reset = () => { if (defaults && project) { setWeights(defaults); local.set(weightsKey(project.root), null); } };
  const toggleNetwork = () => ctx && run(async () => ctx.setProject(await api.setNetwork(!ctx.project.network_enabled)));
  const recalibrate = (collection: string, space: string, llm: boolean) => run(async () => {
    jobs.begin("calibrate", `Calibrating ${collection}`);
    try { await api.startCalibration({ collection, space }, llm); } catch (e) { jobs.clear(); throw e; }
  });
  const download = (m: ModelInfo) => run(async () => {
    jobs.begin("download", `Downloading ${m.key}`);
    try { await api.installModel(m.key); } catch (e) { jobs.clear(); throw e; }
  });
  const llmReady = llms.some((l) => l.installed);
  const changed = weights && defaults && JSON.stringify(weights) !== JSON.stringify(defaults);

  return <div className="page narrow">
    <h1>Settings</h1>
    <ErrorLine error={error} />

    <section aria-labelledby="s-models" className="card stack gap">
      <h2 id="s-models">Embedding models</h2>
      {models.map((m) => <div key={m.key} className="spread setting-row">
        <div><b>{m.key}</b>{m.default && <span className="muted"> · default</span>}<div className="muted small">≈{fmt(m.megabytes)} MB · {m.dimensions} dimensions · {m.installed ? "installed, checksum verified" : "not installed"}</div></div>
        {!m.installed && <button type="button" className="btn" disabled={busy || !project?.network_enabled || jobs.current?.status === "running"}
          title={!project?.network_enabled ? "Open a project and turn off offline mode to download" : undefined} onClick={() => download(m)}>Download ≈{fmt(m.megabytes)} MB</button>}
      </div>)}
      <div className="muted small">Models are shared by all projects. The CLI's <code>invectus models</code> manages them too.</div>
    </section>

    <section aria-labelledby="s-net" className="card stack gap">
      <h2 id="s-net">Network</h2>
      {project ? <label className="check top">
        <input type="checkbox" checked={!project.network_enabled} onChange={toggleNetwork} disabled={busy} />
        <span><b>Offline mode</b> <span className="muted small">· this project</span>
          <div className="muted small">Blocks model downloads and outside-source checks. Import, indexing, search, maps, gaps and descriptions always run offline.</div></span>
      </label> : <p className="muted">Open a project to change its network switch.</p>}
      {project?.offline_env && <div className="warn small">INVECTUS_OFFLINE is set: every network use is refused whatever this switch says.</div>}
      <div className="muted small">Outside sources used for checking gaps: PubMed and OpenAlex. Results are cached in the project for 7 days.</div>
    </section>

    <section aria-labelledby="s-llm" className="card stack gap">
      <h2 id="s-llm">Language model</h2>
      <div className="muted small">Used to describe gaps, name regions and calibrate verdicts. Optional, and it runs on this computer.</div>
      {llms.map((l) => <div key={l.key} className="spread setting-row">
        <div><b>{l.key}</b><div className="muted small">≈{fmt(l.megabytes)} MB · {l.installed ? "installed" : "not installed"} · {l.note}</div></div>
      </div>)}
      {!llmReady && <div className="muted small">Not installed: descriptions and language-model calibration are off. <code>invectus llm list</code> shows where the pinned file goes.</div>}
    </section>

    {project && weights && <section aria-labelledby="s-w" className="card stack gap">
      <h2 id="s-w">Gap scoring <span className="muted normal">· this project</span></h2>
      <div className="muted small">Weights on each standardised term. They apply the next time you score gaps.</div>
      {WEIGHT_LABEL.map(([k, label]) => <label key={k} className="slider">
        <span>{label}</span>
        <input type="range" min={0} max={3} step={0.25} value={weights[k]} onChange={(e) => setWeight(k, Number(e.target.value))} />
        <b>{weights[k].toFixed(2)}</b>
      </label>)}
      <div className="row"><button type="button" className="btn" disabled={!changed} onClick={reset}>Reset to defaults</button>
        <span className="muted small">Defaults were tuned on a 591k-paper benchmark.</span></div>
    </section>}

    {project && <section aria-labelledby="s-cal" className="card stack gap">
      <h2 id="s-cal">Verdict calibration</h2>
      {project.tables.length === 0 && <p className="muted">Nothing indexed yet.</p>}
      {project.tables.map((t) => <div key={t.space_id} className="spread setting-row">
        <div><b>{t.collection} · {t.model_key}</b>
          <div className="muted small">{t.calibration
            ? `${t.calibration.method} · ${fmt(t.calibration.sample)} items${t.calibration.holdout_auc ? ` · AUC ${t.calibration.holdout_auc.toFixed(3)}` : ""}${t.calibration.vectors !== t.vectors ? " · collection changed since" : ""}`
            : "Not calibrated"}</div></div>
        <div className="row">
          {llmReady && <button type="button" className="btn small" disabled={busy || jobs.current?.status === "running"} onClick={() => recalibrate(t.collection, t.space_id, true)}>With language model</button>}
          <button type="button" className="btn small" disabled={busy || jobs.current?.status === "running"} onClick={() => recalibrate(t.collection, t.space_id, false)}>{t.calibration ? "Recalibrate" : "Calibrate"}</button>
        </div>
      </div>)}
    </section>}

    {project && project.tables.length > 0 && <details className="card">
      <summary>Advanced: embedding details</summary>
      {project.tables.map((t) => <dl key={t.space_id} className="facts">
        <dt>Collection</dt><dd>{t.collection}</dd>
        <dt>Space ID</dt><dd><code>{t.space_id}</code></dd>
        <dt>Model</dt><dd>{t.model_key} · {t.dimensions} dimensions</dd>
        <dt>Runtime</dt><dd>{t.runtime}</dd>
        <dt>Origin</dt><dd>{t.origin}</dd>
        <dt>Vectors</dt><dd>{fmt(t.vectors)}{t.ann_partitions ? ` · ANN ${t.ann_partitions} partitions` : ""}</dd>
      </dl>)}
    </details>}
    {project && <p className="muted small">Project folder: <code>{project.root}</code></p>}
  </div>;
}
