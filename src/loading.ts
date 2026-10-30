import type { JobInfo } from "./api";

// Row-major indices in a 2-column, 3-row grid, traversed clockwise.
export const LOADER_ORDER = [0, 1, 3, 5, 4, 2] as const;
/** Only actual running work can show progress; idle and completed jobs stay hidden. */
export function indexingProgress(job: JobInfo | undefined) {
  if (!job || job.state !== "running" || job.kind === "download") return undefined;
  const percent = job.total > 0 ? Math.max(0, Math.min(100, Math.round(100 * job.done / job.total))) : undefined;
  return { percent, label: job.phase ?? job.label };
}
