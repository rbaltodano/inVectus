import type { JobInfo } from "./api";

// Row-major cell indices: down column one, up column two, down column three.
export const SNAKE_ORDER = [0, 3, 6, 7, 4, 1, 2, 5, 8] as const;
export const INDEXING_PREVIEW_PERCENT = 84;

export function indexingProgress(job: JobInfo | undefined, preview: boolean) {
  const running = job?.state === "running" && job.kind !== "download";
  if (!running && !preview) return undefined;
  const percent = running
    ? job.total > 0 ? Math.max(0, Math.min(100, Math.round(100 * job.done / job.total))) : undefined
    : INDEXING_PREVIEW_PERCENT;
  return { percent, preview: !running, label: running ? job.phase ?? job.label : "Indexing animation preview" };
}
