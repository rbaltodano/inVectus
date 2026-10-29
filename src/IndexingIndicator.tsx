import type { CSSProperties } from "react";
import type { JobInfo } from "./api";
import { indexingProgress, SNAKE_ORDER } from "./loading";

export function SnakeLoader() {
  return <span className="snake-loader" aria-hidden="true">
    {Array.from({ length: 9 }, (_, cell) => <span key={cell} className="snake-cell"
      data-step={SNAKE_ORDER.indexOf(cell as typeof SNAKE_ORDER[number])}
      style={{ "--snake-step": SNAKE_ORDER.indexOf(cell as typeof SNAKE_ORDER[number]) } as CSSProperties} />)}
  </span>;
}

export default function IndexingIndicator({ job, preview = false }: { job?: JobInfo; preview?: boolean }) {
  const progress = indexingProgress(job, preview);
  if (!progress) return null;
  const { percent, label } = progress;
  return <div className="indexing-indicator" title={label}>
    <span className="indexing-percent"><SnakeLoader /><span>{percent === undefined ? "—" : `${percent}%`}</span></span>
    <div className={`indexing-bar ${percent === undefined ? "indeterminate" : ""}`} role="progressbar"
      aria-label={label} aria-valuenow={percent} aria-valuemin={0} aria-valuemax={100}>
      <span style={{ width: percent === undefined ? "100%" : `${percent}%` }} />
    </div>
  </div>;
}
