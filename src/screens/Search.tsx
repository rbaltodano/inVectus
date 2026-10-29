import { useState, type FormEvent } from "react";
import { api, fmt, STATUS_LABEL, type CheckResult, type Role, type SearchResult } from "../api";
import { ErrorLine, Needs, useAction, type Ctx } from "../App";
import Link from "../Link";

const VERDICT = {
  open: { label: "Open", line: "Nothing in this collection looks like the same thing." },
  near: { label: "Near", line: "Related items exist here, but none is clearly the same thing." },
  exists: { label: "Exists", line: "Something very close is already in this collection." },
};

export default function Search({ ctx, initial }: { ctx: Ctx; initial?: string }) {
  const { table, tref, project } = ctx;
  const [query, setQuery] = useState(initial ?? "");
  const [scope, setScope] = useState<Role | "">("coverage");
  const [fast, setFast] = useState(false);
  const [check, setCheck] = useState<CheckResult>();
  const [plain, setPlain] = useState<SearchResult>();
  const [asked, setAsked] = useState<string>();
  const { busy, error, run } = useAction();

  if (!table || !tref) return <div className="page"><Needs title="Nothing to search yet"
    body="Search compares your idea with every item's vector, so a collection needs an index first. Indexing starts by itself after an import."
    action="Go to Collections" onAction={() => ctx.go({ view: "project", tab: "collections" })} /></div>;

  const coll = project.collections.find((c) => c.collection_id === table.collection);
  const texts = coll ? coll.items + coll.chunks : 0;
  const calibrated = !!table.calibration;
  const hasRole = (r: Role) => (coll?.by_role[r] ?? 0) > 0;
  const role = scope && hasRole(scope) ? scope : null;
  const go = (e?: FormEvent) => {
    e?.preventDefault();
    if (!query.trim()) return;
    run(async () => {
      setAsked(query.trim());
      if (calibrated) { setCheck(await api.check(tref, query, role, fast)); setPlain(undefined); }
      else { setPlain(await api.search(tref, query, role, !fast)); setCheck(undefined); }
    });
  };
  const result = check?.search ?? plain;
  const v = check && VERDICT[check.verdict];
  const pct = check ? Math.round(check.p_exists * 100) : 0;
  const partial = table.origin === "embedded" && table.index_status !== "indexed";

  return <div className="page">
    <form role="search" className="panel stack gap" onSubmit={go}>
      <label htmlFor="q" className="h2">Does this already exist?</label>
      <div className="row">
        <input id="q" type="search" className="grow big-input" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="Describe the idea in a sentence or two" />
        <button type="submit" className="btn primary" disabled={!query.trim() || busy}>{busy ? "Checking…" : "Search"}</button>
      </div>
      <div className="row wrap small">
        <span className="label">In</span><span className="mono">{table.collection} · {table.model_key}</span>
        <label className="inline push">Compare against
          <select value={scope} onChange={(e) => setScope(e.target.value as Role | "")}>
            <option value="coverage">Coverage only</option><option value="">Everything</option><option value="demand">Demand only</option>
          </select></label>
        <div role="radiogroup" aria-label="Search mode" className="segmented">
          <button type="button" role="radio" aria-checked={!fast} onClick={() => setFast(false)}>Check every item</button>
          <button type="button" role="radio" aria-checked={fast} onClick={() => setFast(true)}>Fast (approximate)</button>
        </div>
      </div>
      {scope && !hasRole(scope) && <div className="muted small">No {scope} items in this collection, so this compares against everything.</div>}
      {partial && <div className="note small">This index is “{STATUS_LABEL[table.index_status].toLowerCase()}”: {fmt(table.vectors)} of {fmt(texts)} texts have vectors, so results may miss items.</div>}
    </form>
    <ErrorLine error={error} />

    {!calibrated && <div className="panel row wrap">
      <span className="grow"><b>No assessment yet.</b> <span className="muted">This index is not calibrated, so results rank similar items but cannot say whether your idea exists.</span></span>
      <button type="button" className="btn" onClick={() => ctx.go({ view: "project", tab: "collections", collection: table.collection })}>Calibrate in Collections</button>
    </div>}

    {check && v && <section aria-labelledby="verdict" className="panel stack gap assessment">
      <div className="label">Assessment · {table.collection}</div>
      <div className="row wrap">
        <span id="verdict" className={`verdict ${check.verdict}`}>{v.label}</span>
        <span className="strong">{check.summary}</span>
      </div>
      <div className="gauge-row">
        <Gauge pct={pct} />
        <div className="stack gap-s grow">
          <div className="mono small">P(exists in this collection) = {check.p_exists.toFixed(2)}</div>
          <div className="small">{check.next_step}</div>
          <div className="muted small">{check.calibration.quality}{check.calibration.stale ? " The collection changed since calibration; recalibrate for current numbers." : ""}</div>
        </div>
      </div>
      {check.between.length > 0 && <div className="small">Sits {check.between.length === 2 ? "between " : "in "}
        {check.between.map((r, i) => <span key={r.region_id}>{i > 0 && " and "}<button type="button" className="link" onClick={() => ctx.go({ view: "project", tab: "map", region: r.region_id })}>{r.label}</button></span>)} (regions of this collection).
      </div>}
      {check.notes.map((n) => <div key={n} className="muted small">{n}</div>)}
    </section>}

    {result && <section aria-labelledby="near" className="panel stack gap">
      <div className="title-row"><h2 id="near">Nearest items</h2>
        <span className="label">{result.exhaustive ? `exhaustive · all ${fmt(result.vectors_compared)} texts` : `approximate · ${fmt(result.vectors_compared)} compared`} · {(result.embed_ms + result.retrieve_ms).toFixed(0)} ms</span></div>
      <p className="muted small">Similarity is cosine similarity between your text and each item (−1 to 1). It ranks items; it is not a probability. Results come from “{table.collection}” only{asked ? ` for “${asked}”` : ""}.</p>
      {result.hits.length === 0 && <p className="muted">No items to compare against.</p>}
      <ol className="hits">{result.hits.map((h) => <li key={h.item.item_id}>
        <div className="sim"><span className="label">Similarity</span><b className="mono">{h.similarity.toFixed(3)}</b></div>
        <div className="stack gap-s grow">
          <Link url={h.item.url} className="strong">{h.item.title ?? h.item.source_id}</Link>
          <div className="row wrap small muted mono">
            <span className={`tag role ${h.item.role}`}>{h.item.role}</span>
            {h.item.date && <span>{h.item.date}</span>}
            <span>id {h.item.source_id}</span>
            {h.via_chunk !== null && <span>matched chunk {h.via_chunk + 1}</span>}
          </div>
        </div>
      </li>)}</ol>
      {!check && result.notes.map((n) => <div key={n} className="muted small">{n}</div>)}
      <div className="muted small">Not finding something here is not proof it does not exist elsewhere.</div>
    </section>}
  </div>;
}

/** A dial for P(exists): the arc length is the calibrated value, ticks at the verdict lines. */
function Gauge({ pct }: { pct: number }) {
  const r = 34, c = 2 * Math.PI * r;
  return <svg className="gauge" width="88" height="88" viewBox="0 0 88 88" role="img" aria-label={`P(exists) ${pct}%`}>
    <circle cx="44" cy="44" r={r} className="gauge-track" />
    <circle cx="44" cy="44" r={r} className="gauge-value" strokeDasharray={`${(pct / 100) * c} ${c}`} transform="rotate(-90 44 44)" />
    {[30, 70].map((t) => { const a = (t / 100) * 2 * Math.PI - Math.PI / 2; return <line key={t} x1={44 + Math.cos(a) * 28} y1={44 + Math.sin(a) * 28} x2={44 + Math.cos(a) * 40} y2={44 + Math.sin(a) * 40} className="gauge-tick" />; })}
    <text x="44" y="49" textAnchor="middle" className="gauge-text">{pct}%</text>
  </svg>;
}
