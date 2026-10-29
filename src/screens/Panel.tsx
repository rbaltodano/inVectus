// The menu beside the data circle. Pages stack: choosing a row replaces the menu with that
// page, and Back returns. The circle stays in view, so results and gaps show where they sit.
import { useEffect, useMemo, useState, type FormEvent } from "react";
import { api, fmt, local, year, KIND_LABEL, verdictLabel, type CheckResult, type GapKind, type JobInfo, type Role, type SearchResult, type Weights } from "../api";
import { ErrorLine, useAction, type Ctx, type Page } from "../App";
import { isRunning } from "../jobs";
import Link from "../Link";
import Import from "./Import";
import Collections from "./Collections";
import GapDetailView from "./GapDetail";
import { weightsKey } from "./Settings";
import backIcon from "../assets/icons/back.svg";
import searchIcon from "../assets/icons/search-panel.svg";

export default function Panel({ ctx, stack, job, active, tabId }: { ctx: Ctx; stack: Page[]; job?: JobInfo; active: boolean; tabId: string }) {
  const page = stack[stack.length - 1];
  const searchPage = stack.find((p): p is Extract<Page, { kind: "search" }> => p.kind === "search");
  // Marks on the circle belong to the page that set them.
  useEffect(() => { if (page.kind !== "search" && page.kind !== "item" && page.kind !== "gap") ctx.setMarks({}); }, [page.kind]);
  return <aside className="menu" role="tabpanel" id={`panel-${tabId}`} aria-labelledby={`tab-${tabId}`} hidden={!active} tabIndex={0}>
    {stack.length > 1 && <button type="button" className="back" onClick={ctx.back}><img src={backIcon} alt="" aria-hidden="true" />Back</button>}
    {page.kind === "home" && <><Home ctx={ctx} job={job} /><MapPage ctx={ctx} /></>}
    {searchPage && <div className="stack gap-s" hidden={page.kind !== "search"}>
      <SearchPage key={`${ctx.tref?.collection}:${ctx.tref?.space}`} ctx={ctx} initial={searchPage.query} active={active && page.kind === "search"} />
    </div>}
    {page.kind === "import" && <Import ctx={ctx} initialPath={page.path} onCancel={ctx.back} onDone={() => ctx.back()} />}
    {stack.some((p) => p.kind === "index") && <div className="stack gap-s" hidden={page.kind !== "index"}><Collections ctx={ctx} focus={stack.find((p) => p.kind === "index")?.collection} /></div>}
    {page.kind === "map" && <MapPage ctx={ctx} />}
    {stack.some((p) => p.kind === "gaps") && <div className="stack gap-s" hidden={page.kind !== "gaps"}><GapsPage key={`${ctx.tref?.collection}:${ctx.tref?.space}`} ctx={ctx} /></div>}
    {page.kind === "gap" && <GapDetailView ctx={ctx} id={page.id} />}
    {page.kind === "item" && <ItemPage ctx={ctx} page={page} />}
  </aside>;
}

function Home({ ctx, job }: { ctx: Ctx; job?: JobInfo }) {
  const { project, table } = ctx;
  const items = project.collections.reduce((n, c) => n + c.items, 0);
  const ready = table?.index_status === "indexed" || table?.index_status === "imported";
  const pct = job?.total ? Math.round((job.done / job.total) * 100) : 0;
  return <>
    <div className="menu-card project-card">
      <div><div className="card-title">{project.name}</div><div className="path">{project.root.replace(/^\/Users\/[^/]+/, "~")}</div></div>
      <div className="card-foot">
        <span>{fmt(items)} items</span><span>/</span>
        <span className={`square ${ready ? "on" : ""}`} title={ready ? "Indexed" : "Not indexed yet"} aria-label={ready ? "Indexed" : "Not indexed yet"} />
      </div>
    </div>
    {job && <div className="menu-card stack gap-s" aria-live="polite">
      <span className="card-title">{job.label}</span>
      <span className="dim small">{isRunning(job) ? `${job.phase ?? "Starting"}${job.total ? ` · ${pct}%` : ""}` : job.state === "completed" ? "Done" : job.state === "cancelled" ? "Cancelled" : `Failed: ${job.error}`}</span>
      {isRunning(job) && <button type="button" className="text-btn start" onClick={() => api.cancel()}>Cancel</button>}
      {job.state === "failed" && job.collection && <button type="button" className="text-btn start" onClick={() => ctx.push({ kind: "index", collection: job.collection! })}>Retry in Collections</button>}
    </div>}
  </>;
}

const VERDICT = { open: "Open", near: "Near", exists: "Exists" };

