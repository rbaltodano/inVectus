import { useState, type FormEvent } from "react";
import { api, fmt, year, type CheckResult, type Role, type SearchResult } from "../api";
import { ErrorLine, Needs, useAction, type Ctx } from "../App";
import Link from "../Link";

const VERDICT = {
  open: { label: "Open", line: "Nothing in this collection is the same thing." },
  near: { label: "Near", line: "Related work exists, but not this exact thing." },
  exists: { label: "Exists", line: "Something very close is already here." },
};

export default function Search({ ctx, initial }: { ctx: Ctx; initial?: string }) {
  const { table, tref } = ctx;
  const [query, setQuery] = useState(initial ?? "");
  const [scope, setScope] = useState<Role | "">("coverage");
  const [fast, setFast] = useState(false);
  const [check, setCheck] = useState<CheckResult>();
  const [plain, setPlain] = useState<SearchResult>();
  const { busy, error, run } = useAction();

  if (!table || !tref) return <div className="page narrow"><Needs title="Nothing to search yet"
    body="Embed a collection first: search compares your idea with every item's vector." action="Go to Index" onAction={() => ctx.go({ name: "index" })} /></div>;

  const calibrated = !!table.calibration;
  const hasRole = (r: Role) => (ctx.project.collections.find((c) => c.collection_id === table.collection)?.by_role[r] ?? 0) > 0;
  const role = scope && hasRole(scope) ? scope : null;
  const go = (e?: FormEvent) => {
    e?.preventDefault();
    if (!query.trim()) return;
    run(async () => {
      if (calibrated) { setCheck(await api.check(tref, query, role, fast)); setPlain(undefined); }
      else { setPlain(await api.search(tref, query, role, !fast)); setCheck(undefined); }
    });
  };
  const result = check?.search ?? plain;
  const v = check && VERDICT[check.verdict];
  const pct = check ? Math.round(check.p_exists * 100) : 0;

  return <div className="page">
    <form role="search" className="card stack gap" onSubmit={go}>
      <label htmlFor="q" className="h2">Does this already exist?</label>
      <div className="row">
        <input id="q" type="search" className="grow big-input" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Describe your idea in a sentence or two" />
        <button type="submit" className="btn primary" disabled={!query.trim() || busy}>{busy ? "Checking…" : "inVect"}</button>
      </div>
      <div className="row wrap">
        <label className="inline">Compare against
          <select value={scope} onChange={(e) => setScope(e.target.value as Role | "")}>
            <option value="coverage">Coverage only</option><option value="">Everything</option><option value="demand">Demand only</option>
          </select></label>
        {scope && !hasRole(scope) && <span className="muted small">No {scope} items in this collection: comparing against everything.</span>}
        <div role="radiogroup" aria-label="Search mode" className="segmented push">
          <button type="button" role="radio" aria-checked={!fast} onClick={() => setFast(false)}>Check every item</button>
          <button type="button" role="radio" aria-checked={fast} onClick={() => setFast(true)}>Fast</button>
        </div>
      </div>
    </form>
    <ErrorLine error={error} />

    {!calibrated && <div className="card row wrap">
      <span><b>No verdicts yet.</b> <span className="muted">This collection is not calibrated, so results show similarity only, not whether your idea exists.</span></span>
      <button type="button" className="btn push" onClick={() => ctx.go({ name: "index", collection: table.collection })}>Calibrate</button>
    </div>}

    {check && v && <section aria-labelledby="verdict" className="card stack gap">
      <div className="row wrap">
        <span id="verdict" className={`verdict ${check.verdict}`}>{v.label}</span>
        <span className="strong">{check.summary}</span>
      </div>
      <div className="stack gap-s">
        <div className="spread"><span>Chance it already exists</span><b>{pct}%</b></div>
        <div className="bar" role="meter" aria-label="Chance it already exists" aria-valuenow={pct} aria-valuemin={0} aria-valuemax={100}><div style={{ width: `${pct}%` }} /></div>
        <div className="muted small">{check.calibration.quality}{check.calibration.stale ? " The collection changed since calibration." : ""}</div>
      </div>
      <div>{check.next_step}</div>
      {check.between.length > 0 && <div>Your idea sits {check.between.length === 2 ? "between " : "in "}
        {check.between.map((r, i) => <span key={r.region_id}>{i > 0 && " and "}<button type="button" className="link" onClick={() => ctx.go({ name: "map", region: r.region_id })}>{r.label}</button></span>)}.
      </div>}
      {check.notes.map((n) => <div key={n} className="muted small">{n}</div>)}
    </section>}

    {result && <section aria-labelledby="near" className="card stack gap">
      <div className="title-row"><h2 id="near">Nearest items</h2>
        <span className="muted small">{result.exhaustive ? `Checked all ${fmt(result.vectors_compared)} texts` : `Approximate search (${fmt(result.vectors_compared)} compared)`} · {(result.embed_ms + result.retrieve_ms).toFixed(0)} ms</span></div>
      {result.hits.length === 0 && <p className="muted">No items to compare against.</p>}
      <ol className="hits">{result.hits.map((h) => <li key={h.item.item_id}>
        <div className="sim"><b>{h.similarity.toFixed(2)}</b><span className="muted small">similarity</span></div>
        <div className="stack">
          <div className="row wrap gap-s">
            <Link url={h.item.url} className="strong">{h.item.title ?? h.item.source_id}</Link>
            <span className={`chip role ${h.item.role}`}>{h.item.role}</span>
            {h.item.date && <span className="muted small">{year(h.item.date)}</span>}
          </div>
          <div className="muted small">{h.via_chunk !== null ? `Matched part ${h.via_chunk + 1}` : "Whole item"}</div>
        </div>
      </li>)}</ol>
      {!check && result.notes.map((n) => <div key={n} className="muted small">{n}</div>)}
      <div className="muted small">These are the nearest items in this collection only. Not finding something here is not proof it doesn’t exist.</div>
    </section>}
  </div>;
}
