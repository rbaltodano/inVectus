import { useEffect, useState, type CSSProperties } from "react";
import { api, fmt, year, KIND_LABEL, VERDICTS, type SavedBookmark, type DescribeResult, type GapDetail, type Verification } from "../api";
import { ErrorLine, useAction, type Ctx } from "../App";
import Link from "../Link";
import { stagger } from "../motion";

// Keys are the engine's term names (crates/gaps/src/scan.rs).
const TERM_LABEL: Record<string, string> = {
  "demand since last coverage (log)": "Demand since last coverage", "demand per coverage (log)": "Demand per coverage item",
  momentum: "Momentum", "share of demand after coverage": "Demand newer than the coverage", "recent coverage": "Recent coverage penalty",
  "similarity × (1 − bridge ratio) × (1 + momentum)": "Closeness with little in between",
};

/** Momentum is a log-linear yearly rate; show it as percent growth per year. */
const growth = (m: number) => { const pct = (Math.exp(m) - 1) * 100; return `${pct >= 0 ? "+" : ""}${pct.toFixed(0)}%`; };

export default function GapDetailView({ ctx, id }: { ctx: Ctx; id: number }) {
  const tref = ctx.tref!;
  const { project } = ctx;
  const [gap, setGap] = useState<GapDetail>();
  const [verdict, setVerdict] = useState("open");
  const [note, setNote] = useState("");
  const [bookmark, setBookmark] = useState<SavedBookmark>();
  const pin = useAction();
  const [saved, setSaved] = useState(false);
  const [t, setT] = useState(50);
  const [described, setDescribed] = useState<DescribeResult>();
  const [verification, setVerification] = useState<Verification>();
  const load = useAction();
  const save = useAction();
  const describe = useAction();
  const verify = useAction();

  useEffect(() => {
    load.run(async () => {
      const g = await api.gap(tref, id);
      setGap(g); setVerdict(g.verdict?.verdict ?? "open"); setNote(g.verdict?.note ?? "");
      const kept = await api.saved(tref);
      setBookmark(kept.bookmarks.find((b) => b.gap_id === id));
      ctx.setMarks({ region: g.region_a });
      setDescribed(undefined); setVerification(undefined);
    });
  }, [id, tref.collection, tref.space]);

  if (!gap) return <div className="page"><ErrorLine error={load.error} />{load.busy && <p className="muted">Loading…</p>}</div>;

  const bridge = gap.kind === "bridge";
  const [nameA, nameB] = bridge ? gap.label.split(/\s*↔\s*/) : [gap.label, ""];
  const maxContribution = Math.max(1e-9, ...gap.terms.map((x) => Math.abs(x.contribution)));
  const every = Math.ceil(gap.timeline.length / 12);
  const maxYear = Math.max(1, ...gap.timeline.map((y) => Math.max(y.demand, y.coverage)));
  const attempt = described && (described.accepted !== null ? described.attempts[described.accepted] : described.attempts[described.attempts.length - 1]);
  const offline = !project.network_enabled || project.offline_env;
  const outside = verification
    ? { summary: verification.summary, hits: verification.hits.slice(0, 8) }
    : gap.verification && { summary: gap.verification.summary, hits: gap.verification.closest.map((h) => ({ ...h, looks_like_match: false })) };

  const toggleBookmark = () => pin.run(async () => {
    if (bookmark) await api.removeBookmark(tref, bookmark.kind, bookmark.core_key);
    else await api.bookmark(tref, id);
    const kept = await api.saved(tref);
    setBookmark(kept.bookmarks.find((b) => b.gap_id === id));
  });
  const saveVerdict = () => save.run(async () => { const updated = await api.setVerdict(tref, id, verdict, note); setGap({ ...gap, verdict: updated.verdict }); await ctx.reloadGaps(); setSaved(true); setTimeout(() => setSaved(false), 2000); });

  return <div className="page wide">
    <div className="title-row">
      <h2 className="h1">{gap.label}</h2>
      <span className="tag">{KIND_LABEL[gap.kind]}</span>
      <span className="score-chip mono">{gap.score.toFixed(2)}</span>
    </div>
    <ErrorLine error={load.error} />

    <div className="detail-layout">
      <div className="stack gap">
        <section aria-labelledby="why" className="panel stack gap">
          <h2 id="why">Why it’s flagged</h2>
          <p className="muted">{gap.explanation}</p>
          <dl className="gap-stats">
            <div><dt>Demand</dt><dd>{fmt(gap.demand)}</dd></div>
            <div><dt>Coverage</dt><dd>{fmt(gap.coverage)}</dd></div>
            {gap.momentum !== null && <div><dt>Activity</dt><dd>{growth(gap.momentum)}<small>/yr</small></dd></div>}
            {gap.last_coverage && <div><dt>Last coverage</dt><dd>{gap.last_coverage}</dd></div>}
            {gap.bridge_items !== null && <div><dt>In between</dt><dd>{fmt(gap.bridge_items)}</dd></div>}
          </dl>
          <div className="breakdown-head"><span>Score breakdown</span><b>{gap.score.toFixed(2)}</b></div>
          <div className="terms" key={`terms-${gap.gap_id}`}>{gap.terms.filter((x) => x.weight !== 0).map((x, i) => <div key={x.name} className="term" style={{ "--i": i } as CSSProperties}>
            <span>{TERM_LABEL[x.name] ?? x.name}</span>
            <div className="meter thin" aria-hidden="true"><div className={x.contribution < 0 ? "neg" : ""} style={{ width: `${(Math.abs(x.contribution) / maxContribution) * 100}%` }} /></div>
            <b>{x.contribution >= 0 ? "+" : "−"}{Math.abs(x.contribution).toFixed(2)}</b>
          </div>)}</div>
        </section>

        {gap.timeline.length > 0 && <section aria-labelledby="tl" className="panel stack gap">
          <div className="row wrap"><h2 id="tl">Per year</h2><span className="key demand">Demand</span><span className="key coverage">Coverage</span></div>
          <div className="timeline" key={`timeline-${gap.gap_id}`} role="img" aria-label="Demand and coverage items per year"
            style={{ "--stagger": `${stagger(gap.timeline.length)}ms` } as CSSProperties}>
            {gap.timeline.map((y, i) => <div key={y.year} className="year" style={{ "--i": i } as CSSProperties} title={`${y.year}: ${y.demand} demand, ${y.coverage} coverage`}>
              <div className="bars"><div className="demand" style={{ height: `${(y.demand / maxYear) * 100}%` }} /><div className="coverage" style={{ height: `${(y.coverage / maxYear) * 100}%` }} /></div>
              <span>{i % every === 0 ? `'${String(y.year).slice(2)}` : "\u00a0"}</span>
            </div>)}
          </div>
        </section>}

        <section aria-labelledby="mid" className="panel stack gap">
          <h2 id="mid">{bridge ? "What would sit in between?" : "What would fill this gap?"}</h2>
          {bridge && <><div className="row">
            <span className="small clip w-160">{nameA}</span>
            <input type="range" min={0} max={100} value={t} className="grow" onChange={(e) => setT(Number(e.target.value))} aria-label="Position between the two regions" />
            <span className="small clip w-160 right">{nameB}</span>
          </div><div className="muted small center">t = {(t / 100).toFixed(2)}</div></>}
          <ErrorLine error={describe.error} />
          {attempt && <div className="quote stack gap-s">
            <b>{attempt.title}</b><span>{attempt.description}</span>
          </div>}
          {attempt && <div className="row wrap small">
            <span className={attempt.on_target.passed ? "ok-text" : "warn-text"}>{attempt.on_target.passed ? "✓ Lands on target" : "✗ Drifts off target"}</span>
            {attempt.novelty && <span className={attempt.novelty.passed ? "ok-text" : "warn-text"}>{attempt.novelty.passed ? "✓ Not already in the collection" : `✗ Already here: ${attempt.novelty.nearest_title ?? "a close item"}`}</span>}
            {described!.accepted === null && <span className="muted">No attempt passed both checks; showing the last one.</span>}
          </div>}
          {described?.notes.map((n) => <div key={n} className="muted small">{n}</div>)}
          <div className="row">
            <button type="button" className="btn" disabled={describe.busy} onClick={() => describe.run(async () => setDescribed(await api.describe(tref, id, bridge ? t / 100 : null)))}>
              {describe.busy ? "Writing… (the local model can take a minute)" : attempt ? "Regenerate" : "Describe with the local model"}</button>
          </div>
        </section>

        <section aria-labelledby="ni" className="panel stack gap">
          <h2 id="ni">Central items</h2>
          <ul className="plain source-list">{gap.core_items.map((i) => <li key={i.item_id}>
            <Link url={i.url}>{i.title ?? `item ${i.item_id}`}</Link>
            <span className="muted small"> · <span className={`tag role ${i.role}`}>{i.role}</span> {year(i.date)}</span>
          </li>)}</ul>
          <div className="row"><button type="button" className="btn" onClick={() => ctx.showRegion(gap.region_a)}>Show on map</button></div>
        </section>
      </div>

      <div className="stack gap">
        <section aria-labelledby="vd" className="panel stack gap">
          <div className="row wrap"><div className="label">Your decision</div>
            <button type="button" className="text-btn push" aria-pressed={!!bookmark} disabled={pin.busy} onClick={toggleBookmark}>{bookmark ? "Remove bookmark" : "Save gap"}</button>
          </div>
          <p className="dim small">{bookmark ? "Bookmarked in Saved." : "Save this gap to review later, with or without a decision."}</p>
          <ErrorLine error={pin.error} />
          <h2 id="vd">Decision and note</h2>
          <div className="muted small">Yours, kept apart from the engine’s score; it carries over when gaps are scanned again.</div>
          <div role="radiogroup" aria-labelledby="vd" className="stack gap-s">
            {VERDICTS.map(([v, label]) => <button key={v} type="button" role="radio" aria-checked={verdict === v} className="option" onClick={() => setVerdict(v)}>{label}</button>)}
          </div>
          <label>Note<textarea rows={4} value={note} onChange={(e) => setNote(e.target.value)} placeholder="What did you check? What would you do next?" /></label>
          <div className="row">
            <button type="button" className="btn primary" disabled={save.busy} onClick={saveVerdict}>{saved ? "Saved" : "Save decision"}</button>
            
          </div>
          <ErrorLine error={save.error} />
        </section>

        <section aria-labelledby="vf" className="panel stack gap">
          <h2 id="vf">Check outside sources</h2>
          <div className="row wrap">
            <button type="button" className="btn" disabled={verify.busy || offline} onClick={() => verify.run(async () => setVerification(await api.verify(tref, id)))}>
              {verify.busy ? "Searching…" : outside ? "Check PubMed and OpenAlex again" : "Check PubMed and OpenAlex"}</button>
          </div>
          {offline && <div className="note small">Offline: switch the network on (top right) to search outside sources. Results are cached in the project.</div>}
          <ErrorLine error={verify.error} />
          {outside && <>
            <div>{outside.summary}</div>
            <ul className="plain">{outside.hits.map((h) => <li key={`${h.source}${h.title}`}>
              <Link url={h.url}>{h.title}</Link>
              <span className="muted small"> · {h.source}{h.year ? ` ${h.year}` : ""} · {h.similarity.toFixed(2)}{h.looks_like_match ? " · looks like a match" : ""}</span>
            </li>)}</ul>
            {verification && <details><summary className="small">Queries sent</summary>
              <ul className="small muted">{verification.sources.map((s) => <li key={s.source}>{s.source}: <code>{s.query}</code> · {s.total !== null ? `${fmt(s.total)} matching` : ""}{s.cached ? " · cached" : ""}{s.error ? ` · ${s.error}` : ""}</li>)}</ul>
            </details>}
          </>}
          <div className="muted small">Read the titles yourself. Similarity alone misses things.</div>
        </section>
      </div>
    </div>
  </div>;
}
