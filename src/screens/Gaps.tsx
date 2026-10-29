import { useEffect, useMemo, useState } from "react";
import { api, fmt, local, KIND_LABEL, verdictLabel, VERDICTS, type GapKind, type GapList, type Weights } from "../api";
import { ErrorLine, Needs, useAction, type Ctx } from "../App";

export const weightsKey = (root: string) => `invectus.weights.${root}`;

export default function Gaps({ ctx }: { ctx: Ctx }) {
  const { table, tref, project } = ctx;
  const [list, setList] = useState<GapList>();
  const [kind, setKind] = useState<GapKind | "">("");
  const [verdict, setVerdict] = useState("");
  const [minScore, setMinScore] = useState("");
  const [picked, setPicked] = useState<Set<number>>(new Set());
  const [progress, setProgress] = useState<string>();
  const [exported, setExported] = useState<string>();
  const { busy, error, run } = useAction();

  const load = () => tref && run(async () => setList(await api.gaps(tref)));
  useEffect(() => { if (table?.regions?.scanned) load(); }, [tref?.collection, tref?.space, table?.regions?.run_id]);

  const shown = useMemo(() => (list?.gaps ?? []).filter((g) =>
    (!kind || g.kind === kind) && (!verdict || (g.verdict?.verdict ?? "open") === verdict) && (!minScore || g.score >= Number(minScore))), [list, kind, verdict, minScore]);
  const top = Math.max(1e-9, ...shown.map((g) => g.score));

  if (!table || !tref) return <div className="page narrow"><Needs title="No gaps yet" body="Embed a collection first." action="Go to Index" onAction={() => ctx.go({ name: "index" })} /></div>;
  if (!table.regions) return <div className="page narrow"><Needs title="No regions yet" body="Gaps are found between and within regions. Find regions first." action="Go to Index" onAction={() => ctx.go({ name: "index", collection: table.collection })} /></div>;

  const scan = () => run(async () => {
    await api.scan(tref, local.get<Weights | null>(weightsKey(project.root), null));
    await ctx.refresh();
    setList(await api.gaps(tref));
  });
  if (!table.regions.scanned) return <div className="page narrow"><ErrorLine error={error} />
    <Needs title="Not scored yet" body="Score the latest regions for coverage gaps, stale coverage and bridges. It takes seconds and runs offline." action={busy ? "Scoring…" : "Score gaps"} onAction={scan} /></div>;

  const verify = () => run(async () => {
    const ids = [...picked];
    for (const [i, id] of ids.entries()) { setProgress(`Checking ${i + 1} of ${ids.length}…`); await api.verify(tref, id); }
    setProgress(undefined); setPicked(new Set());
    setList(await api.gaps(tref));
  }).finally(() => setProgress(undefined));
  const exportAs = (f: "csv" | "markdown") => run(async () => setExported(await api.exportGaps(tref, f)));
  const toggle = (id: number) => setPicked((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const allPicked = shown.length > 0 && shown.every((g) => picked.has(g.gap_id));
  const offline = !project.network_enabled || project.offline_env;

  return <div className="page wide">
    <div className="title-row">
      <h1>Gaps</h1>
      <span className="muted">{fmt(shown.length)} of {fmt(list?.gaps.length ?? 0)} · ranked by score</span>
      <div className="row push wrap">
        <button type="button" className="btn" disabled={!picked.size || busy || offline} title={offline ? "Turn off offline mode to check outside sources" : undefined} onClick={verify}>
          {progress ?? `Verify selected${picked.size ? ` (${picked.size})` : ""}`}</button>
        <button type="button" className="btn" disabled={busy} onClick={() => exportAs("markdown")}>Export Markdown</button>
        <button type="button" className="btn" disabled={busy} onClick={() => exportAs("csv")}>Export CSV</button>
        <button type="button" className="btn" disabled={busy} onClick={scan}>Score again</button>
      </div>
    </div>
    <ErrorLine error={error} />
    {exported && <p className="ok-text small">Saved to {exported}</p>}
    {offline && picked.size > 0 && <p className="muted small">Offline mode is on: turn it off at the top right to check PubMed and OpenAlex.</p>}

    <div className="row wrap filters">
      <label className="inline">Type<select value={kind} onChange={(e) => setKind(e.target.value as GapKind | "")}>
        <option value="">All types</option>{(Object.keys(KIND_LABEL) as GapKind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}</select></label>
      <label className="inline">Verdict<select value={verdict} onChange={(e) => setVerdict(e.target.value)}>
        <option value="">Any verdict</option>{VERDICTS.map(([id, l]) => <option key={id} value={id}>{l}</option>)}</select></label>
      <label className="inline">Min score<input type="number" step="0.5" className="w-90" value={minScore} onChange={(e) => setMinScore(e.target.value)} /></label>
      <span className="muted small">Scores compare gaps of the same type; higher is a stronger claim.</span>
    </div>

    <section aria-label="Gap list" className="card flush scroll-x">
      {shown.length === 0 ? <p className="pad muted">{list?.gaps.length ? "No gaps match these filters." : "The scan found no gaps in this collection."}</p>
        : <table className="table">
          <thead><tr>
            <th><input type="checkbox" aria-label="Select all" checked={allPicked} onChange={() => setPicked(allPicked ? new Set() : new Set(shown.map((g) => g.gap_id)))} /></th>
            <th>Score</th><th>Type</th><th>Where</th><th>Why it’s a gap</th><th>Outside sources</th><th>Verdict</th>
          </tr></thead>
          <tbody>{shown.map((g) => <tr key={g.gap_id}>
            <td><input type="checkbox" aria-label={`Select gap ${g.gap_id}`} checked={picked.has(g.gap_id)} onChange={() => toggle(g.gap_id)} /></td>
            <td><div className="score"><b>{g.score.toFixed(2)}</b><div className="bar thin" aria-hidden="true"><div style={{ width: `${Math.max(0, (g.score / top) * 100)}%` }} /></div></div></td>
            <td><span className={`chip kind ${g.kind}`}>{KIND_LABEL[g.kind]}</span></td>
            <td><button type="button" className="link strong left" onClick={() => ctx.go({ name: "gap", id: g.gap_id })}>{g.label}</button></td>
            <td className="w-320">{g.explanation}</td>
            <td className="small">{g.verification ? <span className={g.verification.matches ? "warn-text" : "muted"}>{g.verification.matches ? `${g.verification.matches} look like matches` : "no match found"}</span> : <span className="muted">not checked</span>}</td>
            <td className="nowrap">{verdictLabel(g.verdict?.verdict)}</td>
          </tr>)}</tbody>
        </table>}
    </section>
  </div>;
}
