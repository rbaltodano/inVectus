// AI agents working through the app (target product §8 stage 2): the backend sends one
// `agent-activity` event per tool call an agent makes via `invectus mcp`. This keeps the
// recent ones so the sidebar, the project header and Settings can show what agents are doing.
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { inTauri } from "./api";
import AgentIcon from "./AgentIcon";

export interface AgentActivity {
  client: string | null; tool: string; project: string; query: string | null;
  gap_id: number | null; ok: boolean; error: string | null; at_ms: number;
}

/** How long after its last call an agent still counts as active. */
const ACTIVE_MS = 90_000;

const Ctx = createContext<AgentActivity[]>([]);

export function AgentsProvider({ children }: { children: ReactNode }) {
  const [events, setEvents] = useState<AgentActivity[]>([]);
  useEffect(() => {
    if (!inTauri) return;
    const un = listen<AgentActivity>("agent-activity", (e) => setEvents((xs) => [e.payload, ...xs].slice(0, 50)));
    return () => { un.then((f) => f()); };
  }, []);
  return <Ctx.Provider value={events}>{children}</Ctx.Provider>;
}

export const useAgentActivity = () => useContext(Ctx);

/** Re-render every few seconds so "12 s ago" and the active state stay current. */
function useNow(ms = 5000) {
  const [now, setNow] = useState(Date.now());
  useEffect(() => { const t = setInterval(() => setNow(Date.now()), ms); return () => clearInterval(t); }, [ms]);
  return now;
}

const clip = (s: string, n = 40) => (s.length > n ? `${s.slice(0, n - 1)}…` : s);

/** What a tool call did, in a few words. */
export function describe(a: AgentActivity): string {
  const q = a.query ? `“${clip(a.query)}”` : "";
  const gap = a.gap_id !== null ? ` on gap ${a.gap_id}` : "";
  switch (a.tool) {
    case "search": return `searched ${q}`;
    case "check_idea": return `checked ${q}`;
    case "list_gaps": return "read the gaps";
    case "gap_detail": return `read gap ${a.gap_id ?? ""}`.trim();
    case "gap_context": return `read the evidence${gap}`;
    case "suggest_decision": return `suggested a decision${gap}`;
    case "submit_description": return `wrote a description${gap}`;
    case "paraphrase_sample": return "read the calibration sample";
    case "submit_paraphrases": return "recalibrated with its paraphrases";
    case "region_context": return "read the regions";
    case "submit_region_names": return "named regions";
    case "scan_gaps": return "scanned gaps again";
    case "verify_gap": return `checked outside sources${gap}`;
    case "export_gaps": return "exported the gaps";
    case "import_collection": return "imported a collection";
    case "project_status": case "open_project": return "read the project status";
    case "job_status": return "followed the running job";
    case "cancel_job": return "cancelled the running job";
    default: return a.tool.replace(/_/g, " ");
  }
}

export const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  return s < 5 ? "just now" : s < 60 ? `${s} s ago` : `${Math.round(s / 60)} min ago`;
};

/** The latest call of an agent that is still active, optionally in one project. */
export function useActiveAgent(project?: string): AgentActivity | undefined {
  const events = useAgentActivity();
  const now = useNow();
  return events.find((e) => (!project || e.project === project) && now - e.at_ms < ACTIVE_MS);
}

/** The second line of a project's sidebar row: an active agent's latest call, else a quiet
 * fact about the project; and the agent's mark for the right edge of the row. */
export function ProjectRowDetail({ root, fallback }: { root: string; fallback: string }) {
  const a = useActiveAgent(root);
  const now = useNow();
  if (!a) return <span className="project-row-sub">{fallback}</span>;
  const text = `${describe(a)}${!a.ok ? " · failed" : ""}`;
  return <span className="project-row-sub agent" title={`${a.client ?? "An agent"} ${text} · ${ago(a.at_ms, now)}`}>
    {now - a.at_ms < WORKING_MS ? <Shimmer text={text} /> : text}
  </span>;
}

/** How long after a call the agent counts as mid-work (its line shimmers). */
const WORKING_MS = 15_000;

/** Thinking shimmer, letter by letter: one bright peak travels left to right, each letter
 * lighting in strict order one step after its neighbour, then a pause before the next pass.
 * Letters change brightness individually, so it steps rather than glides like a gradient. The
 * step shrinks for long lines so a pass always fits in the cycle. Screen readers get the text. */
export function Shimmer({ text }: { text: string }) {
  const chars = Array.from(text);
  // The pass takes at most 70% of the 2 s cycle; the rest is the pause.
  const step = Math.min(60, 1400 / Math.max(1, chars.length));
  return <span className="shimmer" aria-label={text}>
    {chars.map((ch, i) => <span key={i} aria-hidden="true" className="shimmer-ch"
      style={{ animationDelay: `${(i * step).toFixed(1)}ms` }}>{ch}</span>)}
  </span>;
}

export function ProjectRowAgent({ root }: { root: string }) {
  const a = useActiveAgent(root);
  if (!a) return null;
  return <span className="project-row-agent" title={`${a.client ?? "An agent"} is working here`} aria-label={`${a.client ?? "An agent"} is working here`}>
    <AgentIcon client={a.client} />
  </span>;
}
