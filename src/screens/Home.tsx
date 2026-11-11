// Home: the last project as a slice of its data circle with what is ready, its top gaps, and the
// recent projects. Map positions are for display only (SPEC §6.5): nothing here decides a gap.
import { useEffect, useMemo, useRef, useState, type CSSProperties } from "react";
import { ago } from "../ago";
import { api, fmt, inTauri, KIND_LABEL, local, type GapView, type JobInfo, type MapData, type ProjectInfo } from "../api";
import { defaultTable, type Page } from "../App";
import { gapCode } from "../DataCircle";
import { isRunning } from "../jobs";
import LineArt from "../LineArt";
import { springAt, SPRING_MS, stagger } from "../motion";
import { canvasColors, useTheme } from "../theme";
import type { Recent } from "./Projects";
import "./Home.css";

interface Props {
  project?: ProjectInfo;
  recent: Recent[];
  job?: JobInfo;
  loading: boolean;
  onEnter: (first?: Page) => void;
  onPick: (r: Recent) => void;
  onNew: () => void;
  onOpen: () => void;
  onProjects: () => void;
}

const TOP_GAPS = 5;
const ROW_DELAY = 120;   // each gap row starts this long after the one above
const ROW_START = 180;
const SPARK_YEARS = 37;
const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;
const tilde = (p: string) => p.replace(/^\/Users\/[^/]+/, "~");

