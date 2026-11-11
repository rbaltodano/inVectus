// Add to Dataset: choose one or more sources, as on the New project screen, then each goes
// through Import in turn (columns, roles, preview, import).
import { useState } from "react";
import type { Ctx } from "../App";
import { isRunning, useJobs } from "../jobs";
import SourcePicker, { type Picked } from "../SourcePicker";

export default function AddData({ ctx, onIntegrations }: { ctx: Ctx; onIntegrations: () => void }) {
  const [sources, setSources] = useState<Picked[]>([]);
  const job = useJobs().current;
  const busy = isRunning(job) && job!.project === ctx.project.root;
  const start = () => {
    const [first, ...rest] = sources.map((p) => p.path);
    ctx.push(first ? { kind: "import", path: first, next: rest } : { kind: "import" });
  };
  return <div className="add-data stack gap">
    <div className="title-row"><h2 className="h1">Add to Dataset</h2><span className="label">into {ctx.project.name}</span></div>
    <p className="dim small">Choose where the data comes from. Several sources import one after another, each as its own collection.</p>
    <SourcePicker picked={sources} onChange={setSources} onIntegrations={onIntegrations} disabled={busy} />
    {busy && <p className="note small">“{job!.label}” is running; imports can start when it finishes.</p>}
    <div className="row add-data-actions">
      <button type="button" className="btn primary" disabled={!sources.length || busy} onClick={start}>
        {sources.length > 1 ? `Import ${sources.length} sources` : "Import"}</button>
      <button type="button" className="text-btn" disabled={busy} onClick={() => ctx.push({ kind: "import" })}>Paste a path or link instead</button>
    </div>
  </div>;
}
