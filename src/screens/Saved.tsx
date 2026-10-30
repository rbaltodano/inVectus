import { useEffect, useState } from "react";
import { api, KIND_LABEL, VERDICTS, verdictLabel, when, type SavedOverview } from "../api";
import { ErrorLine, tableKey, useAction, type Ctx } from "../App";
import LineArt from "../LineArt";

export default function SavedPage({ ctx, active }: { ctx: Ctx; active: boolean }) {
  const { tref, table, project, gaps, gapsLoading, gapsError } = ctx;
  const [saved, setSaved] = useState<SavedOverview>();
  const [error, setError] = useState<string>();
  const [revision, setRevision] = useState(0);
  const [filter, setFilter] = useState("");
  const action = useAction();
  useEffect(() => {
    if (!tref || !active) return;
    let cancelled = false;
    setError(undefined);
    api.saved(tref).then(async (data) => {
      if (!cancelled) { setSaved(data); await ctx.reloadGaps(); }
    })
      .catch((e) => { if (!cancelled) setError(String(e)); });
    return () => { cancelled = true; };
  }, [tref, revision, active, ctx.reloadGaps]);

  const selected = saved?.decisions.filter((d) => d.verdict.verdict === "selected") ?? [];
  const selectedIds = new Set(selected.map((d) => d.gap_id).filter((id) => id !== null));
  const bookmarks = [...(saved?.bookmarks ?? [])].sort((a, b) =>
    Number(b.gap_id !== null && selectedIds.has(b.gap_id)) - Number(a.gap_id !== null && selectedIds.has(a.gap_id)));
  const decisions = [...(saved?.decisions ?? [])].filter((d) => !filter || d.verdict.verdict === filter)
    .sort((a, b) => Number(b.verdict.verdict === "selected") - Number(a.verdict.verdict === "selected"));
  const next = !gapsLoading && !gapsError && saved ? [...gaps].sort((a, b) => b.score - a.score)
    .find((g) => !g.verdict && !saved.bookmarks.some((b) => b.gap_id === g.gap_id)) : undefined;

  return <div className="saved-page stack">
    <div className="row wrap"><span className="section">Saved</span><span className="dim small push">Kept in this project</span></div>
    {project.tables.length > 1 && <label>Collection / index
      <select value={table ? tableKey(table) : ""} onChange={(e) => {
        const picked = project.tables.find((t) => tableKey(t) === e.target.value);
        if (picked) ctx.pickTable(picked);
      }}>{project.tables.map((t) => <option key={tableKey(t)} value={tableKey(t)}>{t.collection} · {t.model_key}</option>)}</select>
    </label>}
    {table && <p className="dim small saved-scope">{table.collection} · {table.model_key}</p>}
    {!tref && <div className="empty-state"><LineArt kind="venn" /><span className="dim">Save gaps and decisions here once a collection is indexed and scanned.</span><button type="button" className="text-btn" onClick={() => ctx.push({ kind: "index" })}>Open Collections</button></div>}
    <ErrorLine error={error} />
    {error && <button type="button" className="text-btn" onClick={() => setRevision((n) => n + 1)}>Retry loading Saved</button>}
    {tref && !saved && !error && <p className="dim small" role="status">Loading saved work…</p>}
    {gapsError && <><ErrorLine error={gapsError} /><button type="button" className="text-btn" disabled={gapsLoading} onClick={() => void ctx.reloadGaps()}>Retry loading current gaps</button></>}
    {saved && <>
      <dl className="figures"><div><dt>Bookmarked</dt><dd>{bookmarks.length}</dd></div><div><dt>Decisions</dt><dd>{saved.decisions.length}</dd></div><div><dt>Selected</dt><dd>{selected.length}</dd></div></dl>
      <section className="saved-section" aria-labelledby="saved-bookmarks">
        <div className="row wrap"><h2 id="saved-bookmarks" className="section">01 / Saved gaps</h2><span className="dim small push">Selected first</span></div>
        {bookmarks.length === 0 && <p className="dim small">No bookmarks yet. Open a gap and choose “Save gap” to keep it here without making a decision.</p>}
        {bookmarks.map((b) => {
          const current = gaps.find((g) => g.gap_id === b.gap_id);
          return <article className="saved-entry" key={`${b.kind}:${b.core_key}`}>
            <div className="row saved-entry-heading"><span className="saved-title">{current?.label ?? b.snapshot.label}</span><span className="score">{(current?.score ?? b.snapshot.score).toFixed(2)}</span></div>
            <p className="dim small">{KIND_LABEL[b.kind as keyof typeof KIND_LABEL]}{current?.verdict ? ` · ${verdictLabel(current.verdict.verdict)}` : ""} · Saved {when(b.saved_at)}</p>
            <div className="row wrap">
              {b.gap_id !== null ? <button type="button" className="text-btn" onClick={() => ctx.push({ kind: "gap", id: b.gap_id! })}>Review evidence</button> : <span className="dim small">No matching gap in the current scan</span>}
              <button type="button" className="text-btn push" disabled={action.busy} onClick={() => action.run(async () => {
                await api.removeBookmark(tref!, b.kind, b.core_key);
                setRevision((n) => n + 1);
              })}>Remove bookmark</button>
            </div>
          </article>;
        })}
        <ErrorLine error={action.error} />
      </section>
      <section className="saved-section" aria-labelledby="saved-decisions">
        <div className="row wrap"><h2 id="saved-decisions" className="section">02 / Decision log</h2>
          <label className="saved-filter push"><span className="sr-only">Filter decisions</span><select value={filter} onChange={(e) => setFilter(e.target.value)}><option value="">All decisions</option>{VERDICTS.map(([id, label]) => <option key={id} value={id}>{label}</option>)}</select></label>
        </div>
        <p className="dim small">Your latest decision for each gap. Selected opportunities appear first.</p>
        {decisions.length === 0 && <p className="dim small">{saved.decisions.length ? "No decisions with this status." : "No decisions yet. Decisions and notes you save while reviewing evidence appear here automatically."}</p>}
        {decisions.map((d) => <article className="saved-entry" key={`${d.verdict.kind}:${d.core_key}`}>
          <div className="row wrap"><span className="label">{verdictLabel(d.verdict.verdict)}</span><span className="dim small push">{when(d.verdict.updated_at)}</span></div>
          <h3 className="saved-title">{d.label}</h3>
          {d.verdict.note && <p className="saved-note">{d.verdict.note}</p>}
          {d.gap_id !== null ? <button type="button" className="text-btn" onClick={() => ctx.push({ kind: "gap", id: d.gap_id! })}>Review evidence / edit decision</button> : <span className="dim small">No matching gap in the current scan · decision retained</span>}
        </article>)}
      </section>
      {next && <section className="saved-section" aria-labelledby="saved-next">
        <div className="row wrap"><h2 id="saved-next" className="section">03 / Next to review</h2><span className="dim small push">Highest ranked · not saved</span></div>
        <article className="saved-entry"><div className="row saved-entry-heading"><h3 className="saved-title">{next.label}</h3><span className="score">{next.score.toFixed(2)}</span></div>
          <p className="dim small">{next.explanation}</p><button type="button" className="text-btn" onClick={() => ctx.push({ kind: "gap", id: next.gap_id })}>Review evidence</button></article>
      </section>}
    </>}
  </div>;
}