export default function Home({ project, recent, job, loading, onEnter, onPick, onNew, onOpen, onProjects }: Props) {
  const table = project ? defaultTable(project, local.get<string | undefined>(`invectus.scope.${project.root}`, undefined)) : undefined;
  const [map, setMap] = useState<MapData>();
  const [gaps, setGaps] = useState<GapView[]>();
  const [error, setError] = useState<string>();
  const runId = table?.regions?.run_id;
  const scanned = !!table?.regions?.scanned;

  useEffect(() => {
    setMap(undefined); setGaps(undefined); setError(undefined);
    if (!inTauri || !project || !table?.regions) return;
    let cancelled = false;
    const tref = { project: project.root, collection: table.collection, space: table.space_id };
    api.map(tref).then((m) => { if (!cancelled) setMap(m); }).catch((e) => { if (!cancelled) setError(String(e)); });
    if (scanned) api.gaps(tref).then((g) => { if (!cancelled) setGaps(g.gaps); }).catch((e) => { if (!cancelled) setError(String(e)); });
    else setGaps([]);
    return () => { cancelled = true; };
  }, [project?.root, table?.collection, table?.space_id, runId, scanned]);

  const today = new Date().toLocaleDateString(undefined, { weekday: "long", day: "numeric", month: "long" });
  const entry = project && recent.find((r) => r.root === project.root);

  return <div className="page home-page">
    <div className="page-head">
      <div><p className="eyebrow">{today}</p><h1>Home</h1></div>
    </div>

    {!project && !loading && <div className="empty-state"><LineArt kind="orbit" />
      <span className="dim">No projects yet. Open an existing .invectus folder, or create one.</span>
      <div className="row"><button type="button" className="btn" onClick={onOpen}>Open project…</button>
        <button type="button" className="btn primary" onClick={onNew}>New project</button></div>
    </div>}

    {(project || loading) && <section className="home-card home-continue" aria-label="Continue">
      {map && <MapSlice map={map} gaps={gaps ?? []} onGap={(id) => onEnter({ kind: "gap", id })} />}
      {project && !table?.regions && <p className="home-slice-note dim">No map yet. Open the project to build one.</p>}
      {(loading || (table?.regions && !map && !error)) && <p className="home-slice-note dim">Loading map…</p>}
      <div className="home-continue-text">
        <p className="eyebrow">Continue{entry ? ` · opened ${ago(entry.opened)}` : ""}</p>
        {project && <>
          <h2 className="home-project-name">{project.name}</h2>
          <div className="path">{tilde(project.root)}</div>
          <dl className="home-facts">
            <div><dt>Items</dt><dd>{fmt(project.collections.reduce((n, c) => n + c.items, 0))}</dd></div>
            <div><dt>Regions</dt><dd>{map ? fmt(map.regions.filter((r) => r.level === "fine").length) : "—"}</dd></div>
            <div><dt>Gaps</dt><dd>{gaps ? fmt(gaps.length) : "—"}</dd></div>
          </dl>
          <ul className="home-ready">
            <Ready ok={project.collections.length > 0} label={project.collections.length ? `Imported · ${project.collections.length} collection${project.collections.length === 1 ? "" : "s"}` : "Nothing imported"} />
            <Ready ok={!!table} label={table ? `Vectors · ${table.model_key}${table.origin === "imported" ? " (imported)" : ""}` : "Not indexed"} />
            <Ready ok={!!table?.regions} label={table?.regions ? "Mapped" : "No map yet"} />
            <Ready ok={!!table?.calibration} label={table?.calibration ? `Calibrated${table.calibration.holdout_auc ? ` · AUC ${table.calibration.holdout_auc.toFixed(2)}` : ""}` : "Not calibrated"} />
          </ul>
          {error && <p className="err small">{error}</p>}
          <div className="row home-continue-actions">
            <button type="button" className="btn primary" onClick={() => onEnter()}>Open project</button>
            {scanned && <button type="button" className="btn" onClick={() => onEnter({ kind: "gaps" })}>All gaps</button>}
          </div>
        </>}
      </div>
      {map && <div className="home-legend" aria-hidden="true"><span><b className="home-legend-square" />Items</span><span><b className="home-legend-ring" />Gap</span></div>}
    </section>}

    {gaps && gaps.length > 0 && <TopGaps gaps={gaps} onGap={(id) => onEnter({ kind: "gap", id })} onAll={() => onEnter({ kind: "gaps" })} />}

    {recent.length > 0 && <section className="home-card" aria-label="Recent projects">
      <div className="home-card-head"><h2>Recent projects</h2><button type="button" className="text-btn" onClick={onProjects}>All projects ↗</button></div>
      <div className="home-table" role="table">
        <div className="home-table-head" role="row">
          <span role="columnheader">Project</span><span role="columnheader" className="num">Items</span>
          <span role="columnheader" className="home-col-models">Models</span><span role="columnheader">Status</span>
          <span role="columnheader" className="num">Opened</span>
        </div>
        {recent.slice(0, 5).map((r) => {
          const running = job && job.project === r.root && isRunning(job);
          const status = running ? job!.phase ?? job!.label : project?.root === r.root ? "Open" : r.models.length ? "Indexed" : "Not indexed";
          const tone = running ? "busy" : r.models.length ? "ok" : "";
          return <button key={r.root} type="button" role="row" className="home-table-row" onClick={() => onPick(r)}>
            <span role="cell" className="home-table-name">{r.name}</span>
            <span role="cell" className="num">{r.items ? fmt(r.items) : "—"}</span>
            <span role="cell" className="home-col-models models">{r.models.join(" · ") || "—"}</span>
            <span role="cell" className={`home-status ${tone}`}><i aria-hidden="true" />{status}</span>
            <span role="cell" className="num dim">{ago(r.opened)}</span>
          </button>;
        })}
      </div>
    </section>}
  </div>;
}

function Ready({ ok, label }: { ok: boolean; label: string }) {
  return <li className={ok ? "ok" : "todo"}><span aria-hidden="true">{ok ? "✓" : "–"}</span>{label}</li>;
}

/** The project's data circle seen through the card: centred on the right edge, its radius 60% of
 *  the card's width, so the rim curves through the card at 40% from the left. After the squares
 *  flicker in, the camera picks one of the top gaps at random and zooms to it (as on invectus.app). */
