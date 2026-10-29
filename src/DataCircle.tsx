// The dataset as a circle: every item is a small square at its map position, clipped to the
// circle; squares that would overlap on screen merge into one larger square that splits up as
// you zoom. Map positions are for browsing only (SPEC §6.5): nothing here is evidence.
import { useCallback, useEffect, useMemo, useRef, useState, type PointerEvent as PE } from "react";
import LineArt from "./LineArt";
import { fmt, year, KIND_LABEL, type GapView, type JobInfo, type MapData, type MapPoint, type Region } from "./api";
import { DotFlicker } from "./dot-flicker";
import { regionFocusView } from "./map-focus";

export interface Marks {
  /** Estimated position of the searched idea, in map coordinates (approximate). */
  query?: { x: number; y: number; label: string };
  /** Items to outline in the accent colour (e.g. the hovered ranking). */
  items?: number[];
  /** A region to outline, e.g. from "sits between". */
  region?: number;
  /** Short labels for marked items, e.g. their similarity ("0.83"). */
  labels?: Record<number, string>;
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
  /** Explicit focus requests; the counter lets the same region be focused again after panning. */
  focusRegion?: { region: number; request: number };
}

const INK = "#eeeae4";
const ACCENT = "#6edcaa";
const SINGLE = 4;         // one item, in screen px
const LABEL_FONT = `500 9.5px "Martian Mono Variable", ui-monospace, monospace`;
const white = (a: number) => `rgba(255, 255, 255, ${a})`;
const markKey = {
  query: (q: { x: number; y: number }) => `mark:query:${q.x}:${q.y}`,
  item: (id: number) => `mark:item:${id}`,
  region: (id: number) => `mark:region:${id}`,
};
/** A gap's code, shared by the map and the gap list (rank by score, 1-based). */
export const gapCode = (rank: number) => `G-${String(rank + 1).padStart(2, "0")}`;
const MERGED_MAX = 11;    // the largest merged square
const GAP_MARKS = 12;     // the highest-scoring gaps only
// Navigation follows Angrove's Mac canvas (macOS/Platform/MacCanvasScrollInput.swift and
// InsightTreeCanvasGestures.swift): two-finger scroll pans with native momentum, Shift pans
// sideways, Command-scroll zooms at the pointer by exp(delta * 0.012), pinch zooms by
// magnification^0.72, click-drag pans.
const ZOOM_PER_DELTA = 0.012;
const PINCH_DAMPING = 0.72;
const ZOOM_MIN = 0.6, ZOOM_MAX = 200;

/** A square's side for a group of `n` items. */
const side = (n: number) => (n === 1 ? SINGLE : Math.min(MERGED_MAX, SINGLE + 1.3 * Math.log2(n)));

/** Merge squares that touch until none do: start from points in a fine grid, then repeatedly
 *  join groups whose squares overlap or touch (their sides grow as they merge). */
function mergeTouching(cells: Cell[]): Cell[] {
  let groups = cells;
  for (let pass = 0; pass < 6; pass++) {
    const bucket = MERGED_MAX + 2;
    const hash = new Map<string, number[]>();
    groups.forEach((c, i) => {
      const k = `${Math.floor(c.x / bucket)},${Math.floor(c.y / bucket)}`;
      (hash.get(k) ?? hash.set(k, []).get(k)!).push(i);
    });
    const parent = groups.map((_, i) => i);
    const find = (i: number): number => (parent[i] === i ? i : (parent[i] = find(parent[i])));
    let merged = false;
    groups.forEach((a, i) => {
      const bx = Math.floor(a.x / bucket), by = Math.floor(a.y / bucket);
      for (let dx = -1; dx <= 1; dx++) for (let dy = -1; dy <= 1; dy++) {
        for (const j of hash.get(`${bx + dx},${by + dy}`) ?? []) {
          if (j <= i) continue;
          const b = groups[j], reach = (side(a.n) + side(b.n)) / 2 + 1;
          if (Math.abs(a.x - b.x) < reach && Math.abs(a.y - b.y) < reach) {
            const ra = find(i), rb = find(j);
            if (ra !== rb) { parent[rb] = ra; merged = true; }
          }
        }
      }
    });
    if (!merged) break;
    const joined = new Map<number, Cell>();
    groups.forEach((c, i) => {
      const r = find(i), t = joined.get(r);
      if (!t) { joined.set(r, { ...c, x: c.x * c.n, y: c.y * c.n, regions: new Map(c.regions) }); return; }
      t.x += c.x * c.n; t.y += c.y * c.n; t.n += c.n;
      c.regions.forEach((v, k) => t.regions.set(k, (t.regions.get(k) ?? 0) + v));
    });
    groups = [...joined.values()].map((c) => ({ ...c, x: c.x / c.n, y: c.y / c.n }));
  }
  return groups;
}
const PLACEHOLDERS = 1400;

