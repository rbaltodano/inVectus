import { useEffect, useState } from "react";
import { api, fmt, when, KIND_LABEL, STATUS_LABEL, verdictLabel, type GapView, type JobInfo, type JobRecord } from "../api";
import { ErrorLine, type Ctx } from "../App";
import { isRunning, jobStateLabel, useJobs } from "../jobs";
import { StatusMark } from "./Collections";

const KIND_NAME: Record<string, string> = { embed: "Index", calibrate: "Calibration", regions: "Regions" };

export default function Overview({ ctx }: { ctx: Ctx }) {
  const { project, table, tref } = ctx;
  const jobs = useJobs();
  const [gaps, setGaps] = useState<GapView[]>();
  const [gapError, setGapError] = useState<string>();

  useEffect(() => {
    setGaps(undefined); setGapError(undefined);
    if (tref && table?.regions?.scanned) api.gaps(tref).then((l) => setGaps(l.gaps)).catch((e) => setGapError(String(e)));
  }, [tref?.collection, tref?.space, table?.regions?.run_id, table?.regions?.scanned]);

  const addCollection = () => ctx.go({ view: "project", tab: "collections", adding: true });

  if (project.collections.length === 0) return <div className="page">
    <section className="panel empty">
      <span className="ring" aria-hidden="true" />
      <h2>Add the first collection</h2>
      <p className="muted">Import a CSV or TSV, a Parquet file or a folder of .txt/.md files. inVectus indexes it on this computer, then you can search it for what is missing and scan it for gaps.</p>
      <button type="button" className="btn primary" onClick={addCollection}>+ Add collection</button>
    </section>
    <p className="muted small">The project is ready to use: settings and the network switch work without a collection.</p>
  </div>;

  // The pipeline for the scope in view, from recorded facts only.
  const stages: [string, boolean, string][] = [
    ["Imported", true, table ? `${fmt(project.collections.find((c) => c.collection_id === table.collection)?.items ?? 0)} items` : ""],
    ["Indexed", table?.index_status === "indexed" || table?.index_status === "imported", table ? STATUS_LABEL[table.index_status] : "no index"],
    ["Calibrated", !!table?.calibration, table?.calibration ? "verdicts on" : "search shows similarity only"],
    ["Regions", !!table?.regions, table?.regions ? `${fmt(Number(table.regions.stats.fine ?? 0))} regions` : "map and gaps need them"],
    ["Gaps scored", !!table?.regions?.scanned, gaps ? `${fmt(gaps.length)} findings` : table?.regions?.scanned ? "" : "not scanned"],
  ];
  const mine = [...(isRunning(jobs.current) && jobs.current!.project === project.root ? [jobs.current!] : []),
    ...jobs.history.filter((j) => j.project === project.root)].slice(0, 6);
  const decided = gaps?.filter((g) => g.verdict && g.verdict.verdict !== "open").length ?? 0;

  return <div className="page">
    <div className="row wrap">
      <button type="button" className="btn primary" onClick={() => ctx.go({ view: "project", tab: "search" })} disabled={!table}>Search</button>
      <button type="button" className="btn" onClick={() => ctx.go({ view: "project", tab: "gaps" })} disabled={!table}>Scan for gaps</button>
      <button type="button" className="btn" onClick={addCollection}>+ Add collection</button>
    </div>

    {table && <section className="panel stack gap" aria-labelledby="pipe">
      <div className="title-row"><h2 id="pipe">Status</h2><span className="label">{table.collection} · {table.model_key}</span></div>
      <ol className="pipeline-diagram" aria-label="What this scope has">
        {stages.map(([name, done, note], i) => <li key={name} data-done={done}>
          <span className="node" aria-hidden="true">{i + 1}</span>
          <span className="mono small strong">{name}</span>
          <span className="muted small">{done ? "✓ " : ""}{note}</span>
        </li>)}
      </ol>
      {!table.calibration && <div className="row wrap small"><span className="grow">Search can rank similar items now; a calibrated verdict (“does this exist?”) needs calibration.</span>
        <button type="button" className="btn small" onClick={() => ctx.go({ view: "project", tab: "collections", collection: table.collection })}>Calibrate in Collections</button></div>}
    </section>}

    <section className="panel stack gap" aria-labelledby="ready">
      <h2 id="ready">Collections</h2>
      <table className="table compact">
        <thead><tr><th><span className="sr-only">State</span></th><th>Collection</th><th>Items</th><th>Indexes</th></tr></thead>
        <tbody>{project.collections.map((c) => {
          const ts = project.tables.filter((t) => t.collection === c.collection_id);
          const indexing = isRunning(jobs.current) && jobs.current!.kind === "index" && jobs.current!.project === project.root && jobs.current!.collection === c.collection_id;
          return <tr key={c.collection_id}>
            <td><StatusMark status={indexing ? "indexing" : ts.length === 0 ? "none" : ts[0].index_status} /></td>
            <td><button type="button" className="link mono" onClick={() => ctx.go({ view: "project", tab: "collections", collection: c.collection_id })}>{c.collection_id}</button></td>
            <td className="mono small">{fmt(c.items)}</td>
            <td className="small">{indexing ? "Indexing…" : ts.length === 0 ? "Not indexed" : ts.map((t) => `${t.model_key}: ${STATUS_LABEL[t.index_status].toLowerCase()}`).join(" · ")}</td>
          </tr>;
        })}</tbody>
      </table>
    </section>

    {table?.regions?.scanned && <section className="panel stack gap" aria-labelledby="find">
      <div className="title-row"><h2 id="find">Top findings</h2>{gaps && <span className="label">{fmt(gaps.length)} gaps · {fmt(decided)} decided</span>}
        <button type="button" className="link push" onClick={() => ctx.go({ view: "project", tab: "gaps" })}>All gaps</button></div>
      <ErrorLine error={gapError} />
      {gaps && <ol className="findings">{gaps.slice(0, 5).map((g) => <li key={g.gap_id}>
        <span className="mono small score-chip">{g.score.toFixed(2)}</span>
        <span className="tag">{KIND_LABEL[g.kind]}</span>
        <button type="button" className="link" onClick={() => ctx.go({ view: "project", tab: "gaps", gap: g.gap_id })}>{g.label}</button>
        <span className="muted small push">{verdictLabel(g.verdict?.verdict)}</span>
      </li>)}</ol>}
    </section>}

    <section className="panel stack gap" aria-labelledby="act">
      <h2 id="act">Recent activity</h2>
      {mine.length > 0 && <ul className="log">{mine.map((j) => <li key={`s${j.id}`}>
        <span className="mono small">{when(j.finished_ms ?? j.started_ms)}</span>
        <span>{j.label}</span>
        <span className={`small ${j.state === "failed" ? "error-text" : "muted"}`}>{jobStateLabel(j)}{j.error ? `: ${j.error}` : ""}</span>
      </li>)}</ul>}
      <ActivityLog records={project.activity} running={isRunning(jobs.current) && jobs.current!.project === project.root ? jobs.current : undefined} />
    </section>
  </div>;
}

/** Jobs recorded in the project (they survive restarts). */
function ActivityLog({ records, running }: { records: JobRecord[]; running?: JobInfo }) {
  // A record still marked running is live only if this session's job is that job.
  const live = (r: JobRecord) => !!running && (running.kind === "index" ? "embed" : running.kind) === r.kind && r.job_id.split(":")[1] === running.collection;
  if (!records.length) return <p className="muted small">No jobs recorded yet.</p>;
  return <>
    <div className="label">Recorded in this project</div>
    <ul className="log">{records.map((r) => <li key={r.job_id}>
      <span className="mono small">{when(r.updated_at)}</span>
      <span>{KIND_NAME[r.kind] ?? r.kind} · <span className="mono">{r.job_id.split(":")[1] ?? ""}</span>{r.params.model ? ` · ${String(r.params.model)}` : ""}</span>
      <span className={`small ${r.state === "failed" ? "error-text" : "muted"}`}>{r.state === "running" ? (live(r) ? "running" : "did not finish (interrupted)") : r.state}</span>
    </li>)}</ul>
  </>;
}
