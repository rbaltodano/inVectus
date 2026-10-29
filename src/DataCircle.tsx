// The dataset as a circle: every item is a small square at its map position, clipped to the
// circle; squares that would overlap on screen merge into one larger square that splits up as
// you zoom. Map positions are for browsing only (SPEC §6.5): nothing here is evidence.
import { useEffect, useMemo, useRef, useState, type PointerEvent as PE } from "react";
import { fmt, year, KIND_LABEL, type GapView, type JobInfo, type MapData, type MapPoint, type Region } from "./api";

export interface Marks {
  /** Estimated position of the searched idea, in map coordinates (approximate). */
  query?: { x: number; y: number; label: string };
  /** Items to outline in the accent colour (e.g. the hovered ranking). */
  items?: number[];
  /** A region to outline, e.g. from "sits between". */
  region?: number;
}

interface Props {
  data?: MapData;
  gaps: GapView[];
  marks: Marks;
  showGaps: boolean;
  showLabels: boolean;
  showOutlines: boolean;
  /** The running job of this project, for the loading states. */
  job?: JobInfo;
  /** Nothing imported yet: the circle is a button that adds a dataset. */
  empty?: { title: string; hint: string; onPick: () => void };
  /** Shown in the centre while there is no map yet (and no job). */
  idle?: string;
  onGap: (id: number) => void;
  onItem: (p: MapPoint) => void;
  /** Bumped by the parent to reset the view. */
  resetKey?: number;
}

const INK = "#f7f3eb";
const ACCENT = "#6edcaa";
const CELL = 10;          // screen px per merge cell
const SINGLE = 4;         // one item
const MERGED_MAX = 7;     // a merged square stays inside its cell, so the grid reads as squares
const GAP_MARKS = 12;     // the highest-scoring gaps only
const PLACEHOLDERS = 1400;

interface Cell { x: number; y: number; n: number; first: MapPoint; regions: Map<number, number> }

/** Deterministic pseudo-random positions inside the unit disc (placeholders while loading). */
function disc(i: number): [number, number] {
  const a = Math.sin(i * 12.9898) * 43758.5453, b = Math.sin(i * 78.233) * 12345.6789;
  const r = Math.sqrt(a - Math.floor(a)) * 0.86, t = (b - Math.floor(b)) * Math.PI * 2;
  return [Math.cos(t) * r, Math.sin(t) * r];
}

