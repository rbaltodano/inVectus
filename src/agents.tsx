// AI agents working through the app (target product §8 stage 2): the backend sends one
// `agent-activity` event per tool call an agent makes via `invectus mcp`. This keeps the
// recent ones so the sidebar, the project header and Settings can show what agents are doing.
import { createContext, useContext, useEffect, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import AgentIcon from "./AgentIcon";
import { clientName, registerClients } from "./agentIdentity";
import { api, inTauri } from "./api";

import { describe, recordActivity, type AgentActivity } from "./agent-activity";
export { describe, type AgentActivity } from "./agent-activity";

/** How long after its last call an agent still counts as active. */
const ACTIVE_MS = 90_000;

const Ctx = createContext<AgentActivity[]>([]);

export function AgentsProvider({ children }: { children: ReactNode }) {
  const [events, setEvents] = useState<AgentActivity[]>([]);
  useEffect(() => {
    if (!inTauri) return;
    // Client names for the sidebar and attributions; a failure keeps the built-in names.
    api.connectClients().then(registerClients).catch(() => undefined);
    const un = listen<AgentActivity>("agent-activity", (e) => setEvents((xs) => recordActivity(xs, e.payload)));
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

export const ago = (ms: number, now: number) => {
  const s = Math.max(0, Math.round((now - ms) / 1000));
  return s < 5 ? "just now" : s < 60 ? `${s} s ago` : `${Math.round(s / 60)} min ago`;
};

/** The latest call of an agent that is still active, optionally in one project. */
export function useActiveAgent(project?: string): AgentActivity | undefined {
  const events = useAgentActivity();
  const now = useNow();
  return events.find((e) => (!project || e.project === project) && (e.phase === "started" || now - e.at_ms < ACTIVE_MS));
}

/** The second line of a project's sidebar row: an active agent's latest call, else a quiet
 * fact about the project; and the agent's mark for the right edge of the row. */
export function ProjectRowDetail({ root, fallback }: { root: string; fallback: string }) {
  const a = useActiveAgent(root);
  const now = useNow();
  if (!a) return <span className="project-row-sub">{fallback}</span>;
  const text = describe(a);
  return <span className="project-row-sub agent" title={`${clientName(a.client)} ${text} · ${ago(a.at_ms, now)}`}>
    {a.phase === "started" ? <Shimmer text={text} /> : text}
  </span>;
}

/** Terminal-style thinking shimmer. Like a CLI, it repaints on a tick (120 ms, about 8 fps):
 * the bright head moves exactly one letter per tick, and every letter snaps to one of three
 * brightness levels by its distance from the head, with nothing in between, so the band jumps
 * letter by letter instead of gliding like a CSS gradient. A short pause follows each pass.
 * Screen readers get the plain text; reduced motion shows it still. */
const TICK_MS = 120;
const LEVELS = [1, 0.65]; // distance 0, 1 from the head; further letters use BASE
const BASE = 0.35;
const PAUSE_TICKS = 6;

export function Shimmer({ text }: { text: string }) {
  const chars = Array.from(text);
  const span = chars.length + 2 * LEVELS.length;
  const [tick, setTick] = useState(0);
  const still = typeof window !== "undefined" && window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  useEffect(() => {
    if (still) return;
    const t = setInterval(() => setTick((n) => n + 1), TICK_MS);
    return () => clearInterval(t);
  }, [still]);
  // The head starts just before the first letter and leaves just after the last.
  const head = (tick % (span + PAUSE_TICKS)) - LEVELS.length;
  return <span className="shimmer" aria-label={text}>
    {chars.map((ch, i) => {
      const d = Math.abs(i - head);
      return <span key={i} aria-hidden="true" className="shimmer-ch"
        style={{ opacity: still ? 1 : d < LEVELS.length ? LEVELS[d] : BASE }}>{ch}</span>;
    })}
  </span>;
}

export function ProjectRowAgent({ root }: { root: string }) {
  const a = useActiveAgent(root);
  if (!a) return null;
  return <span className="project-row-agent" title={`${clientName(a.client)} · ${describe(a)}`} aria-label={`${clientName(a.client)} · ${describe(a)}`}>
    <AgentIcon client={a.client} />
  </span>;
}

/** Recent activity for this project, including errors from completed calls. */
export function AgentStatus({ root }: { root: string }) {
  const events = useAgentActivity().filter((e) => e.project === root);
  const now = useNow();
  if (!events.length) return null;
  return <details className="agent-status">
    <summary><span aria-live="polite">{clientName(events[0].client)} · {describe(events[0])}</span></summary>
    <ol>{events.slice(0, 5).map((event, i) => <li key={event.call_id ?? `${event.at_ms}-${i}`}>
      <span className={event.ok ? "" : "err"}>{clientName(event.client)} · {describe(event)}</span>
      <span className="dim small">{event.phase === "started" ? "Running" : ago(event.at_ms, now)}</span>
      {event.error && <p className="err small">{event.error}</p>}
    </li>)}</ol>
  </details>;
}
