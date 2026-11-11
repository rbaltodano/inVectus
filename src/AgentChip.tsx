// The project header's agent pill: who does this project's writing work. Opens a list of the
// connected agents and the on-device model; "Add new agent" opens the Agents
// page's connect screen in a dialog.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Bot, Check, ChevronDown, Plus, X } from "lucide-react";
import { AgentMark, LAST_AGENT, useAgentOptions } from "./agentOptions";
import { api, local, type ProjectInfo } from "./api";
import { useFlickerDelays } from "./dot-flicker";
import ConnectAgents from "./screens/ConnectAgents";
import "./AgentChip.css";

export default function AgentChip({ project, setProject }: { project: ProjectInfo; setProject: (p: ProjectInfo) => void }) {
  const { agents, refresh } = useAgentOptions();
  const flicker = useFlickerDelays();
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [place, setPlace] = useState({ top: 0, right: 0 });
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const current = agents.find((a) => a.id === project.agent);

  useEffect(() => {
    if (!open) return;
    const r = trigger.current!.getBoundingClientRect();
    setPlace({ top: r.bottom + 8, right: Math.max(8, window.innerWidth - r.right) });
    const outside = (e: PointerEvent) => { if (!popup.current?.contains(e.target as Node) && !trigger.current?.contains(e.target as Node)) setOpen(false); };
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") { setOpen(false); trigger.current?.focus(); } };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", key);
    return () => { document.removeEventListener("pointerdown", outside); document.removeEventListener("keydown", key); };
  }, [open]);

  const choose = async (id: string) => {
    setBusy(true); setError(undefined);
    try { setProject(await api.setProjectAgent(project.root, id)); local.set(LAST_AGENT, id); setOpen(false); }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };

  return <>
    <button type="button" ref={trigger} className="agent-chip" aria-haspopup="dialog" aria-expanded={open} onClick={() => { refresh(); setOpen(!open); }}
      title="Agent for this project: who writes gap descriptions, region names and paraphrases">
      <span className="agent-chip-mark">{current ? <AgentMark id={current.id} size={14} /> : <Bot size={14} strokeWidth={1.5} aria-hidden="true" />}</span>
      <span>{current?.name ?? (project.agent ? project.agent : "Choose agent")}</span>
      <ChevronDown size={12} aria-hidden="true" className="agent-chip-chev" />
    </button>
    {open && createPortal(<div ref={popup} className="agent-menu" style={{ top: place.top, right: place.right }} role="dialog" aria-label="Agent for this project">
      <div className="agent-menu-head">Agent for this project</div>
      <div role="radiogroup" aria-label="Agents">
        {agents.map((a) => <button key={a.id} type="button" role="radio" aria-checked={a.id === project.agent} className="agent-menu-row flicker-in" style={flicker(a.id)} disabled={busy} onClick={() => void choose(a.id)}>
          <span className="agent-menu-mark"><AgentMark id={a.id} /></span>
          <span className="agent-menu-text"><span>{a.name}</span><span className="agent-menu-note">{a.note}</span></span>
          <span className="agent-menu-check">{a.id === project.agent && <Check size={14} aria-hidden="true" />}</span>
        </button>)}
      </div>
      <button type="button" className="agent-menu-row agent-menu-add" onClick={() => { setOpen(false); setAdding(true); }}>
        <span className="agent-menu-mark"><Plus size={16} strokeWidth={1.5} aria-hidden="true" /></span><span>Add new agent</span>
      </button>
      {error && <p className="err small agent-menu-error" role="alert">{error}</p>}
    </div>, document.body)}
    {adding && createPortal(<div className="integration-modal-backdrop" onClick={(e) => { if (e.target === e.currentTarget) { setAdding(false); refresh(); } }}
      onKeyDown={(e) => { if (e.key === "Escape") { setAdding(false); refresh(); } }}>
      <div className="integration-modal agent-add-modal" role="dialog" aria-modal="true" aria-labelledby="agent-add-title">
        <div className="agent-add-head">
          <div><h3 id="agent-add-title">Add an agent</h3>
            <p className="dim small">Connect an AI agent app once; it works in whichever project you have open here, on your own plan.</p></div>
          <button type="button" className="agent-add-close" aria-label="Close" onClick={() => { setAdding(false); refresh(); }}><X size={16} aria-hidden="true" /></button>
        </div>
        <ConnectAgents />
      </div>
    </div>, document.body)}
  </>;
}
