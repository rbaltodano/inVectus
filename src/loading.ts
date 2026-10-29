import type { JobInfo } from "./api";

// Row-major indices in a 2-column, 3-row grid, traversed clockwise.
export const LOADER_ORDER = [0, 1, 3, 5, 4, 2] as const;
export const INDEXING_PREVIEW_PERCENT = 84;

export function indexingProgress(job: JobInfo | undefined, preview: boolean) {
  const running = job?.state === "running" && job.kind !== "download";
  if (!running && !preview) return undefined;
  const percent = running
    ? job.total > 0 ? Math.max(0, Math.min(100, Math.round(100 * job.done / job.total))) : undefined
    : INDEXING_PREVIEW_PERCENT;
  return { percent, preview: !running, label: running ? job.phase ?? job.label : "Indexing animation preview" };
}
