// The menu beside the data circle. Pages stack: choosing a row replaces the menu with that
// page, and Back returns. The circle stays in view, so results and gaps show where they sit.
import { useEffect, useMemo, useRef, useState, Fragment, type FormEvent } from "react";
import { api, fmt, local, year, KIND_LABEL, verdictLabel, type CheckResult, type GapKind, type Role, type SearchResult, type Weights } from "../api";
import { ErrorLine, useAction, type Ctx, type Page } from "../App";
import Link from "../Link";
import Import from "./Import";
import Collections from "./Collections";
import GapDetailView from "./GapDetail";
import { assessmentCopy } from "../search-presentation";
import { gapCode } from "../DataCircle";
import LineArt from "../LineArt";
import { weightsKey } from "./Settings";
import { ArrowLeft } from "lucide-react";
import filterChevron from "../assets/figma/search-filter-chevron.svg?no-inline";
import resultDivider from "../assets/figma/search-result-divider.svg?no-inline";
import { MatrixLoader } from "../IndexingIndicator";

const pageKey = (p: Page) => p.kind === "gap" ? `gap:${p.id}` : p.kind === "item" ? `item:${p.point.item_id}` : p.kind;
const SWAP_OUT_MS = 150;

export default function Panel({ ctx, stack, active, tabId }: { ctx: Ctx; stack: Page[]; active: boolean; tabId: string }) {
  const page = stack[stack.length - 1];
  // Changing page fades the current content out, swaps it, then fades the new page in.
  const [shown, setShown] = useState(page);
  const [phase, setPhase] = useState<"in" | "out">("in");
  const scroller = useRef<HTMLElement>(null);
  useEffect(() => {
    if (pageKey(page) === pageKey(shown)) { setShown(page); return; }
    const swap = () => { setShown(page); setPhase("in"); if (scroller.current) scroller.current.scrollTop = 0; };
    if (!active || window.matchMedia("(prefers-reduced-motion: reduce)").matches) { swap(); return; }
    setPhase("out");
    const t = setTimeout(swap, SWAP_OUT_MS);
    return () => clearTimeout(t);
  }, [pageKey(page), active]);
  const searchPage = stack.find((p): p is Extract<Page, { kind: "search" }> => p.kind === "search");
  // Marks on the circle belong to the page that set them.
  useEffect(() => { if (page.kind !== "search" && page.kind !== "item" && page.kind !== "gap") ctx.setMarks({}); }, [page.kind]);
  return <aside ref={scroller} className={`menu ${shown.kind === "search" ? "search-menu" : ""}`} role="tabpanel" id={`panel-${tabId}`} aria-labelledby={`tab-${tabId}`} hidden={!active} tabIndex={0}>
    <div className={`page-swap ${phase}`}>
    {stack.length > 1 && shown.kind !== "search" && <button type="button" className="back" onClick={ctx.back}><ArrowLeft size={16} aria-hidden="true" />Back</button>}
    {searchPage && <div className="stack gap-s" hidden={shown.kind !== "search"}>
      <SearchPage key={`${ctx.tref?.collection}:${ctx.tref?.space}`} ctx={ctx} initial={searchPage.query} active={active && shown.kind === "search"} />
    </div>}
    {shown.kind === "import" && <Import ctx={ctx} initialPath={shown.path} onCancel={ctx.back} onDone={() => ctx.back()} />}
    {stack.some((p) => p.kind === "index") && <div className="stack gap-s" hidden={shown.kind !== "index"}><Collections ctx={ctx} focus={stack.find((p) => p.kind === "index")?.collection} /></div>}
    {stack.some((p) => p.kind === "gaps") && <div className="stack gap-s" hidden={shown.kind !== "gaps"}><GapsPage key={`${ctx.tref?.collection}:${ctx.tref?.space}`} ctx={ctx} /></div>}
    {shown.kind === "gap" && <GapDetailView ctx={ctx} id={shown.id} />}
    {shown.kind === "item" && <ItemPage ctx={ctx} page={shown} />}
    </div>
  </aside>;
}