interface Cell { x: number; y: number; n: number; first: MapPoint; regions: Map<number, number> }

/** Deterministic pseudo-random positions inside the unit disc (placeholders while loading). */
function disc(i: number): [number, number] {
  const a = Math.sin(i * 12.9898) * 43758.5453, b = Math.sin(i * 78.233) * 12345.6789;
  const r = Math.sqrt(a - Math.floor(a)) * 0.86, t = (b - Math.floor(b)) * Math.PI * 2;
  return [Math.cos(t) * r, Math.sin(t) * r];
}

export default function DataCircle({ data, gaps, marks, showGaps, showLabels, showOutlines, job, empty, idle, onGap, onItem, resetKey, focusRegion }: Props) {
  const wrap = useRef<HTMLDivElement>(null);
  const canvas = useRef<HTMLCanvasElement>(null);
  const focusedRequest = useRef<Props["focusRegion"]>();
  const [size, setSize] = useState(560);
  const [view, setView] = useState({ s: 1, x: 0, y: 0 });
  const viewRef = useRef(view);
  viewRef.current = view;
  const zoomFrame = useRef<number>();
  const stopZoom = useCallback(() => {
    if (zoomFrame.current !== undefined) cancelAnimationFrame(zoomFrame.current);
    zoomFrame.current = undefined;
  }, []);
  const [hover, setHover] = useState<{ cell?: Cell; gap?: GapView; query?: boolean; sx: number; sy: number }>();
  const drag = useRef<{ x: number; y: number; v: typeof view; moved: boolean }>();
  const cells = useRef<Cell[]>([]);
  // Loading state B: placeholder squares that glide to real positions when the map arrives.
  const placeholders = useRef(0);
  const [glide, setGlide] = useState(0); // 0 = none, (0,1] = animating
  const prevData = useRef<MapData>();
  const [flicker] = useState(() => new DotFlicker());
  const [flickerFrame, setFlickerFrame] = useState(0);
  const [reducedMotion, setReducedMotion] = useState(() => window.matchMedia("(prefers-reduced-motion: reduce)").matches);

  useEffect(() => {
    const preference = window.matchMedia("(prefers-reduced-motion: reduce)");
    const changed = () => setReducedMotion(preference.matches);
    preference.addEventListener("change", changed);
    return () => preference.removeEventListener("change", changed);
  }, []);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize(Math.max(260, Math.min(el.clientWidth, el.clientHeight))));
    ro.observe(el);
    return () => ro.disconnect();
  }, []);
  useEffect(() => { stopZoom(); setView({ s: 1, x: 0, y: 0 }); }, [resetKey, data?.run.run_id, stopZoom]);
  // A resized canvas changes the zoom anchor; also cancel pending frames on unmount.
  useEffect(() => stopZoom, [size, stopZoom]);

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
  useEffect(() => {
    if (!focusRegion || focusedRequest.current === focusRegion) return;
    const region = fine.get(focusRegion.region);
    if (!region) return;
    focusedRequest.current = focusRegion;
    stopZoom();
    setHover(undefined);
    setView(regionFocusView(region, fit, size, ZOOM_MIN, ZOOM_MAX));
    canvas.current?.focus({ preventScroll: false });
  }, [focusRegion, fine, fit, size, stopZoom]);
  const regionName = (r?: Region) => r ? r.llm_label ?? r.label : "";
  const gapPos = (g: GapView): [number, number] | undefined => {
    const a = fine.get(g.region_a), b = g.region_b !== null ? fine.get(g.region_b) : undefined;
    if (!a) return undefined;
    return b ? toScreen((a.x + b.x) / 2, (a.y + b.y) / 2) : toScreen(a.x, a.y);
  };

  const renderedCells = useMemo(() => {
    // Group items in a fine screen grid, then merge squares that touch.
    const CELL = SINGLE + 1;
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
    return mergeTouching([...grid.values()].map((c) => ({ ...c, x: c.x / c.n, y: c.y / c.n })));
  }, [data, size, view, loading, fit]);
  const placeholderCount = placeholders.current;
  const showPlaceholders = placeholderCount > 0 && (loading || glide > 0);
  useEffect(() => {
    const keys = renderedCells.map((cell) => `item:${data?.run.run_id}:${cell.first.item_id}`);
    if (showPlaceholders) for (let i = 0; i < placeholderCount; i++) keys.push(`placeholder:${i}`);
    // Accent marks flicker in like the dots whenever they (re)appear, e.g. back on Search.
    if (marks.query) keys.push(markKey.query(marks.query));
    for (const id of marks.items ?? []) keys.push(markKey.item(id));
    if (marks.region !== undefined) keys.push(markKey.region(marks.region));
    flicker.sync(keys, performance.now());
  }, [renderedCells, showPlaceholders, placeholderCount, data?.run.run_id, flicker, marks]);

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

    const list = renderedCells;
    cells.current = list;
    const now = performance.now();
    const opacity = (key: string) => reducedMotion ? 1 : flicker.opacity(key, now);

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
      g.strokeStyle = id === marks.region ? ACCENT : "rgba(238, 234, 228, 0.45)";
      g.globalAlpha = id === marks.region ? opacity(markKey.region(id)) : 1;
      g.beginPath(); g.arc(x, y, Math.max(6, r.radius * fit.k * view.s), 0, Math.PI * 2); g.stroke();
      g.globalAlpha = 1;
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
        g.globalAlpha = (loading ? 0.55 : 0.55 * (1 - ease) + 0.2) * opacity(`placeholder:${i}`);
        g.fillRect(x - 2, y - 2, 4, 4);
      }
      g.globalAlpha = 1;
    }

    // Items (merged): one square per cell, larger when it holds more.
    if (!loading) {
      const hl = new Set(marks.items ?? []);
      for (const c of list) {
        const s = side(c.n);
        g.fillStyle = INK;
        g.globalAlpha = (c.n === 1 ? 0.62 : 0.88) * opacity(`item:${data?.run.run_id}:${c.first.item_id}`);
        g.fillRect(c.x - s / 2, c.y - s / 2, s, s);
      }
      g.globalAlpha = 1;
      // Marked items (search results): an accent × with a short label, like a plotting board.
      if (hl.size) {
        g.strokeStyle = ACCENT; g.fillStyle = ACCENT; g.lineWidth = 1.25; g.font = LABEL_FONT; g.textBaseline = "middle";
        for (const p of data?.points ?? []) if (hl.has(p.item_id)) {
          const [x, y] = toScreen(p.x, p.y);
          g.globalAlpha = opacity(markKey.item(p.item_id));
          g.beginPath(); g.moveTo(x - 5, y - 5); g.lineTo(x + 5, y + 5); g.moveTo(x + 5, y - 5); g.lineTo(x - 5, y + 5); g.stroke();
          const label = marks.labels?.[p.item_id];
          if (label) g.fillText(label, x + 9, y - 7);
        }
        g.globalAlpha = 1;
      }
    }

    // Gap marks: small rings with their codes.
    if (showGaps && !loading) {
      g.lineWidth = 1.25; g.font = LABEL_FONT; g.textBaseline = "middle";
      gaps.slice(0, GAP_MARKS).forEach((gp, i) => {
        const p = gapPos(gp); if (!p || !inside(p[0], p[1], 4)) return;
        g.strokeStyle = INK; g.fillStyle = "#0e0f10";
        g.beginPath(); g.arc(p[0], p[1], 4.5, 0, Math.PI * 2); g.fill(); g.stroke();
        // Codes on the strongest few only; the rest show theirs on hover.
        if (i < 6) { g.fillStyle = white(0.55); g.fillText(gapCode(i), p[0] + 8, p[1]); }
      });
    }

    // The searched idea: an accent crosshair with a glow, labelled.
    if (marks.query && !loading) {
      const [x, y] = toScreen(marks.query.x, marks.query.y);
      g.globalAlpha = opacity(markKey.query(marks.query));
      g.shadowColor = ACCENT; g.shadowBlur = 16; g.strokeStyle = ACCENT; g.lineWidth = 1.5;
      g.beginPath(); g.moveTo(x - 8, y); g.lineTo(x + 8, y); g.moveTo(x, y - 8); g.lineTo(x, y + 8); g.stroke();
      g.shadowBlur = 0;
      g.fillStyle = ACCENT; g.font = LABEL_FONT; g.textBaseline = "middle";
      g.fillText("QUERY", x + 11, y + 9);
      g.globalAlpha = 1;
    }
    g.restore();

    // The rim, and in loading state A the progress arc along it.
    g.strokeStyle = INK; g.lineWidth = 1;
    g.beginPath(); g.arc(R, R, R - 1, 0, Math.PI * 2); g.stroke();
    if (loading && frac > 0) {
      g.lineWidth = 3;
      g.beginPath(); g.arc(R, R, R - 2, -Math.PI / 2, -Math.PI / 2 + frac * Math.PI * 2); g.stroke();
    }
    if (!reducedMotion && flicker.pending(now)) {
      const frame = requestAnimationFrame(setFlickerFrame);
      return () => cancelAnimationFrame(frame);
    }
  }, [data, size, view, gaps, marks, showGaps, showOutlines, hover?.cell, loading, frac, glide, fit, renderedCells, flickerFrame, reducedMotion]);

  // Pointer: drag to pan, wheel to zoom, hover to inspect, click to open.
  const at = (e: PE) => { const r = canvas.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
  const find = (x: number, y: number) => {
    if (!inside(x, y)) return undefined;
    if (showGaps) for (const gp of gaps.slice(0, GAP_MARKS)) { const p = gapPos(gp); if (p && (p[0] - x) ** 2 + (p[1] - y) ** 2 < 64) return { gap: gp }; }
    if (marks.query) { const [qx, qy] = toScreen(marks.query.x, marks.query.y); if ((qx - x) ** 2 + (qy - y) ** 2 < 81) return { query: true }; }
    let best: Cell | undefined, bd = 100;
    for (const c of cells.current) { const d = (c.x - x) ** 2 + (c.y - y) ** 2; if (d < bd) { bd = d; best = c; } }
    return best ? { cell: best } : undefined;
  };
  // Keep some of the data inside the circle however far it is dragged.
  const clampPan = (v: { s: number; x: number; y: number }) => {
    const lim = R * Math.max(1, v.s);
    return { s: v.s, x: Math.max(-lim, Math.min(lim, v.x)), y: Math.max(-lim, Math.min(lim, v.y)) };
  };
  const zoomTo = (v: { s: number; x: number; y: number }, next: number, cx: number, cy: number) => {
    const s = Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, next)), k = s / v.s;
    return clampPan({ s, x: cx - R - (cx - R - v.x) * k, y: cy - R - (cy - R - v.y) * k });
  };
  const zoomAt = (f: number, cx: number, cy: number) => {
    stopZoom();
    const start = viewRef.current;
    const target = zoomTo(start, start.s * f, cx, cy);
    setHover(undefined);
    if (window.matchMedia("(prefers-reduced-motion: reduce)").matches) {
      setView(target);
      return;
    }
    const t0 = performance.now();
    const step = (t: number) => {
      const progress = Math.min(1, (t - t0) / 360);
      const ease = progress * progress * (3 - 2 * progress);
      // Geometric scale interpolation keeps the clicked location anchored throughout.
      setView(zoomTo(start, start.s * Math.pow(target.s / start.s, ease), cx, cy));
      if (progress < 1) zoomFrame.current = requestAnimationFrame(step);
      else zoomFrame.current = undefined;
    };
    zoomFrame.current = requestAnimationFrame(step);
  };

  // Wheel and pinch need non-passive listeners (React's onWheel cannot preventDefault).
  const pinch = useRef<{ s: number; v: typeof view } | null>(null);
  useEffect(() => {
    const c = canvas.current;
    if (!c) return;
    const local = (e: { clientX: number; clientY: number }) => { const r = c.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
    const wheel = (e: WheelEvent) => {
      e.preventDefault();
      stopZoom();
      const unit = e.deltaMode === 1 ? 16 : e.deltaMode === 2 ? size : 1;
      const dx = e.deltaX * unit, dy = e.deltaY * unit;
      const [x, y] = local(e);
      if (e.ctrlKey) {
        // Trackpad pinch as delivered by Chromium-based webviews.
        setView((v) => zoomTo(v, v.s * Math.exp(-dy * ZOOM_PER_DELTA), x, y));
      } else if (e.metaKey) {
        const vertical = Math.max(-60, Math.min(60, dy === 0 ? dx : dy));
        setView((v) => zoomTo(v, v.s * Math.exp(-vertical * ZOOM_PER_DELTA), x, y));
      } else if (e.shiftKey) {
        setView((v) => clampPan({ ...v, x: v.x - (dy === 0 ? dx : dy) }));
      } else {
        setView((v) => clampPan({ ...v, x: v.x - dx, y: v.y - dy }));
      }
    };
    // Trackpad pinch in WebKit (the macOS webview): gesture events carry a scale.
    type GestureEvt = Event & { scale: number; clientX: number; clientY: number };
    const gStart = (e: Event) => { e.preventDefault(); stopZoom(); pinch.current = { s: viewRef.current.s, v: viewRef.current }; };
    const gChange = (e: Event) => {
      e.preventDefault();
      const g = e as GestureEvt, p = pinch.current;
      if (!p) return;
      const [x, y] = local(g);
      setView(zoomTo(p.v, p.s * Math.pow(g.scale, PINCH_DAMPING), x, y));
    };
    const gEnd = (e: Event) => { e.preventDefault(); pinch.current = null; };
    c.addEventListener("wheel", wheel, { passive: false });
    c.addEventListener("gesturestart", gStart);
    c.addEventListener("gesturechange", gChange);
    c.addEventListener("gestureend", gEnd);
    return () => {
      c.removeEventListener("wheel", wheel);
      c.removeEventListener("gesturestart", gStart);
      c.removeEventListener("gesturechange", gChange);
      c.removeEventListener("gestureend", gEnd);
    };
  }, [size, R, !!empty, stopZoom]);

  if (empty) return <div ref={wrap} className="circle-wrap">
    <button type="button" className="circle empty-circle" style={{ width: size, height: size }} onClick={empty.onPick}>
      <LineArt kind="sphere" size={Math.round(size * 0.22)} />
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
        role="img" tabIndex={0} aria-label={data ? `Map of ${fmt(data.points.length)} items; positions are for browsing only` : "Dataset circle"}
        onPointerDown={(e) => { stopZoom(); const [x, y] = at(e); canvas.current!.setPointerCapture(e.pointerId); drag.current = { x, y, v: view, moved: false }; }}
        onPointerMove={(e) => {
          const [x, y] = at(e); const d = drag.current;
          if (d) { if (Math.abs(x - d.x) + Math.abs(y - d.y) > 3) d.moved = true; if (d.moved) setView(clampPan({ ...d.v, x: d.v.x + x - d.x, y: d.v.y + y - d.y })); return; }
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
        onPointerLeave={() => setHover(undefined)} />

      {showLabels && !loading && [...fine.values()].sort((a, b) => b.size - a.size).slice(0, 16).map((r) => {
        const [x, y] = toScreen(r.x, r.y);
        return inside(x, y, 30) && <span key={r.region_id} className="circle-label" style={{ left: x, top: y }}>{regionName(r).slice(0, 28)}</span>;
      })}

      {(loading || (!data && idle)) && <div className="circle-center">
        <span className="circle-title">{loading ? `${job!.phase?.startsWith("Mapping") ? "Mapping" : job!.phase === "Calibrating" ? "Calibrating" : "Indexing"}… ${pct}%` : idle}</span>
        {loading && <span className="circle-sub">{job!.total ? `${fmt(job!.done)} / ${fmt(job!.total)}` : ""}{job!.phase?.startsWith("Embedding") ? " · positions are placeholders until mapping finishes" : ""}</span>}
      </div>}

      {hover && !drag.current?.moved && <div className="circle-tip" style={{ left: Math.min(hover.sx + 14, size - 230), top: hover.sy + 14 }}>
        {g && <><b>{gapCode(gaps.indexOf(g))} · {KIND_LABEL[g.kind]}</b><span>{g.label}</span><span className="dim">Score {g.score.toFixed(2)} · click to open</span></>}
        {hover.query && <><b>Your search</b><span className="dim">Approximate position: {marks.query?.label}</span></>}
        {c && c.n === 1 && <><b>{c.first.title ?? `item ${c.first.item_id}`}</b><span className="dim">{regionName(fine.get(c.first.fine_id))}{c.first.date ? ` · ${year(c.first.date)}` : ""}</span></>}
        {c && c.n > 1 && <><b>{fmt(c.n)} items</b><span className="dim">Mostly {regionName(majorityRegion)} · click to zoom</span></>}
      </div>}

      {(view.s !== 1 || view.x !== 0 || view.y !== 0) && <button type="button" className="circle-reset" onClick={() => setView({ s: 1, x: 0, y: 0 })}>Reset view</button>}
    </div>
  </div>;
}
