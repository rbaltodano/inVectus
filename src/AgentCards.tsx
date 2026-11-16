// New project, step 03: who does the project's writing work. The three agents used most
// recently are cards; "More" opens every connected agent and the on-device model in a dialog.
// Agent access (whether agents may read the project) is the switch under the cards.
import { useEffect, useRef, useState } from "react";
import { createPortal } from "react-dom";
import { Check, MoreHorizontal } from "lucide-react";
import { AgentMark, type AgentOption } from "./agentOptions";
import { local } from "./api";
import { useFlickerDelays } from "./dot-flicker";
import { FlickerPresence, ModalBackdrop } from "./Modal";
import Switch from "./Switch";

/** Agents chosen in New project, most recent first (this computer only). */
export const RECENT_AGENTS = "invectus.recent-agents";

export function rememberAgent(id: string) {
  const before = local.get<string[]>(RECENT_AGENTS, []);
  local.set(RECENT_AGENTS, [id, ...before.filter((x) => x !== id)].slice(0, 8));
}

/** Up to three agents: the chosen one, then recent choices, then the rest in list order. */
export function topAgents(agents: AgentOption[], chosen: string | undefined, recent: string[]): AgentOption[] {
  const order = [chosen, ...recent, ...agents.map((a) => a.id)];
  const seen = new Set<string>();
  const out: AgentOption[] = [];
  for (const id of order) {
    const a = id ? agents.find((x) => x.id === id) : undefined;
    if (a && !seen.has(a.id)) { seen.add(a.id); out.push(a); }
    if (out.length === 3) break;
  }
  return out;
}

interface Props {
  agents: AgentOption[];
  value?: string;
  onChoose: (id: string) => void;
  access: boolean;
  onAccess: (on: boolean) => void;
  disabled?: boolean;
}

export default function AgentCards({ agents, value, onChoose, access, onAccess, disabled }: Props) {
  const [more, setMore] = useState(false);
  const flicker = useFlickerDelays();
  const shown = topAgents(agents, value, local.get<string[]>(RECENT_AGENTS, []));
  return <div className="agent-cards">
    <ul className="np-sources">
      {shown.map((a) => <li key={a.id}>
        <button type="button" className="np-source flicker-in" aria-pressed={a.id === value} disabled={disabled}
          onClick={() => onChoose(a.id)} style={flicker(a.id)}>
          <span className="np-source-mark"><AgentMark id={a.id} size={20} /></span>
          <span className="np-source-name">{a.name}</span>
          <span className="np-source-hint">{a.note}</span>
          {a.id === value && <span className="np-source-check" aria-label="Chosen"><Check size={12} strokeWidth={2.5} aria-hidden="true" /></span>}
        </button>
      </li>)}
      <li>
        <button type="button" className="np-source flicker-in" disabled={disabled} onClick={() => setMore(true)} style={flicker("more")}>
          <span className="np-source-mark"><MoreHorizontal size={20} strokeWidth={1.5} aria-hidden="true" /></span>
          <span className="np-source-name">More</span>
          <span className="np-source-hint">{agents.length} available</span>
        </button>
      </li>
    </ul>
    <label className="agent-access-row">
      <Switch checked={access} disabled={disabled} onChange={onAccess} />
      <span><span className="agent-access-title">Let agents read this project</span>
        <span className="dim small">Agents can search its items, gaps and files through inVectus. What they read can reach their provider. Your next project starts with this choice.</span></span>
    </label>
    <FlickerPresence show={more}>{more && <AgentDialog agents={agents} value={value} onChoose={(id) => { onChoose(id); setMore(false); }} onClose={() => setMore(false)} />}</FlickerPresence>
  </div>;
}

function AgentDialog({ agents, value, onChoose, onClose }: {
  agents: AgentOption[]; value?: string; onChoose: (id: string) => void; onClose: () => void;
}) {
  const box = useRef<HTMLDivElement>(null);
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("keydown", key);
    box.current?.querySelector<HTMLButtonElement>("button")?.focus();
    return () => document.removeEventListener("keydown", key);
  }, [onClose]);
  const flicker = useFlickerDelays();
  return createPortal(<ModalBackdrop onClick={(e) => { if (e.target === e.currentTarget) onClose(); }}>
    <div ref={box} className="integration-modal stack gap-s agent-dialog" role="dialog" aria-modal="true" aria-labelledby="agent-dialog-title">
      <div className="integration-modal-head"><h3 id="agent-dialog-title">Choose an agent</h3></div>
      <p className="dim small">Connected agents and the on-device model. Connect more on the Agents page.</p>
      <ul className="agent-dialog-rows">
        {agents.map((a) => <li key={a.id}>
          <button type="button" className={`choice-row flicker-in${a.id === value ? " on" : ""}`} style={flicker(a.id)} onClick={() => onChoose(a.id)}>
            <span className="agent-dialog-mark"><AgentMark id={a.id} size={18} /></span>
            <span className="agent-dialog-text"><span>{a.name}</span><span className="dim small">{a.note}</span></span>
            {a.id === value && <Check size={14} strokeWidth={2} aria-label="Chosen" />}
          </button>
        </li>)}
      </ul>
      <div className="integration-modal-actions"><button type="button" className="btn" onClick={onClose}>Cancel</button></div>
    </div>
  </ModalBackdrop>, document.body);
}