function SearchPage({ ctx, initial, active }: { ctx: Ctx; initial?: string; active: boolean }) {
  const { table, tref, map, project } = ctx;
  const [query, setQuery] = useState(initial ?? "");
  const [scope, setScope] = useState<Role | "">("");
  const [check, setCheck] = useState<CheckResult>();
  const [plain, setPlain] = useState<SearchResult>();
  const { busy, error, run } = useAction();
  const points = useMemo(() => new Map((map?.points ?? []).map((p) => [p.item_id, p])), [map]);
  const coll = project.collections.find((c) => c.collection_id === table?.collection);
  const queryInput = useRef<HTMLInputElement>(null);
  const result = check?.search ?? plain;
  const [queryPos, setQueryPos] = useState<{ x: number; y: number; label: string }>();

  useEffect(() => {
    if (!active) return;
    const focusSearch = (e: KeyboardEvent) => {
      const target = e.target;
      if (e.key !== "/" || e.metaKey || e.ctrlKey || e.altKey || e.isComposing) return;
      if (target instanceof HTMLElement && (target.isContentEditable || target.closest("input, textarea, select, [contenteditable]"))) return;
      e.preventDefault(); queryInput.current?.focus();
    };
    window.addEventListener("keydown", focusSearch);
    return () => window.removeEventListener("keydown", focusSearch);
  }, [active]);

  useEffect(() => {
    if (initial !== undefined) { setQuery(initial); setCheck(undefined); setPlain(undefined); }
  }, [initial]);

  // Where the idea sits: between its two nearest regions (weighted by similarity), else the
  // mean of its nearest items on the map. Either way it is an estimate.
  useEffect(() => {
    if (!active) return;
    if (!result || !map) { ctx.setMarks({}); return; }
    const regions = new Map(map.regions.map((r) => [r.region_id, r]));
    const between = (check?.between ?? []).map((b) => ({ r: regions.get(b.region_id), w: Math.max(b.similarity, 0.01) })).filter((b) => b.r);
    let pos: { x: number; y: number; label: string } | undefined;
    if (between.length) {
      const w = between.reduce((s, b) => s + b.w, 0);
      pos = { x: between.reduce((s, b) => s + b.r!.x * b.w, 0) / w, y: between.reduce((s, b) => s + b.r!.y * b.w, 0) / w, label: "between its nearest regions" };
    } else {
      const near = result.hits.slice(0, 3).map((h) => points.get(h.item.item_id)).filter((p) => p !== undefined);
      if (near.length) pos = { x: near.reduce((s, p) => s + p!.x, 0) / near.length, y: near.reduce((s, p) => s + p!.y, 0) / near.length, label: "near its closest items" };
    }
    setQueryPos(pos);
    const top = result.hits[0];
    ctx.setMarks({ query: pos, items: top ? [top.item.item_id] : [], labels: top ? { [top.item.item_id]: top.similarity.toFixed(2) } : {} });
  }, [result, map, active]);

  if (!table || !tref) return <div className="menu-card dim small">Search needs an indexed collection. Add a dataset first.</div>;
  const search = (nextScope = scope) => {
    if (!query.trim() || busy) return;
    const role = nextScope || null;
    run(async () => {
      setCheck(undefined); setPlain(undefined);
      if (table.calibration) setCheck(await api.check(tref, query, role, false));
      else setPlain(await api.search(tref, query, role, true));
    });
  };
  const go = (e: FormEvent) => { e.preventDefault(); search(); };
  const assessment = check ? assessmentCopy(check, scope || null) : undefined;
  return <div className="search-page" aria-busy={busy}>
    <form role="search" className="search-command" onSubmit={go}>
      <label htmlFor="panel-q" className="sr-only">Describe an idea</label>
      <input ref={queryInput} id="panel-q" value={query} onChange={(e) => setQuery(e.target.value)} placeholder="/ to Search" />
      {busy && <span role="status" aria-label="Searching"><MatrixLoader /></span>}
      <label className="search-scope">
        <span className="sr-only">Compare with</span>
        <select value={scope} disabled={busy} onChange={(e) => {
          const next = e.target.value as Role | "";
          setScope(next);
          if (result) search(next);
        }}>
          <option value="">All items</option>
          <option value="coverage" disabled={!coll?.by_role.coverage}>Coverage</option>
          <option value="demand" disabled={!coll?.by_role.demand}>Demand</option>
        </select>
        <img src={filterChevron} alt="" aria-hidden="true" />
      </label>
    </form>
    <div className="search-content">
      <ErrorLine error={error} />
      {!result && !busy && <div className="empty-state">
        <span className="dim">Describe an idea. inVectus compares it with every item in “{table.collection}” and says whether something close already exists there.</span></div>}
      {assessment && <div className="search-assessment" aria-live="polite">
        <span className={`verdict ${check!.calibration.stale || !check!.search.hits.length ? "unassessed" : check!.verdict}`}>{assessment.label}</span>
        <span>{assessment.message}</span>
      </div>}
      {!table.calibration && result && <p className="dim small">Not calibrated · similarity rankings only</p>}
      {result && <section className="search-rankings" aria-labelledby="rankings-title">
        <h2 id="rankings-title">Similarity rankings</h2>
        <div className="search-results" role="group" aria-label="Search results">
          {result.hits.map((h, i) => <Fragment key={h.item.item_id}>
            {i > 0 && <img className="search-result-divider" src={resultDivider} alt="" aria-hidden="true" />}
            <button type="button" className="search-hit"
              onMouseEnter={() => ctx.setMarks({ query: queryPos, items: [h.item.item_id], labels: { [h.item.item_id]: h.similarity.toFixed(2) } })}
              onFocus={() => ctx.setMarks({ query: queryPos, items: [h.item.item_id], labels: { [h.item.item_id]: h.similarity.toFixed(2) } })}
              onClick={() => { const p = points.get(h.item.item_id); if (p) ctx.push({ kind: "item", point: p }); }}>
              <span className={`score ${i === 0 ? "top" : ""}`} aria-label={`similarity ${h.similarity.toFixed(2)}`}>{h.similarity.toFixed(2)}</span>
              <span className="rank-title">{h.item.title ?? h.item.source_id}
                <span className="dim">{[h.item.role.charAt(0).toUpperCase() + h.item.role.slice(1), year(h.item.date)].filter(Boolean).join(" – ")}</span>
              </span>
            </button>
          </Fragment>)}
          {result.hits.length === 0 && <p className="dim small">No items to compare in this selection.</p>}
        </div>
        <details className="assessment-details">
          <summary>Assessment details</summary>
          <div className="stack gap-s small dim">
            <span>Cosine similarity, not a probability · “{table.collection}” only · {result.exhaustive ? "every item checked" : "approximate search"}</span>
            {check && <>
              <span>{Math.round(check.p_exists * 100)}% estimated chance the idea is already in this collection.</span>
              <span>{check.calibration.quality}</span>
              <span>{check.summary}</span>
              <span>{check.next_step}</span>
              {check.notes.map((note, i) => <span key={i}>{note}</span>)}
            </>}
            {result.notes.map((note, i) => <span key={i}>{note}</span>)}
          </div>
        </details>
      </section>}
    </div>
  </div>;
}