function SearchPage({ ctx, initial, active }: { ctx: Ctx; initial?: string; active: boolean }) {
  const { table, tref, map, project } = ctx;
  const [query, setQuery] = useState(initial ?? "");
  const [scope, setScope] = useState<Role | "">("coverage");
  const [check, setCheck] = useState<CheckResult>();
  const [plain, setPlain] = useState<SearchResult>();
  const { busy, error, run } = useAction();
  const points = useMemo(() => new Map((map?.points ?? []).map((p) => [p.item_id, p])), [map]);
  const coll = project.collections.find((c) => c.collection_id === table?.collection);
  const role = scope && (coll?.by_role[scope] ?? 0) > 0 ? scope : null;
  const result = check?.search ?? plain;
  const [queryPos, setQueryPos] = useState<{ x: number; y: number; label: string }>();

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
    ctx.setMarks({ query: pos, items: result.hits.slice(0, 1).map((h) => h.item.item_id) });
  }, [result, map, active]);

  if (!table || !tref) return <div className="menu-card dim small">Search needs an indexed collection. Add a dataset first.</div>;
  const go = (e?: FormEvent) => {
    e?.preventDefault();
    if (!query.trim()) return;
    run(async () => {
      if (table.calibration) { setCheck(await api.check(tref, query, role, false)); setPlain(undefined); }
      else { setPlain(await api.search(tref, query, role, true)); setCheck(undefined); }
    });
  };
  const pct = check ? Math.round(check.p_exists * 100) : 0;
  return <>
    <form role="search" className="menu-row search-row" onSubmit={go}>
      <img src={searchIcon} alt="" aria-hidden="true" />
      <label htmlFor="panel-q" className="sr-only">Describe an idea</label>
      <input id="panel-q" autoFocus value={query} onChange={(e) => setQuery(e.target.value)} placeholder="DESCRIBE AN IDEA" />
      {busy && <span className="dim small">…</span>}
    </form>
    <div className="row wrap small">
      <label className="inline dim">Compare with
        <select value={scope} onChange={(e) => setScope(e.target.value as Role | "")}>
          <option value="coverage">Coverage</option><option value="">Everything</option><option value="demand">Demand</option>
        </select></label>
    </div>
    <ErrorLine error={error} />
    {!table.calibration && result && <div className="menu-card dim small">Not calibrated, so there is no verdict yet; scores below rank similarity only.</div>}
    {check && <div className="menu-card stack gap-s" aria-live="polite">
      <div className="row"><span className={`verdict ${check.verdict}`}>{VERDICT[check.verdict]}</span><span className="big-num">{pct}%</span><span className="dim small">chance it is already in this collection</span></div>
      <span className="small">{check.summary}</span>
      <span className="dim small">{check.calibration.quality}</span>
    </div>}
    {result && <>
      <div className="section">Rankings</div>
      <div className="dim small">Cosine similarity (not a probability), against “{table.collection}” only · {result.exhaustive ? "every item checked" : "approximate"}</div>
      {result.hits.map((h, i) => <button key={h.item.item_id} type="button" className="menu-row rank"
        onMouseEnter={() => ctx.setMarks({ query: queryPos, items: [h.item.item_id] })}
        onFocus={() => ctx.setMarks({ query: queryPos, items: [h.item.item_id] })}
        onClick={() => { const p = points.get(h.item.item_id); if (p) ctx.push({ kind: "item", point: p }); }}>
        <span className={`score ${i === 0 ? "top" : ""}`} aria-label={`similarity ${h.similarity.toFixed(2)}`}>{h.similarity.toFixed(2)}</span>
        <span className="rank-title">{h.item.title ?? h.item.source_id}<span className="dim">{[h.item.role, year(h.item.date)].filter(Boolean).join(" · ")}</span></span>
      </button>)}
    </>}
  </>;
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

function MapPage({ ctx }: { ctx: Ctx }) {
  const { map, view, table } = ctx;
  const toggle = (k: "showGaps" | "showLabels" | "showOutlines", label: string, hint: string) =>
    <label className="menu-row toggle"><input type="checkbox" checked={view[k]} onChange={(e) => view.set(k, e.target.checked)} /><span>{label}</span><span className="dim small push">{hint}</span></label>;
  return <>
    <div className="menu-card stack gap-s">
      <div className="card-title">Map</div>
      <span className="dim small">{map ? `${fmt(map.points.length)} items in ${fmt(map.regions.filter((r) => r.level === "fine").length)} regions · ${table?.model_key}` : "No map yet: it is drawn after indexing and mapping."}</span>
      <span className="dim small">Squares that overlap merge into one; zoom in to separate them. Scroll to pan, pinch or Command-scroll to zoom, drag to pan, and click a merged square to zoom to it.</span>
    </div>
    {toggle("showGaps", "Gap marks", "rings")}
    {toggle("showLabels", "Region names", "largest 16")}
    {toggle("showOutlines", "Region outlines", "else on hover")}
    <button type="button" className="menu-row" onClick={view.reset}>Reset view</button>
    {map && <p className="dim small">{map.note}</p>}
  </>;
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
    {!table.regions.scanned && <div className="menu-card dim small">Not scanned yet. Scanning runs offline in seconds and keeps your decisions.</div>}
    {shown.map((g) => <button key={g.gap_id} type="button" className="menu-row rank" onClick={() => ctx.push({ kind: "gap", id: g.gap_id })}>
      <span className="score">{g.score.toFixed(2)}</span>
      <span className="rank-title">{g.label}<span className="dim">{KIND_LABEL[g.kind]} · {verdictLabel(g.verdict?.verdict)}{g.verification ? ` · ${g.verification.matches ? `${g.verification.matches} outside matches` : "no outside match"}` : ""}</span></span>
    </button>)}
    {shown.length > 0 && <div className="row wrap small">
      <button type="button" className="text-btn" disabled={busy || offline} title={offline ? "Switch the network on to check PubMed and OpenAlex" : undefined} onClick={verifyTop}>{msg ?? "Verify top 5"}</button>
      <button type="button" className="text-btn" disabled={busy} onClick={() => run(async () => setMsg(`Saved to ${await api.exportGaps(tref, "markdown")}`))}>Export Markdown</button>
      <button type="button" className="text-btn" disabled={busy} onClick={() => run(async () => setMsg(`Saved to ${await api.exportGaps(tref, "csv")}`))}>Export CSV</button>
    </div>}
    {msg && !busy && <p className="dim small">{msg}</p>}
  </>;
}