function MapSlice({ map, gaps, onGap }: { map: MapData; gaps: GapView[]; onGap: (id: number) => void }) {
  const theme = useTheme();
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const card = useRef<HTMLButtonElement>(null);
  const [box, setBox] = useState({ w: 0, h: 0 });
  const [view, setView] = useState({ s: 1, x: 0, y: 0 });
  const [focus, setFocus] = useState<{ gap: GapView; code: string; area: { x: number; y: number; radius: number } }>();
  const [arrived, setArrived] = useState(false);
  // The hovered gap, read by the canvas drawing (declared with its card below).
  const hoverRef = useRef<unknown>();
  const [hoverTick, setHoverTick] = useState(0);
  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setBox({ w: el.clientWidth, h: el.clientHeight }));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  const bounds = useMemo(() => {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const p of map.points) { x0 = Math.min(x0, p.x); x1 = Math.max(x1, p.x); y0 = Math.min(y0, p.y); y1 = Math.max(y1, p.y); }
    return x0 <= x1 ? { cx: (x0 + x1) / 2, cy: (y0 + y1) / 2, span: Math.max(x1 - x0, y1 - y0, 1e-9) } : undefined;
  }, [map]);
  const fine = useMemo(() => new Map(map.regions.filter((r) => r.level === "fine").map((r) => [r.region_id, r])), [map]);
  // The top gaps with the area each covers (a region, or the span between two).
  const marks = useMemo(() => gaps.slice(0, 12).flatMap((gap, i) => {
    const a = fine.get(gap.region_a), b = gap.region_b !== null ? fine.get(gap.region_b) : undefined;
    if (!a) return [];
    const area = b ? { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, radius: Math.hypot(a.x - b.x, a.y - b.y) / 2 + Math.max(a.radius, b.radius) } : { x: a.x, y: a.y, radius: a.radius };
    return [{ gap, code: gapCode(i), area }];
  }), [gaps, fine]);
  const { w, h } = box;
  const R = w * 0.6, CX = w, CY = h / 2;
  const k = bounds ? (2 * R * 0.94) / bounds.span : 1;
  const toScreen = (x: number, y: number): [number, number] =>
    bounds ? [CX + (x - bounds.cx) * k * view.s + view.x, CY + (y - bounds.cy) * k * view.s + view.y] : [0, 0];
  const inside = (x: number, y: number, pad = 0) => (x - CX) ** 2 + (y - CY) ** 2 <= (R - pad) ** 2 && x >= 0 && x <= w && y >= 0 && y <= h;

  // The tour: once per map, wait for the flicker, then zoom to a random top gap.
  useEffect(() => {
    if (!w || !h || !bounds || !marks.length) return;
    const pick = marks[Math.floor(Math.random() * marks.length)];
    setFocus(pick); setArrived(false); setView({ s: 1, x: 0, y: 0 });
    // Centre the gap in the open part of the slice, sized to about a fifth of the card's height.
    const s = Math.min(60, Math.max(1, (h * 0.2) / Math.max(1e-9, pick.area.radius * k)));
    const target = { s, x: w * 0.6 - CX - (pick.area.x - bounds.cx) * k * s, y: CY - CY - (pick.area.y - bounds.cy) * k * s };
    if (reducedMotion()) { setView(target); setArrived(true); return; }
    let frame = 0;
    const timer = setTimeout(() => {
      const started = performance.now();
      const step = (now: number) => {
        const p = Math.min(1, (now - started) / 1400), e = p * p * (3 - 2 * p);
        // Geometric zoom; pan interpolated in map units so the path stays straight.
        const sc = Math.pow(target.s, e);
        setView({ s: sc, x: (target.x / target.s) * e * sc, y: (target.y / target.s) * e * sc });
        if (p < 1) frame = requestAnimationFrame(step); else setArrived(true);
      };
      frame = requestAnimationFrame(step);
    }, 700);
    return () => { clearTimeout(timer); cancelAnimationFrame(frame); };
  }, [map, marks, w, h, bounds]);

  useEffect(() => {
    const c = canvas.current;
    if (!c || !w || !h || !bounds) return;
    const dpr = window.devicePixelRatio || 1;
    if (c.width !== Math.round(w * dpr) || c.height !== Math.round(h * dpr)) { c.width = Math.round(w * dpr); c.height = Math.round(h * dpr); }
    const g = c.getContext("2d")!;
    const { ink, accent, bg, inkA } = canvasColors();
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, w, h);
    g.save();
    g.beginPath(); g.arc(CX, CY, R - 1, 0, Math.PI * 2); g.clip();
    // Items in a fine screen grid; denser cells draw larger and stronger.
    const CELL = 5, cols = Math.ceil(w / CELL) + 1;
    const grid = new Map<number, { x: number; y: number; n: number }>();
    const ks = k * view.s, ox = CX - bounds.cx * ks + view.x, oy = CY - bounds.cy * ks + view.y;
    for (const p of map.points) {
      const x = p.x * ks + ox, y = p.y * ks + oy;
      if (x < 0 || x > w || y < 0 || y > h) continue;
      const key = Math.floor(y / CELL) * cols + Math.floor(x / CELL);
      const cell = grid.get(key);
      if (cell) { cell.x += x; cell.y += y; cell.n++; } else grid.set(key, { x, y, n: 1 });
    }
    g.fillStyle = ink;
    for (const cell of grid.values()) {
      const x = cell.x / cell.n, y = cell.y / cell.n, side = cell.n === 1 ? 3 : Math.min(4.6, 3 + 0.5 * Math.log2(cell.n));
      g.globalAlpha = Math.min(0.92, 0.32 + 0.11 * Math.log2(cell.n));
      g.fillRect(x - side / 2, y - side / 2, side, side);
    }
    g.globalAlpha = 1;
    // The focused gap's area, as the app outlines a region.
    if (focus && arrived) {
      const [x, y] = toScreen(focus.area.x, focus.area.y);
      g.setLineDash([3, 4]); g.strokeStyle = accent; g.lineWidth = 1;
      g.beginPath(); g.arc(x, y, Math.max(8, focus.area.radius * ks), 0, Math.PI * 2); g.stroke();
      g.setLineDash([]);
    }
    g.font = `500 9.5px "Martian Mono Variable", ui-monospace, monospace`; g.textBaseline = "middle";
    for (const m of marks) {
      const [x, y] = toScreen(m.area.x, m.area.y);
      if (!inside(x, y, 7)) continue;
      const on = (focus?.gap === m.gap && arrived) || hoverRef.current === m;
      g.fillStyle = bg; g.strokeStyle = on ? accent : ink; g.lineWidth = on ? 2 : 1;
      g.beginPath(); g.arc(x, y, 4.5, 0, Math.PI * 2); g.fill(); g.stroke();
      g.fillStyle = on ? accent : inkA(0.7); g.fillText(m.code, x + 8, y);
    }
    g.restore();
    g.strokeStyle = ink; g.lineWidth = 1;
    g.beginPath(); g.arc(CX, CY, R - 1, 0, Math.PI * 2); g.stroke();
  }, [map, marks, w, h, bounds, view, focus, arrived, theme, hoverTick]);

  // A gap's card appears only while its ring is hovered (as on invectus.app); the tour just
  // zooms to its gap and outlines it. Clicking a ring or its card opens the gap.
  const [hover, setHoverNow] = useState<(typeof marks)[number]>();
  // Leaving a ring waits a moment so the pointer can travel to its card.
  const leaveTimer = useRef<ReturnType<typeof setTimeout>>();
  const setHover = (m: (typeof marks)[number] | undefined) => {
    clearTimeout(leaveTimer.current);
    if (m) setHoverNow(m); else leaveTimer.current = setTimeout(() => setHoverNow(undefined), 250);
  };
  useEffect(() => () => clearTimeout(leaveTimer.current), []);
  useEffect(() => { hoverRef.current = hover; setHoverTick((n) => n + 1); }, [hover]);
  const ringAt = (x: number, y: number) => marks.find((m) => {
    const [mx, my] = toScreen(m.area.x, m.area.y);
    return inside(mx, my, 7) && (mx - x) ** 2 + (my - y) ** 2 <= 12 ** 2;
  });
  const local = (e: React.PointerEvent | React.MouseEvent) => { const r = wrap.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
  const anchor = hover ? toScreen(hover.area.x, hover.area.y) : undefined;
  const cardWidth = 260;
  const reach = hover ? (hover === focus && arrived ? Math.max(8, hover.area.radius * k * view.s) : 8) + 16 : 0; // clear of the ring or outline
  const left = anchor ? (anchor[0] + reach + cardWidth < w - 12 ? anchor[0] + reach : anchor[0] - reach - cardWidth) : 0;
  return <div ref={wrap} className="home-slice" data-hover={!!hover || undefined}
    onPointerMove={(e) => { if ((e.target as HTMLElement).closest(".home-gap-card")) { clearTimeout(leaveTimer.current); return; } const [x, y] = local(e); setHover(ringAt(x, y)); }}
    onPointerLeave={() => setHover(undefined)}
    onClick={(e) => { const [x, y] = local(e); const m = ringAt(x, y); if (m) onGap(m.gap.gap_id); }}>
    <canvas ref={canvas} style={{ width: w, height: h }} aria-hidden="true" />
    {hover && anchor && <button ref={card} type="button" className="home-gap-card" style={{ left: Math.max(w * 0.42, left), top: Math.min(h - 150, Math.max(12, anchor[1] - 60)), width: cardWidth }}
      onClick={(e) => { e.stopPropagation(); onGap(hover.gap.gap_id); }}>
      <span className="home-gap-card-code">{hover.code} · {KIND_LABEL[hover.gap.kind]}</span>
      <b>{hover.gap.label}</b>
      <span className="home-gap-card-why">{hover.gap.explanation}</span>
      <span className="home-gap-card-foot">Score {hover.gap.score.toFixed(2)} · open gap ↗</span>
    </button>}
  </div>;
}

