import type { CSSProperties } from "react";
import type { JobInfo } from "./api";
import { indexingProgress, LOADER_ORDER } from "./loading";

export function MatrixLoader() {
  return <span className="matrix-loader" aria-hidden="true">
    {Array.from({ length: LOADER_ORDER.length }, (_, cell) => <span key={cell} className="matrix-cell"
      data-step={LOADER_ORDER.indexOf(cell as typeof LOADER_ORDER[number])}
      style={{ "--matrix-step": LOADER_ORDER.indexOf(cell as typeof LOADER_ORDER[number]) } as CSSProperties} />)}
  </span>;
}

export default function IndexingIndicator({ job }: { job?: JobInfo }) {
  const progress = indexingProgress(job);
  if (!progress) return null;
  const { percent, label } = progress;
  return <div className="indexing-indicator" title={label}>
    <span className="indexing-percent"><MatrixLoader /><span>{percent === undefined ? "—" : `${percent}%`}</span></span>
    <div className={`indexing-bar ${percent === undefined ? "indeterminate" : ""}`} role="progressbar"
      aria-label={label} aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
      <span style={{ width: percent === undefined ? "100%" : `${percent}%` }} />
    </div>
  </div>;
}
