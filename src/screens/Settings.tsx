import { useEffect, useRef, useState } from "react";
import { Database, Layers, Search, X } from "lucide-react";
import { SETTINGS_SECTIONS, searchSettings, settingsSection, settingsTabIndex, type SettingsSection } from "../settingsNavigation";
import { api, fmt, inTauri, local, when, type LlmInfo, type ModelInfo, type ProjectInfo, type Weights } from "../api";
import { ErrorLine, useAction, useStartJob } from "../App";
import { isRunning } from "../jobs";
import { DownloadConsent } from "../models";
import OutboundLog from "../OutboundLog";
import StorageSettings, { CheckCache, PulledCopies, RecoveryCode } from "./StorageSettings";
import TabIndicator, { useTabIndicator } from "../TabIndicator";
import { setThemeChoice, themeChoice, type ThemeChoice } from "../theme";
import { setFlickerSetting, useFlickerSetting } from "../dot-flicker";
import SettingsGroup from "../SettingsGroup";
import BoxCheck from "../BoxCheck";
import Switch from "../Switch";

const VERIFY_SOURCES: [string, string, string][] = [
  ["pubmed", "PubMed", "Biomedical literature (NCBI)."],
  ["openalex", "OpenAlex", "Scholarly works across all fields."],
  ["semanticscholar", "Semantic Scholar", "Papers across fields; shared free rate limit, so checks are slower."],
  ["arxiv", "arXiv", "Preprints in physics, computer science, maths and more. Has no review filter."],
];

/** Gap-score weights per project, kept on this computer; scans read them. */
export const weightsKey = (root: string) => `invectus.weights.${root}`;

const WEIGHT_LABEL: [keyof Weights, string][] = [
  ["coverage_ratio", "Demand per coverage item"], ["demand_since", "Demand since the last coverage"], ["momentum", "Momentum"],
  ["stale_share", "Share of demand newer than the coverage"], ["bridge", "Bridges: closeness with little in between"], ["saturation", "Recent coverage penalty"],
];