function TopGaps({ gaps, onGap, onAll }: { gaps: GapView[]; onGap: (id: number) => void; onAll: () => void }) {
  const shown = gaps.slice(0, TOP_GAPS);
  const top = Math.max(...shown.map((g) => g.score), 1e-9);
  const last = Math.max(...shown.flatMap((g) => g.timeline.map((y) => y.year)), new Date().getFullYear() - 1);
  const first = last - SPARK_YEARS + 1;
  const kinds = Object.entries(gaps.reduce<Record<string, number>>((n, g) => ({ ...n, [g.kind]: (n[g.kind] ?? 0) + 1 }), {}))
    .sort((a, b) => b[1] - a[1]).map(([kind, n]) => `${fmt(n)} ${kind}`).join(" · ");
  return <section className="home-card" aria-label="Top gaps">
    <div className="home-card-head"><h2>Top gaps</h2><button type="button" className="text-btn" onClick={onAll}>All {fmt(gaps.length)} gaps ↗</button></div>
    <div className="home-gaps-head"><span>{kinds} · ranked by score</span><span>Demand {first}–{last}</span><span className="num">Score</span></div>
    {shown.map((g, i) => {
      const byYear = new Map(g.timeline.map((y) => [y.year, y.demand]));
      const values = Array.from({ length: SPARK_YEARS }, (_, j) => byYear.get(first + j) ?? 0);
      const delay = ROW_START + i * ROW_DELAY;
      return <button key={g.gap_id} type="button" className="home-gap" style={{ "--delay": `${delay}ms` } as CSSProperties} onClick={() => onGap(g.gap_id)}>
        <span className="home-gap-code">{gapCode(i)}</span>
        <span className="home-gap-main"><span className="home-gap-label">{g.label}</span><span className="home-gap-why">{g.explanation}</span></span>
        <Spark values={values} delay={delay} />
        <span className="home-gap-score"><span className="home-gap-meter"><i style={{ width: `${(g.score / top) * 100}%` }} /></span><span className="home-gap-value">{g.score.toFixed(2)}</span></span>
      </button>;
    })}
  </section>;
}

