import { useEffect, useRef, useState } from "react";
import { Check, Database, Layers, Monitor, Moon, Search, SlidersHorizontal, Sun, X } from "lucide-react";
import { SETTINGS_SECTIONS, searchSettings, settingsTabIndex, type SettingsSection } from "../settingsNavigation";
import { api, fmt, inTauri, local, when, type LlmInfo, type ModelInfo, type ProjectInfo, type Weights } from "../api";
import { ErrorLine, useAction, useStartJob } from "../App";
import { isRunning } from "../jobs";
import { DownloadConsent } from "../models";
import ConnectAgents from "./ConnectAgents";
import TabIndicator, { useTabIndicator } from "../TabIndicator";
import { setThemeChoice, themeChoice, type ThemeChoice } from "../theme";

/** Gap-score weights per project, kept on this computer; scans read them. */
export const weightsKey = (root: string) => `invectus.weights.${root}`;

const WEIGHT_LABEL: [keyof Weights, string][] = [
  ["coverage_ratio", "Demand per coverage item"], ["demand_since", "Demand since the last coverage"], ["momentum", "Momentum"],
  ["stale_share", "Share of demand newer than the coverage"], ["bridge", "Bridges: closeness with little in between"], ["saturation", "Recent coverage penalty"],
];

export default function Settings({ project, setProject }: { project?: ProjectInfo; setProject: (p: ProjectInfo) => void }) {
  const [section, setSection] = useState<SettingsSection>("appearance");
  const { tabsRef, tabIndicator } = useTabIndicator(section, true);
  const [query, setQuery] = useState("");
  const content = useRef<HTMLDivElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const current = SETTINGS_SECTIONS.find((s) => s.id === section)!;
  const selectSection = (id: SettingsSection) => { setSection(id); content.current?.scrollTo({ top: 0 }); };

  const [models, setModels] = useState<ModelInfo[]>([]);
  const [llms, setLlms] = useState<LlmInfo[]>([]);
  const matches = searchSettings(query, { models: [...models, ...llms].map((m) => m.key).join(" ") });
  const [defaults, setDefaults] = useState<Weights>();
  const [weights, setWeights] = useState<Weights>();
  const [consent, setConsent] = useState<ModelInfo>();
  const [theme, setTheme] = useState<ThemeChoice>(themeChoice);
  const chooseTheme = (t: ThemeChoice) => { setTheme(t); setThemeChoice(t); };
  const jobs = useStartJob();
  const { busy, error, run } = useAction();
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
  const projectEmpty = !project ? <div className="settings-project-empty"><Layers size={24} aria-hidden="true" /><h3>No project open</h3><p>Open a project from the sidebar to change its network switch, gap weights and calibration, or inspect its embedding spaces.</p></div> : null;

  return <div className="page settings-page">
    <header className="settings-header">
      <div className="settings-title"><SlidersHorizontal size={19} aria-hidden="true" /><h1>Settings</h1></div>
      <div className="settings-search-wrap">
        <div className="settings-search">
          <Search size={14} aria-hidden="true" />
          <input aria-label="Search settings" placeholder="Search settings…" value={query} onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Escape") { e.preventDefault(); setQuery(""); }
              if (e.key === "Enter" && matches[0]) { e.preventDefault(); selectSection(matches[0].id); setQuery(""); setTimeout(() => heading.current?.focus(), 0); }
            }} />
          {query && <button type="button" aria-label="Clear settings search" onClick={() => setQuery("")}><X size={13} aria-hidden="true" /></button>}
        </div>
        {query.trim() && <div className="settings-search-results" aria-label="Matching settings">
          {matches.map((s) => <button type="button" key={s.id} onClick={() => { selectSection(s.id); setQuery(""); setTimeout(() => heading.current?.focus(), 0); }}>
            <span>{s.label}</span><span>{s.scope === "app" ? "Application" : "Project"}</span>
          </button>)}
          {matches.length === 0 && <p role="status">No matching settings.</p>}
        </div>}
      </div>
    </header>
    <div ref={tabsRef} role="tablist" aria-label="Settings categories" className="project-tabs settings-tabs">
      {SETTINGS_SECTIONS.map((s, index) => {
        return <button type="button" role="tab" id={`settings-tab-${s.id}`} key={s.id} aria-label={s.label} title={s.label}
          aria-selected={section === s.id} tabIndex={section === s.id ? 0 : -1}
          aria-controls={`settings-section-${s.id}`} onClick={() => selectSection(s.id)}
          onKeyDown={(e) => {
            const next = settingsTabIndex(e.key, index);
            if (next === undefined) return;
            e.preventDefault(); selectSection(SETTINGS_SECTIONS[next].id);
            const tab = e.currentTarget.parentElement?.querySelectorAll<HTMLButtonElement>("[role=tab]")[next];
            tab?.focus(); tab?.scrollIntoView({ block: "nearest", inline: "nearest" });
          }}>
          <span className="tab-code">{String(index + 1).padStart(2, "0")}</span>{s.label}
        </button>;
      })}
      <TabIndicator geometry={tabIndicator} />
    </div>
    <div className="settings-main">
      <div className="settings-content" ref={content}>
        <div className="settings-content-inner">
          <div className="settings-page-heading"><div className="settings-heading-row"><h2 ref={heading} tabIndex={-1}>{current.label}</h2><span className="settings-scope">{current.scope === "app" ? "All projects" : project?.name ?? "No project open"}</span></div><p>{current.description}</p></div>
          <ErrorLine error={error ?? jobs.error} />
          <div id="settings-section-appearance" role="tabpanel" aria-labelledby="settings-tab-appearance" className="settings-section" hidden={section !== "appearance"}>
            <section aria-labelledby="s-appearance" className="panel settings-group stack gap">
              <div className="settings-group-heading"><h3 id="s-appearance">Theme</h3><p>Choose a look, or follow your system appearance.</p></div>
              <div role="group" aria-labelledby="s-appearance" className="settings-themes">
                {(["system", "light", "dark"] as const).map((t) => {
                  const Icon = t === "system" ? Monitor : t === "light" ? Sun : Moon;
                  return <button key={t} type="button" className="settings-theme" aria-pressed={theme === t} onClick={() => chooseTheme(t)}>
                    <span className={`settings-theme-preview settings-theme-${t}`} aria-hidden="true"><span className="theme-mini-sidebar"><i /><i /><i /></span><span className="theme-mini-content"><i /><i /><i /></span></span>
                    <span className="settings-theme-label"><Icon size={14} aria-hidden="true" />{t === "system" ? "Match system" : t === "light" ? "Light" : "Dark"}{theme === t && <Check size={14} className="settings-theme-check" aria-hidden="true" />}</span>
                  </button>;
                })}
              </div>
            </section>
          </div>
          <div id="settings-section-models" role="tabpanel" aria-labelledby="settings-tab-models" className="settings-section" hidden={section !== "models"}>
            <section aria-labelledby="s-models" className="panel settings-group stack gap">
              <h3 id="s-models">Embedding models</h3>
              {models.length === 0 && <p className="muted small">{inTauri ? "Loading embedding models…" : "Open the desktop app to see installed models and available downloads."}</p>}
              {models.map((m) => <div key={m.key} className="spread setting-row">
                <div><b className="mono">{m.key}</b>{m.default && <span className="muted"> · default</span>}<div className="muted small">≈{fmt(m.megabytes)} MB · {m.dimensions} dimensions · {m.installed ? "installed, checksums verified" : "not installed"}</div></div>
                {!m.installed && <button type="button" className="btn small" onClick={() => setConsent(m)}>Download…</button>}
              </div>)}
              {consent && <DownloadConsent model={{ key: consent.key, repo: consent.repo, megabytes: consent.megabytes }} project={project} onClose={() => setConsent(undefined)} />}
              <div className="muted small">Models are shared by all projects and loaded only from verified local files. The CLI's <code>invectus models</code> manages them too.</div>
            </section>

            <section aria-labelledby="s-llm" className="panel settings-group stack gap">
              <h3 id="s-llm">Local language model</h3>
              <div className="muted small">Optional. Writes gap descriptions, names regions and paraphrases queries for calibration, on this computer.</div>
              {llms.map((l) => <div key={l.key} className="spread setting-row">
                <div><b className="mono">{l.key}</b><div className="muted small">≈{fmt(l.megabytes)} MB · {l.installed ? "installed" : "not installed"} · {l.note}</div></div>
              </div>)}
              {!llmReady && <div className="muted small">Not installed: descriptions and language-model calibration are off. <code>invectus llm list</code> shows where the pinned file goes.</div>}
            </section>

          </div>
          <div id="settings-section-agents" role="tabpanel" aria-labelledby="settings-tab-agents" className="settings-section" hidden={section !== "agents"}><ConnectAgents /></div>
          <div id="settings-section-network" role="tabpanel" aria-labelledby="settings-tab-network" className="settings-section" hidden={section !== "network"}>
            {projectEmpty}

            {project && <section aria-labelledby="s-net" className="panel settings-group stack gap">
              <h3 id="s-net">Network</h3>
              <label className="check top settings-switch-row">
                <input type="checkbox" role="switch" checked={!project.network_enabled} disabled={busy}
                  onChange={() => run(async () => setProject(await api.setNetwork(!project.network_enabled)))} />
                <span><b>Offline</b><div className="muted small">Blocks model downloads and outside-source checks for this project. Import, indexing, search, map, gaps and descriptions always run offline.</div></span>
              </label>
              {project.offline_env && <div className="note small">INVECTUS_OFFLINE is set: every network use is refused whatever this switch says.</div>}
              <div className="muted small">Outside sources for checking gaps: PubMed and OpenAlex. Results are cached in the project for 7 days.</div>
            </section>}

          </div>
          <div id="settings-section-scoring" role="tabpanel" aria-labelledby="settings-tab-scoring" className="settings-section" hidden={section !== "scoring"}>
            {projectEmpty}
            {project && !weights && <p className="muted small" role="status">Loading gap scoring settings…</p>}
            {project && weights && <section aria-labelledby="s-w" className="panel settings-group stack gap">
              <h3 id="s-w">Gap scoring</h3>
              <div className="muted small">Weights on each standardised term; they apply the next time gaps are scanned. Stored on this computer for this project.</div>
              {WEIGHT_LABEL.map(([k, label]) => <label key={k} className="slider">
                <span>{label}</span>
                <input type="range" min={0} max={3} step={0.25} value={weights[k]} onChange={(e) => setWeight(k, Number(e.target.value))} />
                <b className="mono">{weights[k].toFixed(2)}</b>
              </label>)}
              <div className="row"><button type="button" className="btn small" disabled={!changed} onClick={reset}>Reset to defaults</button>
                <span className="muted small">Defaults were tuned on a 591k-paper benchmark (they still trail simple baselines there).</span></div>
            </section>}

          </div>
          <div id="settings-section-calibration" role="tabpanel" aria-labelledby="settings-tab-calibration" className="settings-section" hidden={section !== "calibration"}>
            {projectEmpty}
            {project && <section aria-labelledby="s-cal" className="panel settings-group stack gap">
              <h3 id="s-cal">Verdict calibration</h3>
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

          </div>
          <div id="settings-section-embedding" role="tabpanel" aria-labelledby="settings-tab-embedding" className="settings-section" hidden={section !== "embedding"}>
            {projectEmpty}
            {project && project.tables.length === 0 && <div className="settings-project-empty"><Database size={24} aria-hidden="true" /><h3>No embedding spaces yet</h3><p>Index a collection to see its model, runtime and vector details here.</p></div>}
            {project && project.tables.length > 0 && <details open className="panel settings-group">
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
          </div>
          {current.scope === "project" && project && <p className="settings-project-path"><span>Project folder</span><code>{project.root}</code></p>}
        </div>
      </div>
    </div>
  </div>;
}
