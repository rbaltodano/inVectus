import { useEffect, useMemo, useState } from "react";
import { api, fmt, local, when, KIND_LABEL, verdictLabel, VERDICTS, type GapKind, type GapList, type Weights } from "../api";
import { ErrorLine, Needs, useAction, useStartJob, type Ctx } from "../App";
import { JobProgress, isRunning } from "../jobs";

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
  const regionsJob = useStartJob();
  const job = regionsJob.current;

  useEffect(() => { setList(undefined); if (tref && table?.regions?.scanned) run(async () => setList(await api.gaps(tref))); }, [tref?.collection, tref?.space, table?.regions?.run_id, table?.regions?.scanned]);

  const shown = useMemo(() => (list?.gaps ?? []).filter((g) =>
    (!kind || g.kind === kind) && (!verdict || (g.verdict?.verdict ?? "open") === verdict) && (!minScore || g.score >= Number(minScore))), [list, kind, verdict, minScore]);
  const top = shown.reduce((m, g) => Math.max(m, g.score), 1e-9);

  if (!table || !tref) return <div className="page"><Needs title="No gaps yet" body="Gaps are found in an indexed collection. Add and index a collection first." action="Go to Collections" onAction={() => ctx.go({ view: "project", tab: "collections" })} /></div>;
  const findingRegions = isRunning(job) && job!.kind === "regions" && job!.project === project.root && job!.collection === table.collection;
  if (!table.regions) return <div className="page">
    <Needs title="Regions come first" busy={regionsJob.busy || isRunning(job)}
      body={`Gaps are scored within and between regions (groups of related items). “${table.collection}” has none for ${table.model_key} yet. Finding them runs offline and can take minutes on large collections.`}
      action={findingRegions ? undefined : "Find regions"} onAction={() => regionsJob.start(() => api.startRegions(tref))} />
    {findingRegions && <div className="panel"><JobProgress job={job!} /></div>}
    {isRunning(job) && !findingRegions && <p className="muted small">Another job is running ({job!.label}); this can start when it finishes.</p>}
    <ErrorLine error={regionsJob.error} />
  </div>;

  const scan = () => run(async () => {
    await api.scan(tref, local.get<Weights | null>(weightsKey(project.root), null));
    await ctx.refresh();
    setList(await api.gaps(tref));
  });
  const verify = () => run(async () => {
    const ids = [...picked];
    for (const [i, id] of ids.entries()) { setProgress(`Checking ${i + 1} of ${ids.length}…`); await api.verify(tref, id); }
    setPicked(new Set());
    setList(await api.gaps(tref));
  }).finally(() => setProgress(undefined));
  const exportAs = (f: "csv" | "markdown") => run(async () => setExported(await api.exportGaps(tref, f)));
  const toggle = (id: number) => setPicked((s) => { const n = new Set(s); if (n.has(id)) n.delete(id); else n.add(id); return n; });
  const allPicked = shown.length > 0 && shown.every((g) => picked.has(g.gap_id));
  const offline = !project.network_enabled || project.offline_env;
  const scanning = busy && !list;

  return <div className="page wide">
    <div className="title-row">
      <h2 className="h1">Gaps</h2>
      <span className="label">{table.collection} · {table.model_key}</span>
      <div className="row push wrap">
        <button type="button" className="btn primary" disabled={busy || isRunning(job)} onClick={scan}>{scanning ? "Scanning…" : table.regions.scanned ? "Scan again" : "Scan for gaps"}</button>
        {list && <>
          <button type="button" className="btn" disabled={!picked.size || busy || offline} title={offline ? "Switch the network on to check outside sources" : undefined} onClick={verify}>
            {progress ?? `Verify selected${picked.size ? ` (${picked.size})` : ""}`}</button>
          <button type="button" className="btn" disabled={busy} onClick={() => exportAs("markdown")}>Export Markdown</button>
          <button type="button" className="btn" disabled={busy} onClick={() => exportAs("csv")}>Export CSV</button>
        </>}
      </div>
    </div>
    <div className="muted small">Regions found {when(table.regions.created_at)}. Scanning runs offline in seconds and keeps your decisions. Weights are in Settings.{isRunning(job) && job!.project === project.root ? ` Scanning waits while “${job!.label}” runs.` : ""}</div>
    <ErrorLine error={error} />
    {exported && <p className="small mono">Saved to {exported}</p>}
    {offline && picked.size > 0 && <p className="note small">Offline: switch the network on (top right) to check PubMed and OpenAlex.</p>}

    {!table.regions.scanned && !list && <Needs title="Not scanned yet" body="Scan the regions for coverage gaps (demand without coverage), stale coverage and bridges between close regions." action={scanning ? "Scanning…" : "Scan for gaps"} onAction={scan} busy={busy || isRunning(job)} />}

    {list && <>
      <div className="row wrap filters">
        <label className="inline">Type<select value={kind} onChange={(e) => setKind(e.target.value as GapKind | "")}>
          <option value="">All types</option>{(Object.keys(KIND_LABEL) as GapKind[]).map((k) => <option key={k} value={k}>{KIND_LABEL[k]}</option>)}</select></label>
        <label className="inline">Decision<select value={verdict} onChange={(e) => setVerdict(e.target.value)}>
          <option value="">Any</option>{VERDICTS.map(([id, l]) => <option key={id} value={id}>{l}</option>)}</select></label>
        <label className="inline">Min score<input type="number" step="0.5" className="w-90" value={minScore} onChange={(e) => setMinScore(e.target.value)} /></label>
        <span className="muted small">{fmt(shown.length)} of {fmt(list.gaps.length)} · scores compare gaps of the same type</span>
      </div>

      <section aria-label="Gap list" className="panel flush scroll-x">
        {shown.length === 0 ? <p className="pad muted">{list.gaps.length ? "No gaps match these filters." : "The scan found no gaps here."}</p>
          : <table className="table">
            <thead><tr>
              <th><input type="checkbox" aria-label="Select all shown" checked={allPicked} onChange={() => setPicked(allPicked ? new Set() : new Set(shown.map((g) => g.gap_id)))} /></th>
              <th>Score</th><th>Type</th><th>Where</th><th>Why the engine flags it</th><th>Outside sources</th><th>Your decision</th>
            </tr></thead>
            <tbody>{shown.map((g) => <tr key={g.gap_id}>
              <td><input type="checkbox" aria-label={`Select gap ${g.label}`} checked={picked.has(g.gap_id)} onChange={() => toggle(g.gap_id)} /></td>
              <td><div className="score"><b className="mono">{g.score.toFixed(2)}</b><div className="meter thin" aria-hidden="true"><div style={{ width: `${Math.max(0, (g.score / top) * 100)}%` }} /></div></div></td>
              <td><span className="tag">{KIND_LABEL[g.kind]}</span></td>
              <td><button type="button" className="link strong left" onClick={() => ctx.go({ view: "project", tab: "gaps", gap: g.gap_id })}>{g.label}</button></td>
              <td className="w-320 small">{g.explanation}</td>
              <td className="small">{g.verification ? (g.verification.matches ? `${g.verification.matches} look like matches` : "no match found") : <span className="muted">not checked</span>}</td>
              <td className="nowrap small">{verdictLabel(g.verdict?.verdict)}</td>
            </tr>)}</tbody>
          </table>}
      </section>
    </>}
  </div>;
}