export default function Settings({ project, setProject }: { project?: ProjectInfo; setProject: (p: ProjectInfo) => void }) {
  const [section, setSection] = useState<SettingsSection>(() => settingsSection(local.get("invectus.settings.section", "appearance")));
  const { tabsRef, tabIndicator } = useTabIndicator(section, true);
  const [query, setQuery] = useState("");
  const content = useRef<HTMLDivElement>(null);
  const heading = useRef<HTMLHeadingElement>(null);
  const current = SETTINGS_SECTIONS.find((s) => s.id === section)!;
  const selectSection = (id: SettingsSection) => { setSection(id); local.set("invectus.settings.section", id); content.current?.scrollTo({ top: 0 }); };

  const [models, setModels] = useState<ModelInfo[]>([]);
  const [llms, setLlms] = useState<LlmInfo[]>([]);
  const matches = searchSettings(query, { models: [...models, ...llms].map((m) => m.key).join(" ") });
  const [defaults, setDefaults] = useState<Weights>();
  const [weights, setWeights] = useState<Weights>();
  const [consent, setConsent] = useState<ModelInfo>();
  const [theme, setTheme] = useState<ThemeChoice>(themeChoice);
  const chooseTheme = (t: ThemeChoice) => { setTheme(t); setThemeChoice(t); };
  const flicker = useFlickerSetting();
  const windowFlicker = useFlickerSetting("windows");
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
  const projectEmpty = !project ? <div className="settings-project-empty"><Layers size={24} aria-hidden="true" /><h3>No project open</h3><p>Open a project from the sidebar to change its external checks, gap weights and calibration, or inspect its embedding spaces.</p></div> : null;

  return <div className="page settings-page">
    <header className="settings-header">
      <div className="settings-title"><h1>Settings</h1><p>Appearance, models, storage and how each project checks its gaps.</p></div>
      <div className="settings-search-wrap">
        <div className="settings-search">
          <Search size={14} aria-hidden="true" />
          <input aria-label="Search settings" placeholder="Search settings" value={query} onChange={(e) => setQuery(e.target.value)}
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
{s.label}
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
            <SettingsGroup id="s-appearance" title="Theme" description="Pick a look, or follow your system setting.">
              <div role="group" aria-labelledby="s-appearance" className="settings-themes">
                {(["light", "dark", "system"] as const).map((t) =>
                  <button key={t} type="button" className="settings-theme" aria-pressed={theme === t} onClick={() => chooseTheme(t)}>
                    <span className={`settings-theme-preview settings-theme-${t}`} aria-hidden="true"><span className="theme-mini-sidebar"><i /><i /><i /></span><span className="theme-mini-content"><i /><i /><i /></span></span>
                    <span className="settings-theme-label">{t === "system" ? "System" : t === "light" ? "Light" : "Dark"}</span>
                  </button>)}
              </div>
            </SettingsGroup>
            <SettingsGroup id="s-motion" title="Motion" description="How cards, menus and windows come and go. Always off when your system asks for reduced motion.">
              <label className="check top settings-switch-row">
                <Switch checked={flicker} onChange={(on) => setFlickerSetting(on)} />
                <span><b>Flicker animations</b><span className="settings-hint">Cards, menus and the map’s squares flicker in and out. Off: they appear and go at once.</span></span>
              </label>
              <label className="check top settings-switch-row">
                <Switch checked={windowFlicker} onChange={(on) => setFlickerSetting(on, "windows")} />
                <span><b>Window flicker</b><span className="settings-hint">Popup windows flicker out when they close, even with flicker animations off.</span></span>
              </label>
            </SettingsGroup>
          </div>
          <div id="settings-section-models" role="tabpanel" aria-labelledby="settings-tab-models" className="settings-section" hidden={section !== "models"}>
            <SettingsGroup id="s-models" title="Embedding models" description={<>Shared by all projects and loaded only from verified local files. The CLI’s <code>invectus models</code> manages them too.</>}>
              {models.length === 0 && <p className="muted small">{inTauri ? "Loading embedding models…" : "Open the desktop app to see installed models and available downloads."}</p>}
              {models.map((m) => <div key={m.key} className="spread setting-row">
                <div><b className="mono">{m.key}</b>{m.default && <span className="muted"> · default</span>}<div className="muted small">≈{fmt(m.megabytes)} MB · {m.dimensions} dimensions · {m.installed ? "installed, checksums verified" : "not installed"}</div></div>
                {!m.installed && <button type="button" className="btn small" onClick={() => setConsent(m)}>Download…</button>}
              </div>)}
              {consent && <DownloadConsent model={{ key: consent.key, repo: consent.repo, megabytes: consent.megabytes }} project={project} onClose={() => setConsent(undefined)} />}
            </SettingsGroup>

            <SettingsGroup id="s-llm" title="Local language model" description="Optional. Writes gap descriptions, names regions and paraphrases queries for calibration, on this computer.">
              {llms.length === 0 && <p className="muted small">{inTauri ? "Loading language models…" : "Open the desktop app to see the local language model."}</p>}
              {llms.map((l) => <div key={l.key} className="spread setting-row">
                <div><b className="mono">{l.key}</b><div className="muted small">≈{fmt(l.megabytes)} MB · {l.installed ? "installed" : "not installed"} · {l.note}</div></div>
              </div>)}
              {!llmReady && <div className="muted small">Not installed: descriptions and language-model calibration are off. <code>invectus llm list</code> shows where the pinned file goes.</div>}
            </SettingsGroup>

          </div>
          <div id="settings-section-storage" role="tabpanel" aria-labelledby="settings-tab-storage" className="settings-section" hidden={section !== "storage"}>
            {section === "storage" && <StorageSettings />}
          </div>
          <div id="settings-section-network" role="tabpanel" aria-labelledby="settings-tab-network" className="settings-section" hidden={section !== "network"}>
            {projectEmpty}

            {project && <SettingsGroup id="s-net" title="Access" description="What may leave this computer for this project.">
              <label className="check top settings-switch-row">
                <Switch checked={project.network_enabled} disabled={busy}
                  onChange={() => run(async () => setProject(await api.setNetwork(!project.network_enabled)))} />
                <span><b>External checks</b><span className="settings-hint">Checking a gap sends its description to the outside sources below. Off until you turn it on. Pulling from your integrations and downloading models don’t need it; import, indexing, search, map, gaps and descriptions always run on this computer.</span></span>
              </label>
              <label className="check top settings-switch-row">
                <Switch checked={project.agent_access} disabled={busy}
                  onChange={() => run(async () => setProject(await api.setAgentAccess(!project.agent_access)))} />
                <span><b>Agent access</b><span className="settings-hint">Lets AI agents (Claude Code, Codex and others you connect) read this project’s items, gaps and files through inVectus. What they read can reach their provider. New projects start with your last choice.</span></span>
              </label>
              {project.offline_env && <div className="note small">INVECTUS_OFFLINE is set: every network use is refused whatever this switch says.</div>}
            </SettingsGroup>}
            {project && <SettingsGroup id="s-sources" title="Outside sources" description="Where external checks look when you check a gap. Keep at least one.">
              <div role="group" aria-labelledby="s-sources" className="settings-sources">
                {VERIFY_SOURCES.map(([id, label, note]) => {
                  const on = project.verify_sources.includes(id);
                  const last = on && project.verify_sources.length === 1;
                  return <label key={id} className="check top">
                    <BoxCheck checked={on} disabled={busy || last} title={last ? "Keep at least one source" : undefined}
                      onChange={() => run(async () => setProject(await api.setVerifySources(on ? project.verify_sources.filter((s) => s !== id) : [...project.verify_sources, id])))} />
                    <span><b>{label}</b><span className="settings-hint">{note}</span></span>
                  </label>;
                })}
              </div>
            </SettingsGroup>}
            {project && <SettingsGroup id="s-cache" title="Saved answers" description="Kept in the project for 7 days, so checking a gap again needs no network. Clear them to ask the sources again.">
              <CheckCache key={project.root} />
            </SettingsGroup>}
            {project && <SettingsGroup id="s-pulled" title="Pulled copies" description="What you pulled from your integrations, kept in the project so collections can be updated from it. It stays until you remove it here.">
              <PulledCopies key={project.root} />
            </SettingsGroup>}
            {project && <SettingsGroup id="s-key" title="Recovery code" description="This project’s key, for when encryption is on. Keep the code in a password manager or on paper: it opens the project on another computer, or if the keychain entry is lost. Anyone with it can read the project.">
              <RecoveryCode key={project.root} />
            </SettingsGroup>}
            <OutboundLog />

          </div>
          <div id="settings-section-scoring" role="tabpanel" aria-labelledby="settings-tab-scoring" className="settings-section" hidden={section !== "scoring"}>
            {projectEmpty}
            {project && !weights && <p className="muted small" role="status">Loading gap scoring settings…</p>}
            {project && weights && <SettingsGroup id="s-w" title="Weights" description="Weights on each standardised term; they apply the next time gaps are scanned. Stored on this computer for this project.">
              {WEIGHT_LABEL.map(([k, label]) => <label key={k} className="slider">
                <span>{label}</span>
                <input type="range" min={0} max={3} step={0.25} value={weights[k]} onChange={(e) => setWeight(k, Number(e.target.value))} />
                <b className="mono">{weights[k].toFixed(2)}</b>
              </label>)}
              <div className="row"><button type="button" className="btn small" disabled={!changed} onClick={reset}>Reset to defaults</button>
                <span className="muted small">Defaults were tuned on a 591k-paper benchmark (they still trail simple baselines there).</span></div>
            </SettingsGroup>}

          </div>
          <div id="settings-section-calibration" role="tabpanel" aria-labelledby="settings-tab-calibration" className="settings-section" hidden={section !== "calibration"}>
            {projectEmpty}
            {project && <SettingsGroup id="s-cal" title="Verdict calibration" description="Fitted per collection and embedding space. Without it, search shows similarity but no verdict.">
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
            </SettingsGroup>}

          </div>
          <div id="settings-section-embedding" role="tabpanel" aria-labelledby="settings-tab-embedding" className="settings-section" hidden={section !== "embedding"}>
            {projectEmpty}
            {project && project.tables.length === 0 && <div className="settings-project-empty"><Database size={24} aria-hidden="true" /><h3>No embedding spaces yet</h3><p>Index a collection to see its model, runtime and vector details here.</p></div>}
            {project && project.tables.length > 0 && <SettingsGroup id="s-spaces" title="Embedding spaces" description="Model, runtime and vectors for each indexed collection.">

              {project.tables.map((t) => <dl key={t.space_id} className="facts mono small">
                <dt>Collection</dt><dd>{t.collection}</dd>
                <dt>Space ID</dt><dd>{t.space_id}</dd>
                <dt>Model</dt><dd>{t.model_key} · {t.dimensions} dimensions</dd>
                <dt>Runtime</dt><dd>{t.runtime}</dd>
                <dt>Origin</dt><dd>{t.origin}</dd>
                <dt>Vectors</dt><dd>{fmt(t.vectors)}{t.ann_partitions ? ` · ANN ${t.ann_partitions} partitions` : ""}</dd>
              </dl>)}
            </SettingsGroup>}
          </div>
          {current.scope === "project" && project && <p className="settings-project-path"><span>Project folder</span><code>{project.root}</code></p>}
        </div>
      </div>
    </div>
  </div>;
}