/** Demand per year; each point springs up from the baseline, left to right. */
function Spark({ values, delay }: { values: number[]; delay: number }) {
  const line = useRef<SVGPolylineElement>(null);
  const area = useRef<SVGPolygonElement>(null);
  useEffect(() => {
    const W = 140, H = 28, max = Math.max(...values, 1), step = stagger(values.length, 350);
    const draw = (t: number) => {
      const pts = values.map((v, j) => `${((j * W) / (values.length - 1)).toFixed(1)},${(H - 2 - (v / max) * (H - 4) * springAt(t - delay - j * step)).toFixed(1)}`).join(" ");
      line.current?.setAttribute("points", pts);
      area.current?.setAttribute("points", `0,${H} ${pts} ${W},${H}`);
    };
    if (reducedMotion()) { draw(Infinity); return; }
    const end = delay + values.length * step + SPRING_MS;
    const t0 = performance.now();
    let frame = 0;
    const tick = (now: number) => { draw(now - t0); if (now - t0 < end) frame = requestAnimationFrame(tick); };
    draw(0);
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [values.join(","), delay]);
  return <svg className="home-spark" viewBox="0 0 140 28" preserveAspectRatio="none" aria-hidden="true">
    <polygon ref={area} className="home-spark-area" /><polyline ref={line} className="home-spark-line" />
  </svg>;
}
