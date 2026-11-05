import { useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { ChevronDown } from "lucide-react";
import AgentIcon from "./AgentIcon";
import { agentIdentity } from "./agentIdentity";
import ModelSelector from "./ModelSelector";
import CursorHint from "./CursorHint";
import { api, local, type ProjectInfo } from "./api";

export default function AgentAttribution({ author, project, setProject }: {
  author: string; project: ProjectInfo; setProject: (project: ProjectInfo) => void;
}) {
  const { client, label } = agentIdentity(author);
  const lastModelKey = `invectus.last-agent-model.${project.root}`;
  const [rememberedModel, setRememberedModel] = useState(() => local.get<string | null>(lastModelKey, null));
  const candidate = project.preferred_agent_model ?? rememberedModel;
  const lastModel = typeof candidate === "string" && candidate.trim() ? candidate : null;
  const [open, setOpen] = useState(false);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string>();
  const [position, setPosition] = useState({ top: 0, left: 0, maxHeight: 400 });
  const trigger = useRef<HTMLButtonElement>(null);
  const popup = useRef<HTMLDivElement>(null);
  const id = useId();
  useEffect(() => {
    if (!open) return;
    const place = () => {
      const rect = trigger.current!.getBoundingClientRect();
      const below = window.innerHeight - rect.bottom - 16;
      const above = rect.top - 16;
      const height = Math.min(390, Math.max(below, above));
      setPosition({
        top: below >= height ? rect.bottom + 8 : Math.max(8, rect.top - height - 8),
        left: Math.max(8, Math.min(rect.right - 280, window.innerWidth - 288)),
        maxHeight: height,
      });
    };
    place();
    const frame = requestAnimationFrame(() => popup.current?.querySelector<HTMLButtonElement>('button[aria-pressed="true"], button')?.focus());
    const outside = (e: PointerEvent) => {
      if (!popup.current?.contains(e.target as Node) && !trigger.current?.contains(e.target as Node)) setOpen(false);
    };
    const key = (e: KeyboardEvent) => {
      if (e.key === "Escape") { e.preventDefault(); setOpen(false); trigger.current?.focus(); }
    };
    document.addEventListener("pointerdown", outside);
    document.addEventListener("keydown", key);
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => {
      cancelAnimationFrame(frame);
      document.removeEventListener("pointerdown", outside);
      document.removeEventListener("keydown", key);
      window.removeEventListener("resize", place);
      window.removeEventListener("scroll", place, true);
    };
  }, [open]);
  const choose = async (model: string | null, close = true) => {
    setBusy(true); setError(undefined);
    try {
      const updated = await api.setAgentModel(project.root, model);
      const remember = model ?? project.preferred_agent_model;
      if (remember) { local.set(lastModelKey, remember); setRememberedModel(remember); }
      setProject(updated);
      if (close) { setOpen(false); trigger.current?.focus(); }
    }
    catch (e) { setError(String(e)); }
    finally { setBusy(false); }
  };
  return <span className="agent-attribution-wrap push">
    <button type="button" ref={trigger} className="agent-attribution small" aria-expanded={open} aria-controls={open ? id : undefined}
      aria-label={`${label}; choose model for future work`} onClick={() => setOpen(!open)}>
      <AgentIcon client={client} /><span>{label}</span><ChevronDown size={12} aria-hidden="true" />
    </button>
    {project.preferred_agent_model && <span className="dim small agent-next-model">Next: {project.preferred_agent_model}</span>}
    {open && createPortal(<div ref={popup} id={id} className="model-popover" style={position} role="dialog" aria-label="Model for future work"
      onBlur={(e) => { if (e.relatedTarget && !e.currentTarget.contains(e.relatedTarget as Node) && e.relatedTarget !== trigger.current) setOpen(false); }}>
      <ModelSelector value={project.preferred_agent_model ?? ""} onChange={choose} disabled={busy} />
      <CursorHint className="model-selector-note model-current-row" text="For future work in this project. Your agent app controls model switching.">
        <span id={`${id}-current`}>Use agent’s current model</span>
        <button type="button" role="switch" className="model-current-switch" aria-labelledby={`${id}-current`} aria-describedby={`${id}-help`}
          aria-checked={!project.preferred_agent_model} disabled={busy || (!project.preferred_agent_model && !lastModel)}
          title={!project.preferred_agent_model && !lastModel ? "Choose a model above to set a preference" : undefined}
          onClick={() => choose(project.preferred_agent_model ? null : lastModel, false)}><span aria-hidden="true" /></button>
        <span id={`${id}-help`} className="sr-only">For future work in this project. Your agent app controls model switching.</span>
      </CursorHint>
      {error && <div role="alert" className="err small model-selector-note">{error}</div>}
    </div>, document.body)}
  </span>;
}