function ItemPage({ ctx, page }: { ctx: Ctx; page: Extract<Page, { kind: "item" }> }) {
  const p = page.point;
  const region = ctx.map?.regions.find((r) => r.region_id === p.fine_id);
  useEffect(() => { ctx.setMarks({ items: [p.item_id], region: p.fine_id }); }, [p.item_id]);
  return <div className="menu-card stack gap">
    <div className="card-title">{p.title ?? `Item ${p.item_id}`}</div>
    <dl className="facts">
      <dt>Role</dt><dd>{p.role}</dd>
      {p.date && <><dt>Date</dt><dd>{p.date}</dd></>}
      <dt>Region</dt><dd>{region ? region.llm_label ?? region.label : "—"}{p.noise ? " (loosely attached)" : ""}</dd>
    </dl>
    {p.url && <Link url={p.url}>Open the source</Link>}
    <button type="button" className="menu-row" onClick={() => ctx.push({ kind: "search", query: p.title ?? "" })}>Search for similar items</button>
  </div>;
}

function GapsPage({ ctx }: { ctx: Ctx }) {
  const { table, tref, project, gaps } = ctx;
  const [kind, setKind] = useState<GapKind | "">("");
  const [msg, setMsg] = useState<string>();
  const { busy, error, run } = useAction();
  if (!table || !tref) return <div className="menu-card dim small">Gaps need an indexed, mapped collection.</div>;
  if (!table.regions) return <div className="menu-card stack gap-s"><b>No map yet</b><span className="dim small">Gaps are found within and between regions. Run mapping from Collections.</span>
    <button type="button" className="menu-row" onClick={() => ctx.push({ kind: "index", collection: table.collection })}>Open Collections</button></div>;
  const scan = () => run(async () => {
    await api.scan(tref, local.get<Weights | null>(weightsKey(project.root), null));
    await ctx.refresh(); await ctx.reloadGaps();
  });
  const shown = gaps.filter((g) => !kind || g.kind === kind);
  const offline = !project.network_enabled || project.offline_env;
  const verifyTop = () => run(async () => {
    const top = shown.slice(0, 5);
    for (const [i, g] of top.entries()) { setMsg(`Checking ${i + 1} of ${top.length}…`); await api.verify(tref, g.gap_id); }
    setMsg(undefined); await ctx.reloadGaps();
  });
  return <>
    <div className="row wrap">
      <span className="section">Gaps{table.regions.scanned ? ` · ${fmt(gaps.length)}` : ""}</span>
      <button type="button" className="text-btn push" disabled={busy} onClick={scan}>{table.regions.scanned ? "Scan again" : "Scan for gaps"}</button>
    </div>
    <div role="radiogroup" aria-label="Gap type" className="segmented">
      {(["", "coverage", "stale", "bridge"] as (GapKind | "")[]).map((k) => <button key={k || "all"} type="button" role="radio" aria-checked={kind === k} onClick={() => setKind(k)}>{k ? KIND_LABEL[k] : "All"}</button>)}
    </div>
    <ErrorLine error={error} />
    {!table.regions.scanned && <div className="empty-state"><LineArt kind="venn" /><span className="dim">Not scanned yet. Scanning runs offline in seconds and keeps your decisions.</span></div>}
    {shown.length > 0 && <div className="result-list" role="group" aria-label="Gaps">
    {shown.map((g) => <button key={g.gap_id} type="button" className="menu-row rank" onClick={() => ctx.push({ kind: "gap", id: g.gap_id })}>
      <span className="score">{g.score.toFixed(2)}</span>
      <span className="rank-title">{g.label}<span className="dim">{gapCode(ctx.gaps.indexOf(g))} · {KIND_LABEL[g.kind]} · {verdictLabel(g.verdict?.verdict)}{g.verification ? ` · ${g.verification.matches ? `${g.verification.matches} outside matches` : "no outside match"}` : ""}</span></span>
    </button>)}
    </div>}
    {shown.length > 0 && <div className="row wrap small">
      <button type="button" className="text-btn" disabled={busy || offline} title={offline ? "Switch the network on to check PubMed and OpenAlex" : undefined} onClick={verifyTop}>{msg ?? "Verify top 5"}</button>
      <button type="button" className="text-btn" disabled={busy} onClick={() => run(async () => setMsg(`Saved to ${await api.exportGaps(tref, "markdown")}`))}>Export Markdown</button>
      <button type="button" className="text-btn" disabled={busy} onClick={() => run(async () => setMsg(`Saved to ${await api.exportGaps(tref, "csv")}`))}>Export CSV</button>
    </div>}
    {msg && !busy && <p className="dim small">{msg}</p>}
  </>;
}