export default function DataCircle({ data, gaps, marks, showGaps, showLabels, showOutlines, job, empty, idle, onGap, onItem, resetKey }: Props) {
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const [size, setSize] = useState(560);
  const [view, setView] = useState({ s: 1, x: 0, y: 0 });
  const [hover, setHover] = useState<{ cell?: Cell; gap?: GapView; query?: boolean; sx: number; sy: number }>();
  const drag = useRef<{ x: number; y: number; v: typeof view; moved: boolean }>();
  const cells = useRef<Cell[]>([]);
  // Loading state B: placeholder squares that glide to real positions when the map arrives.
  const placeholders = useRef(0);
  const [glide, setGlide] = useState(0); // 0 = none, (0,1] = animating
  const prevData = useRef<MapData>();

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize(Math.max(260, Math.min(el.clientWidth, el.clientHeight))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useEffect(() => { setView({ s: 1, x: 0, y: 0 }); }, [resetKey, data?.run.run_id]);

  const R = size / 2;
  const loading = job?.state === "running" && (job.kind === "index" || job.kind === "regions");
  const frac = job && job.total ? job.done / job.total : 0;
  if (loading) placeholders.current = Math.max(placeholders.current, Math.round(frac * Math.min(PLACEHOLDERS, Math.max(job!.total, 1))));

  // When a map appears after loading, glide the placeholders to it.
  useEffect(() => {
    if (data && data !== prevData.current && placeholders.current > 0 && !loading) {
      const t0 = performance.now();
      let raf = 0;
      const step = (t: number) => {
        const k = Math.min(1, (t - t0) / 1100);
        setGlide(k);
        if (k < 1) raf = requestAnimationFrame(step); else { placeholders.current = 0; setGlide(0); }
      };
      raf = requestAnimationFrame(step);
      prevData.current = data;
      return () => cancelAnimationFrame(raf);
    }
    prevData.current = data;
  }, [data, loading]);

  // Fit the layout's bounding box to the circle's bounding square; the circle clips the rest.
  const fit = useMemo(() => {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const p of data?.points ?? []) { x0 = Math.min(x0, p.x); x1 = Math.max(x1, p.x); y0 = Math.min(y0, p.y); y1 = Math.max(y1, p.y); }
    if (!(x0 <= x1)) return { k: 1, cx: 0, cy: 0 };
    return { k: (size * 0.94) / Math.max(x1 - x0, y1 - y0, 1e-9), cx: (x0 + x1) / 2, cy: (y0 + y1) / 2 };
  }, [data, size]);
  const toScreen = (x: number, y: number): [number, number] =>
    [R + ((x - fit.cx) * fit.k) * view.s + view.x, R + ((y - fit.cy) * fit.k) * view.s + view.y];
  const inside = (x: number, y: number, pad = 0) => (x - R) ** 2 + (y - R) ** 2 <= (R - pad) ** 2;

  const fine = useMemo(() => new Map((data?.regions ?? []).filter((r) => r.level === "fine").map((r) => [r.region_id, r])), [data]);
  const regionName = (r?: Region) => r ? r.llm_label ?? r.label : "";
  const gapPos = (g: GapView): [number, number] | undefined => {
    const a = fine.get(g.region_a), b = g.region_b !== null ? fine.get(g.region_b) : undefined;
    if (!a) return undefined;
    return b ? toScreen((a.x + b.x) / 2, (a.y + b.y) / 2) : toScreen(a.x, a.y);
  };

  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size * dpr; c.height = size * dpr;
    const g = c.getContext("2d")!;
    g.setTransform(dpr, 0, 0, dpr, 0, 0);
    g.clearRect(0, 0, size, size);
    g.save();
    g.beginPath(); g.arc(R, R, R - 1, 0, Math.PI * 2); g.clip();

    // Merge items per screen cell.
    const grid = new Map<number, Cell>();
    const cols = Math.ceil(size / CELL) + 1;
    if (data && !loading) for (const p of data.points) {
      const [x, y] = toScreen(p.x, p.y);
      if (!inside(x, y, -6)) continue;
      const k = Math.floor(y / CELL) * cols + Math.floor(x / CELL);
      const cell = grid.get(k);
      if (cell) { cell.x += x; cell.y += y; cell.n++; cell.regions.set(p.fine_id, (cell.regions.get(p.fine_id) ?? 0) + 1); }
      else grid.set(k, { x, y, n: 1, first: p, regions: new Map([[p.fine_id, 1]]) });
    }
    const list = [...grid.values()].map((c) => ({ ...c, x: c.x / c.n, y: c.y / c.n }));
    cells.current = list;

    // Region outlines: hidden until hover (or the toggle / a marked region).
    const majority = (c: Cell) => [...c.regions.entries()].sort((a, b) => b[1] - a[1])[0]?.[0];
    const outlined = new Set<number>();
    if (showOutlines) fine.forEach((_, id) => outlined.add(id));
    if (hover?.cell) { const m = majority(hover.cell); if (m !== undefined) outlined.add(m); }
    if (marks.region !== undefined) outlined.add(marks.region);
    g.setLineDash([3, 4]); g.lineWidth = 1;
    for (const id of outlined) {
      const r = fine.get(id); if (!r) continue;
      const [x, y] = toScreen(r.x, r.y);
      g.strokeStyle = id === marks.region ? ACCENT : "rgba(247, 243, 235, 0.45)";
      g.beginPath(); g.arc(x, y, Math.max(6, r.radius * fit.k * view.s), 0, Math.PI * 2); g.stroke();
    }
    g.setLineDash([]);

    // Placeholders (state B), gliding to real cells when the map arrives.
    const n = placeholders.current;
    if (n > 0 && (loading || glide > 0)) {
      const ease = glide * glide * (3 - 2 * glide);
      g.fillStyle = INK;
      for (let i = 0; i < n; i++) {
        const [ux, uy] = disc(i);
        let x = R + ux * R, y = R + uy * R;
        if (glide > 0 && list.length) { const t = list[(i * 7919) % list.length]; x += (t.x - x) * ease; y += (t.y - y) * ease; }
        g.globalAlpha = loading ? 0.55 : 0.55 * (1 - ease) + 0.2;
        g.fillRect(x - 2, y - 2, 4, 4);
      }
      g.globalAlpha = 1;
    }

    // Items (merged): one square per cell, larger when it holds more.
    if (!loading) {
      g.globalAlpha = glide > 0 ? glide : 1;
      const hl = new Set(marks.items ?? []);
      for (const c of list) {
        const s = c.n === 1 ? SINGLE : Math.min(MERGED_MAX, SINGLE + 0.9 * Math.log2(c.n));
        g.fillStyle = INK;
        g.globalAlpha = (glide > 0 ? glide : 1) * (c.n === 1 ? 0.85 : 1);
        g.fillRect(c.x - s / 2, c.y - s / 2, s, s);
        if (hl.size && hl.has(c.first.item_id)) { g.strokeStyle = ACCENT; g.lineWidth = 1.5; g.strokeRect(c.x - s / 2 - 3, c.y - s / 2 - 3, s + 6, s + 6); }
      }
      if (hl.size) {
        g.strokeStyle = ACCENT; g.lineWidth = 1.5;
        for (const p of data?.points ?? []) if (hl.has(p.item_id)) { const [x, y] = toScreen(p.x, p.y); g.strokeRect(x - 5, y - 5, 10, 10); }
      }
      g.globalAlpha = 1;
    }

    // Gap marks: small rings.
    if (showGaps && !loading) {
      g.strokeStyle = INK; g.lineWidth = 1.5;
      for (const gp of gaps.slice(0, GAP_MARKS)) {
        const p = gapPos(gp); if (!p || !inside(p[0], p[1], 4)) continue;
        g.fillStyle = "#121212";
        g.beginPath(); g.arc(p[0], p[1], 5, 0, Math.PI * 2); g.fill(); g.stroke();
      }
    }

    // The searched idea: an accent square with a glow.
    if (marks.query && !loading) {
      const [x, y] = toScreen(marks.query.x, marks.query.y);
      g.shadowColor = ACCENT; g.shadowBlur = 24; g.fillStyle = ACCENT;
      g.fillRect(x - 5.6, y - 5.6, 11.2, 11.2);
      g.shadowBlur = 0;
    }
    g.restore();

    // The rim, and in loading state A the progress arc along it.
    g.strokeStyle = INK; g.lineWidth = 1;
    g.beginPath(); g.arc(R, R, R - 1, 0, Math.PI * 2); g.stroke();
    if (loading && frac > 0) {
      g.lineWidth = 3;
      g.beginPath(); g.arc(R, R, R - 2, -Math.PI / 2, -Math.PI / 2 + frac * Math.PI * 2); g.stroke();
    }
  }, [data, size, view, gaps, marks, showGaps, showOutlines, hover?.cell, loading, frac, glide, fit]);

  // Pointer: drag to pan, wheel to zoom, hover to inspect, click to open.
  const at = (e: PE | React.WheelEvent) => { const r = canvas.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
  const find = (x: number, y: number) => {
    if (!inside(x, y)) return undefined;
    if (showGaps) for (const gp of gaps.slice(0, GAP_MARKS)) { const p = gapPos(gp); if (p && (p[0] - x) ** 2 + (p[1] - y) ** 2 < 64) return { gap: gp }; }
    if (marks.query) { const [qx, qy] = toScreen(marks.query.x, marks.query.y); if ((qx - x) ** 2 + (qy - y) ** 2 < 81) return { query: true }; }
    let best: Cell | undefined, bd = 100;
    for (const c of cells.current) { const d = (c.x - x) ** 2 + (c.y - y) ** 2; if (d < bd) { bd = d; best = c; } }
    return best ? { cell: best } : undefined;
  };
  const zoomAt = (f: number, cx: number, cy: number) => setView((v) => {
    const s = Math.min(200, Math.max(1, v.s * f)), k = s / v.s;
    if (s === 1) return { s: 1, x: 0, y: 0 };
    return { s, x: cx - R - (cx - R - v.x) * k, y: cy - R - (cy - R - v.y) * k };
  });

  if (empty) return <div ref={wrap} className="circle-wrap">
    <button type="button" className="circle empty-circle" style={{ width: size, height: size }} onClick={empty.onPick}>
      <span className="circle-title">{empty.title}</span>
      <span className="circle-sub">{empty.hint}</span>
    </button>
  </div>;

  const pct = Math.round(frac * 100);
  const g = hover?.gap, c = hover?.cell;
  const majorityRegion = c ? fine.get([...c.regions.entries()].sort((a, b) => b[1] - a[1])[0][0]) : undefined;

  return <div ref={wrap} className="circle-wrap">
    <div className="circle" style={{ width: size, height: size }}>
      <canvas ref={canvas} style={{ width: size, height: size, cursor: drag.current?.moved ? "grabbing" : hover ? "pointer" : "grab" }}
        role="img" aria-label={data ? `Map of ${fmt(data.points.length)} items; positions are for browsing only` : "Dataset circle"}
        onPointerDown={(e) => { const [x, y] = at(e); canvas.current!.setPointerCapture(e.pointerId); drag.current = { x, y, v: view, moved: false }; }}
        onPointerMove={(e) => {
          const [x, y] = at(e); const d = drag.current;
          if (d) { if (Math.abs(x - d.x) + Math.abs(y - d.y) > 3) d.moved = true; if (d.moved && d.v.s > 1) setView({ ...d.v, x: d.v.x + x - d.x, y: d.v.y + y - d.y }); return; }
          const f = find(x, y); setHover(f ? { ...f, sx: x, sy: y } : undefined);
        }}
        onPointerUp={(e) => {
          const d = drag.current; drag.current = undefined;
          if (!d || d.moved) return;
          const [x, y] = at(e); const f = find(x, y);
          if (f?.gap) onGap(f.gap.gap_id);
          else if (f?.cell && f.cell.n === 1) onItem(f.cell.first);
          else if (f?.cell) zoomAt(2.5, f.cell.x, f.cell.y);
        }}
        onPointerLeave={() => setHover(undefined)}
        onWheel={(e) => { const [x, y] = at(e); zoomAt(e.deltaY < 0 ? 1.18 : 1 / 1.18, x, y); }} />

      {showLabels && !loading && [...fine.values()].sort((a, b) => b.size - a.size).slice(0, 16).map((r) => {
        const [x, y] = toScreen(r.x, r.y);
        return inside(x, y, 30) && <span key={r.region_id} className="circle-label" style={{ left: x, top: y }}>{regionName(r).slice(0, 28)}</span>;
      })}

      {(loading || (!data && idle)) && <div className="circle-center">
        <span className="circle-title">{loading ? `${job!.phase?.startsWith("Mapping") ? "Mapping" : job!.phase === "Calibrating" ? "Calibrating" : "Indexing"}… ${pct}%` : idle}</span>
        {loading && <span className="circle-sub">{job!.total ? `${fmt(job!.done)} / ${fmt(job!.total)}` : ""}{job!.phase?.startsWith("Embedding") ? " · positions are placeholders until mapping finishes" : ""}</span>}
      </div>}

      {hover && !drag.current?.moved && <div className="circle-tip" style={{ left: Math.min(hover.sx + 14, size - 230), top: hover.sy + 14 }}>
        {g && <><b>{KIND_LABEL[g.kind]}</b><span>{g.label}</span><span className="dim">Score {g.score.toFixed(2)} · click to open</span></>}
        {hover.query && <><b>Your search</b><span className="dim">Approximate position: {marks.query?.label}</span></>}
        {c && c.n === 1 && <><b>{c.first.title ?? `item ${c.first.item_id}`}</b><span className="dim">{regionName(fine.get(c.first.fine_id))}{c.first.date ? ` · ${year(c.first.date)}` : ""}</span></>}
        {c && c.n > 1 && <><b>{fmt(c.n)} items</b><span className="dim">Mostly {regionName(majorityRegion)} · click to zoom</span></>}
      </div>}

      {view.s > 1 && <button type="button" className="circle-reset" onClick={() => setView({ s: 1, x: 0, y: 0 })}>Reset view</button>}
    </div>
  </div>;
}
