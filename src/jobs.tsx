// Background jobs live in the backend (one at a time, tied to a project). This mirrors them:
// it reads the backend's list on start and follows `job-event`, so every screen sees the same
// state, and nothing is lost when a screen closes.
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { api, inTauri, type JobInfo } from "./api";

interface Jobs {
  current?: JobInfo;
  history: JobInfo[];
  /** Record a job the backend just accepted (its first event may arrive after the reply). */
  started: (j: JobInfo) => void;
}

const Ctx = createContext<Jobs>({ history: [], started: () => undefined });
export const useJobs = () => useContext(Ctx);

export function JobsProvider({ children, onFinished }: { children: ReactNode; onFinished: (j: JobInfo) => void }) {
  const [current, setCurrent] = useState<JobInfo>();
  const [history, setHistory] = useState<JobInfo[]>([]);
  const finished = useRef(onFinished);
  finished.current = onFinished;

  const apply = (j: JobInfo) => {
    setCurrent((c) => {
      // A newer job replaces the shown one; the old one moves to history.
      if (c && c.id !== j.id) setHistory((h) => [c, ...h.filter((x) => x.id !== c.id)].slice(0, 30));
      return c && c.id > j.id ? c : j;
    });
    if (j.state !== "running") finished.current(j);
  };

  useEffect(() => {
    if (!inTauri) return;
    api.jobs().then((v) => { if (v.current) setCurrent(v.current); setHistory(v.history); }).catch(() => undefined);
    const un = listen<JobInfo>("job-event", (e) => apply(e.payload));
    return () => { un.then((f) => f()); };
  }, []);

  return <Ctx.Provider value={{ current, history, started: apply }}>{children}</Ctx.Provider>;
}

export const isRunning = (j?: JobInfo) => j?.state === "running";

export function JobProgress({ job }: { job: JobInfo }) {
  const pct = job.total ? Math.round((job.done / job.total) * 100) : 0;
  const secs = (Date.now() - job.started_ms) / 1000;
  const rate = secs > 2 && job.done > 0 ? job.done / secs : undefined;
  const left = rate && job.total ? (job.total - job.done) / rate : undefined;
  return <div className="job-progress">
    <div className="spread mono small"><span className="clip">{job.phase ?? "Starting"}</span><span>{job.total ? `${job.done.toLocaleString()} / ${job.total.toLocaleString()}` : ""}</span></div>
    {job.total > 0 && <div className="meter" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label={job.label}><div style={{ width: `${pct}%` }} /></div>}
    {left !== undefined && left > 1 && <div className="muted small mono">{rate!.toFixed(0)}/s · ~{left < 90 ? `${Math.ceil(left)} s` : `${Math.ceil(left / 60)} min`} left</div>}
  </div>;
}

export function jobStateLabel(j: JobInfo) {
  if (j.state === "running") return "running";
  if (j.state === "failed") return "failed";
  if (j.state === "cancelled") return "cancelled";
  return "done";
}
