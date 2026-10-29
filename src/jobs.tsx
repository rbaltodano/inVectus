// Background jobs (embed, calibrate, regions, model download): one place listens to the
// backend's events so the sidebar and every screen see the same state.
import { createContext, useContext, useEffect, useRef, useState, type ReactNode } from "react";
import { listen } from "@tauri-apps/api/event";
import { inTauri } from "./api";

export type JobKind = "embed" | "calibrate" | "regions" | "download";
export interface Job {
  kind: JobKind;
  label: string;
  status: "running" | "finished" | "failed";
  done: number;
  total: number;
  phase?: string;
  startedAt: number;
  /** Items per second over the run so far (embed only). */
  rate?: number;
  message?: string;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  report?: any;
}

interface Jobs {
  current?: Job;
  begin: (kind: JobKind, label: string) => void;
  clear: () => void;
}

const Ctx = createContext<Jobs>({ begin: () => undefined, clear: () => undefined });
export const useJobs = () => useContext(Ctx);

type Payload =
  | { status: "progress"; done: number; total: number; phase?: string }
  | { status: "finished"; report: unknown }
  | { status: "failed"; error: string }
  | { status: "log"; line: string; model: string };

export function JobsProvider({ children, onFinished }: { children: ReactNode; onFinished: () => void }) {
  const [current, setCurrent] = useState<Job>();
  const finished = useRef(onFinished);
  finished.current = onFinished;

  useEffect(() => {
    if (!inTauri) return;
    const on = (kind: JobKind, event: string) => listen<Payload>(event, ({ payload: p }) => {
      setCurrent((j) => {
        const base: Job = j?.kind === kind ? j : { kind, label: kind, status: "running", done: 0, total: 0, startedAt: Date.now() };
        switch (p.status) {
          case "progress": {
            const secs = (Date.now() - base.startedAt) / 1000;
            return { ...base, status: "running", done: p.done, total: p.total, phase: p.phase, rate: secs > 1 ? p.done / secs : undefined };
          }
          case "log": return { ...base, message: p.line };
          case "finished": return { ...base, status: "finished", report: p.report, done: base.total || base.done };
          case "failed": return { ...base, status: "failed", message: p.error };
        }
      });
      if (p.status === "finished" || p.status === "failed") finished.current();
    });
    const subs = [on("embed", "embed-event"), on("calibrate", "calibrate-event"), on("regions", "regions-event"), on("download", "download-event")];
    return () => { subs.forEach((s) => s.then((f) => f())); };
  }, []);

  const begin = (kind: JobKind, label: string) =>
    setCurrent({ kind, label, status: "running", done: 0, total: 0, startedAt: Date.now() });
  return <Ctx.Provider value={{ current, begin, clear: () => setCurrent(undefined) }}>{children}</Ctx.Provider>;
}

export function JobProgress({ job }: { job: Job }) {
  const pct = job.total ? Math.round((job.done / job.total) * 100) : 0;
  const left = job.rate && job.total ? Math.max(0, (job.total - job.done) / job.rate) : undefined;
  return <div className="job-progress">
    <div className="spread"><span>{job.phase ?? job.label}</span><span>{job.total ? `${job.done.toLocaleString()} / ${job.total.toLocaleString()}` : ""}</span></div>
    <div className="bar" role="progressbar" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100} aria-label={job.label}><div style={{ width: `${pct}%` }} /></div>
    {left !== undefined && <div className="muted small">{job.rate!.toFixed(0)}/s · ~{left < 90 ? `${Math.ceil(left)} s` : `${Math.ceil(left / 60)} min`} left</div>}
    {job.message && job.status === "running" && <div className="muted small clip">{job.message}</div>}
  </div>;
}
