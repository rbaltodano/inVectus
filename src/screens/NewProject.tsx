// New project: name and place, where the first data comes from (a file, a folder or one of the
// integrations), and who does the project's writing work (an agent, or the on-device model).
import { useEffect, useState } from "react";
import { Folder } from "lucide-react";
import { AgentMark, LAST_AGENT, useAgentOptions } from "../agentOptions";
import { api, inTauri, local, pickDirectory, type JobInfo, type ProjectInfo } from "../api";
import type { Page } from "../App";
import SourcePicker, { type Picked } from "../SourcePicker";
import { baseName, joinPath } from "../paths";
import "./NewProject.css";

interface Props {
  onCreated: (p: ProjectInfo, first?: Page) => void;
  onIntegrations: () => void;
  onAgents: () => void;
  blocked?: JobInfo;
}

export default function NewProject({ onCreated, onIntegrations, onAgents, blocked }: Props) {
  const [name, setName] = useState("");
  const [dir, setDir] = useState("");
  const [sources, setSources] = useState<Picked[]>([]);
  const [agent, setAgent] = useState<string>();
  const [agentPicked, setAgentPicked] = useState(false);
  const [error, setError] = useState<string>();
  const [busy, setBusy] = useState(false);
  const { agents, fallback } = useAgentOptions();

  useEffect(() => {
    if (!inTauri) return;
    api.defaultDir().then(setDir).catch(() => undefined);
  }, []);

  // Default to the agent chosen last time (see useAgentOptions) until one is picked here.
  useEffect(() => { if (!agentPicked || !agents.some((a) => a.id === agent)) setAgent(fallback); }, [fallback, agents, agentPicked]);

  // Without a name, the project is named after the first file or folder chosen.
  const firstLocal = sources.find((p) => p.id === "file" || p.id === "folder");
  const stem = firstLocal ? baseName(firstLocal.path).replace(/\.[^.]+$/, "") : "";
  const projectName = name.trim() || stem;
  const canCreate = !busy && !blocked && !!dir.trim() && !!projectName;

  const create = async () => {
    if (!canCreate) return;
    setError(undefined); setBusy(true);
    try {
      const p = await api.open(joinPath(dir, projectName), true);
      const withAgent = agent ? await api.setProjectAgent(p.root, agent).catch(() => p) : p;
      if (agent) local.set(LAST_AGENT, agent);
      // Each chosen source goes through Import in turn.
      const [first, ...rest] = sources.map((p) => p.path);
      onCreated(withAgent, first ? { kind: "import", path: first, next: rest } : undefined);
    } catch (e) { setError(String(e)); } finally { setBusy(false); }
  };

  const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");
  return <div className="page new-project">
    <div className="page-head">
      <div className="np-head"><p className="eyebrow">Setup</p>
        {/* The heading is the name field: type straight away. */}
        <h1><input className="np-title" autoFocus aria-label="Project name" value={name} placeholder={stem || "New project"}
          onChange={(e) => setName(e.target.value)} onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); e.currentTarget.blur(); } }} /></h1>
        <p className="dim new-project-lede">A project is a folder on this computer: one dataset, its index and map, and your decisions.</p></div>
    </div>

    <section className="new-project-section" aria-labelledby="np-where">
      <h2 id="np-where"><span className="np-step">01</span>Project destination</h2>
      <div className="np-destination">
        <input aria-labelledby="np-where" value={dir} onChange={(e) => setDir(e.target.value)} spellCheck={false} />
        <button type="button" className="np-folder" aria-label="Choose a folder" title="Choose a folder"
          onClick={async () => { const d = await pickDirectory(dir).catch((e) => { setError(String(e)); return null; }); if (d) setDir(d); }}>
          <Folder size={16} strokeWidth={1.5} aria-hidden="true" /></button>
      </div>
      <span className="dim small">{projectName && dir.trim() ? `${tilde(joinPath(dir, projectName))}.invectus` : "Everything stays on this computer. Avoid iCloud, Dropbox and OneDrive folders."}</span>
    </section>

    <section className="new-project-section" aria-labelledby="np-data">
      <h2 id="np-data"><span className="np-step">02</span>Import existing data <span className="dim np-optional">optional</span></h2>
      <SourcePicker picked={sources} onChange={setSources} onIntegrations={onIntegrations} disabled={!!blocked} />
    </section>

    <section className="new-project-section" aria-labelledby="np-agent">
      <h2 id="np-agent"><span className="np-step">03</span>Agent for this project</h2>
      <p className="dim small">Who writes gap descriptions, region names and paraphrases. You can change it later.</p>
      <div className="np-agents" role="radiogroup" aria-labelledby="np-agent">
        {agents.map((a) => <button key={a.id} type="button" role="radio" aria-checked={agent === a.id} className="np-agent" onClick={() => { setAgent(a.id); setAgentPicked(true); }}>
          <span className="np-agent-mark"><AgentMark id={a.id} /></span>
          <span className="np-agent-text"><span className="np-agent-name">{a.name}</span><span className="np-agent-note">{a.note}</span></span>
        </button>)}
      </div>
      {agents.length === 1 && <p className="dim small">No agent apps are connected. <button type="button" className="text-btn" onClick={onAgents}>Connect one on Agents</button></p>}
    </section>

    <div className="new-project-actions">
      {error && <p className="err" role="alert">{error}</p>}
      {blocked && <p className="note">“{blocked.label}” is running; a new project can open when it finishes.</p>}
      <button type="button" className="btn primary" disabled={!canCreate} onClick={() => void create()}>
        {busy ? "Creating…" : sources.length ? `Create and import ${sources.length} source${sources.length === 1 ? "" : "s"}` : "Create project"}</button>
    </div>
  </div>;
}
