import { useEffect, useMemo, useRef, useState, type PointerEvent as PE } from "react";
import { api, fmt, year, KIND_LABEL, type GapView, type MapData, type MapPoint } from "../api";
import { ErrorLine, Needs, useAction, type Ctx } from "../App";
import Link from "../Link";

type ColourBy = "region" | "role" | "date" | "gap";
const PALETTE = ["#2F5BD3", "#D9822B", "#1E8A4C", "#7B4BC2", "#B03A6A", "#2A8C99", "#8C6D1F", "#C2463A", "#5470A8", "#6A9A2E", "#9C4FB0", "#3E7C5A"];
const ROLE_COLOUR = { demand: "#D9822B", coverage: "#7B4BC2", neutral: "#A3A39C" };
const GRID = 128;
const PAD = 28;
const MARKERS = 15;

interface View { scale: number; x: number; y: number }

export default function MapView({ ctx, focus }: { ctx: Ctx; focus?: number }) {
  const { table, tref } = ctx;
  const [data, setData] = useState<MapData>();
  const [gaps, setGaps] = useState<GapView[]>([]);
  const [colourBy, setColourBy] = useState<ColourBy>("region");
  const [showGaps, setShowGaps] = useState(true);
  const [showLabels, setShowLabels] = useState(true);
  const [boxMode, setBoxMode] = useState(false);
  const [view, setView] = useState<View>({ scale: 1, x: 0, y: 0 });
  const [selection, setSelection] = useState<MapPoint[]>([]);
  const [hover, setHover] = useState<{ p: MapPoint; x: number; y: number }>();
  const [box, setBox] = useState<{ x0: number; y0: number; x1: number; y1: number }>();
  const canvas = useRef<HTMLCanvasElement>(null);
  const wrap = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; y: number; view: View; moved: boolean }>();
  const [size, setSize] = useState({ w: 800, h: 560 });
  const { busy, error, run } = useAction();

  useEffect(() => {
    if (!tref || !table?.regions) return;
    run(async () => {
      setData(await api.map(tref));
      if (table.regions?.scanned) setGaps((await api.gaps(tref)).gaps);
    });
  }, [tref?.collection, tref?.space, table?.regions?.run_id]);

  useEffect(() => {
    const el = wrap.current;
    if (!el) return;
    const ro = new ResizeObserver(() => setSize({ w: el.clientWidth, h: Math.max(420, Math.min(720, el.clientWidth * 0.68)) }));
    ro.observe(el);
    return () => ro.disconnect();
  }, [data]);

  // Data extent, fitted and centred in the canvas; the view then pans and zooms that fit.
  const extent = useMemo(() => {
    let x0 = Infinity, y0 = Infinity, x1 = -Infinity, y1 = -Infinity;
    for (const p of data?.points ?? []) { x0 = Math.min(x0, p.x); x1 = Math.max(x1, p.x); y0 = Math.min(y0, p.y); y1 = Math.max(y1, p.y); }
    return x0 <= x1 ? { x0, y0, dx: x1 - x0 || 1, dy: y1 - y0 || 1 } : { x0: 0, y0: 0, dx: 1, dy: 1 };
  }, [data]);
  const fit = useMemo(() => {
    const k = Math.min((size.w - PAD * 2) / extent.dx, (size.h - PAD * 2) / extent.dy);
    return { k, ox: (size.w - extent.dx * k) / 2, oy: (size.h - extent.dy * k) / 2 };
  }, [extent, size]);
  const toScreen = (x: number, y: number) =>
    [fit.ox + (x - extent.x0) * fit.k * view.scale + view.x, fit.oy + (y - extent.y0) * fit.k * view.scale + view.y] as const;

  const fine = useMemo(() => (data?.regions ?? []).filter((r) => r.level === "fine"), [data]);
  const bySize = useMemo(() => [...fine].sort((a, b) => b.size - a.size), [fine]);
  const byRegion = useMemo(() => new Map(fine.map((r) => [r.region_id, r])), [fine]);
  // Largest regions get distinct colours; the rest cycle through the palette.
  const colourOfRegion = useMemo(() => new Map(bySize.map((r, i) => [r.region_id, PALETTE[i % PALETTE.length]])), [bySize]);
  const gapScore = useMemo(() => {
    const m = new Map<number, number>();
    const max = gaps.reduce((a, g) => Math.max(a, g.score), 1e-9);
    gaps.forEach((g) => [g.region_a, g.region_b].forEach((r) => { if (r !== null) m.set(r, Math.max(m.get(r) ?? 0, Math.max(0, g.score) / max)); }));
    return m;
  }, [gaps]);
  const years = useMemo(() => {
    let min = Infinity, max = -Infinity;
    for (const p of data?.points ?? []) { const y = Number(year(p.date)); if (y > 0) { min = Math.min(min, y); max = Math.max(max, y); } }
    return min <= max ? { min, max } : undefined;
  }, [data]);

  const colour = (p: MapPoint) => {
    switch (colourBy) {
      case "region": return p.noise ? "#C9C9C4" : colourOfRegion.get(p.fine_id) ?? "#A3A39C";
      case "role": return ROLE_COLOUR[p.role] ?? "#A3A39C";
      case "date": {
        const y = Number(year(p.date));
        if (!y || !years) return "#DADAD6";
        const t = (y - years.min) / Math.max(1, years.max - years.min);
        return `hsl(${220 - t * 190}, 65%, ${55 - t * 10}%)`;
      }
      case "gap": { const s = gapScore.get(p.fine_id) ?? 0; return s > 0 ? `rgba(194, 70, 58, ${0.25 + s * 0.75})` : "#DADAD6"; }
    }
  };

  // Spatial grid over data space for hover and click lookups.
  const cell = (x: number, y: number) => [Math.floor(((x - extent.x0) / extent.dx) * GRID), Math.floor(((y - extent.y0) / extent.dy) * GRID)] as const;
  const grid = useMemo(() => {
    const g = new Map<number, MapPoint[]>();
    for (const p of data?.points ?? []) {
      const [i, j] = cell(p.x, p.y);
      const k = Math.min(GRID - 1, i) * GRID + Math.min(GRID - 1, j);
      (g.get(k) ?? g.set(k, []).get(k)!).push(p);
    }
    return g;
  }, [data, extent]);
  const nearest = (sx: number, sy: number) => {
    const s = fit.k * view.scale;
    const [ci, cj] = cell(extent.x0 + (sx - fit.ox - view.x) / s, extent.y0 + (sy - fit.oy - view.y) / s);
    const ri = Math.ceil((8 / s / extent.dx) * GRID) + 1, rj = Math.ceil((8 / s / extent.dy) * GRID) + 1;
    let best: MapPoint | undefined, bd = 64;
    for (let i = ci - ri; i <= ci + ri; i++) for (let j = cj - rj; j <= cj + rj; j++) {
      for (const p of grid.get(i * GRID + j) ?? []) {
        const [px, py] = toScreen(p.x, p.y);
        const d = (px - sx) ** 2 + (py - sy) ** 2;
        if (d < bd) { bd = d; best = p; }
      }
    }
    return best;
  };

  useEffect(() => {
    const c = canvas.current;
    if (!c || !data) return;
    const dpr = window.devicePixelRatio || 1;
    c.width = size.w * dpr; c.height = size.h * dpr;
    const g = c.getContext("2d")!;
    g.scale(dpr, dpr);
    g.clearRect(0, 0, size.w, size.h);
    const n = data.points.length;
    const dot = (n > 100_000 ? 1.4 : n > 20_000 ? 2.2 : 3.5) * Math.min(2, Math.sqrt(view.scale));
    for (const p of data.points) {
      const [x, y] = toScreen(p.x, p.y);
      if (x < -4 || y < -4 || x > size.w + 4 || y > size.h + 4) continue;
      g.fillStyle = colour(p);
      g.fillRect(x - dot / 2, y - dot / 2, dot, dot);
    }
    const sel = new Set(selection.map((p) => p.item_id));
    g.strokeStyle = "#1C1C1A"; g.lineWidth = 1.5;
    for (const p of data.points) if (sel.has(p.item_id)) { const [x, y] = toScreen(p.x, p.y); g.strokeRect(x - 4, y - 4, 8, 8); }
    const r = focus !== undefined ? byRegion.get(focus) : undefined;
    if (r) {
      const [x, y] = toScreen(r.x, r.y);
      g.beginPath(); g.arc(x, y, Math.max(16, r.radius * fit.k * view.scale), 0, Math.PI * 2);
      g.strokeStyle = "#1C1C1A"; g.setLineDash([5, 4]); g.stroke(); g.setLineDash([]);
    }
  }, [data, size, view, colourBy, selection, gapScore, focus, fit]);

  // Labels for the largest regions that fit without overlapping (estimated text boxes).
  const labels = useMemo(() => {
    if (!showLabels) return [];
    const placed: { x: number; y: number; w: number }[] = [];
    const out: { id: number; x: number; y: number; text: string; title: string }[] = [];
    for (const r of bySize) {
      if (out.length >= 24) break;
      const text = r.llm_label ?? r.label.split(" · ").slice(0, 2).join(" · ");
      const [x, y] = toScreen(r.x, r.y);
      const w = Math.min(180, text.length * 6.2 + 14);
      if (x < w / 2 || x > size.w - w / 2 || y < 12 || y > size.h - 12) continue;
      if (placed.some((p) => Math.abs(p.x - x) < (p.w + w) / 2 + 4 && Math.abs(p.y - y) < 22)) continue;
      placed.push({ x, y, w });
      out.push({ id: r.region_id, x, y, text, title: `${r.label} · ${fmt(r.size)} items${r.stability !== null && r.stability < 0.5 ? " · unstable" : ""}` });
    }
    return out;
  }, [bySize, showLabels, view, fit, size]);

  if (!table || !tref) return <div className="page narrow"><Needs title="No map yet" body="Embed a collection first." action="Go to Index" onAction={() => ctx.go({ name: "index" })} /></div>;
  if (!table.regions) return <div className="page narrow"><Needs title="No regions yet" body="Find regions to group items by topic and lay out the map." action="Go to Index" onAction={() => ctx.go({ name: "index", collection: table.collection })} /></div>;

  const local = (e: PE) => { const r = canvas.current!.getBoundingClientRect(); return [e.clientX - r.left, e.clientY - r.top] as const; };
  const down = (e: PE) => {
    const [x, y] = local(e);
    canvas.current!.setPointerCapture(e.pointerId);
    if (boxMode || e.shiftKey) setBox({ x0: x, y0: y, x1: x, y1: y });
    else drag.current = { x, y, view, moved: false };
  };
  const move = (e: PE) => {
    const [x, y] = local(e);
    if (box) { setBox({ ...box, x1: x, y1: y }); return; }
    const d = drag.current;
    if (d) {
      if (Math.abs(x - d.x) + Math.abs(y - d.y) > 3) d.moved = true;
      if (d.moved) setView({ ...d.view, x: d.view.x + x - d.x, y: d.view.y + y - d.y });
      return;
    }
    const p = nearest(x, y);
    setHover(p ? { p, x, y } : undefined);
  };
  const up = (e: PE) => {
    const [x, y] = local(e);
    if (box) {
      const [ax, bx] = [Math.min(box.x0, x), Math.max(box.x0, x)], [ay, by] = [Math.min(box.y0, y), Math.max(box.y0, y)];
      setSelection(data!.points.filter((p) => { const [px, py] = toScreen(p.x, p.y); return px >= ax && px <= bx && py >= ay && py <= by; }));
      setBox(undefined);
    } else if (drag.current && !drag.current.moved) {
      const p = nearest(x, y);
      setSelection(p ? (e.metaKey || e.ctrlKey ? [...selection, p] : [p]) : []);
    }
    drag.current = undefined;
  };
  const zoom = (f: number, cx = size.w / 2, cy = size.h / 2) =>
    setView((v) => {
      const s = Math.min(64, Math.max(0.5, v.scale * f)), k = s / v.scale;
      return { scale: s, x: cx - fit.ox - (cx - fit.ox - v.x) * k, y: cy - fit.oy - (cy - fit.oy - v.y) * k };
    });

  const name = (id: number) => { const r = byRegion.get(id); return r ? r.llm_label ?? r.label : `region ${id}`; };
  const markers = showGaps ? gaps.slice(0, MARKERS).flatMap((g) => {
    const a = byRegion.get(g.region_a), b = g.region_b !== null ? byRegion.get(g.region_b) : undefined;
    if (!a) return [];
    const [x, y] = b ? toScreen((a.x + b.x) / 2, (a.y + b.y) / 2) : toScreen(a.x, a.y);
    return x > 0 && x < size.w && y > 0 && y < size.h ? [{ g, x, y }] : [];
  }) : [];

  return <div className="page wide">
    <div className="row wrap toolbar">
      <span className="muted small">Colour by</span>
      <div role="radiogroup" aria-label="Colour by" className="segmented">
        {(["region", "role", "date", "gap"] as ColourBy[]).map((c) => <button key={c} type="button" role="radio" aria-checked={colourBy === c}
          disabled={c === "gap" && !gaps.length} onClick={() => setColourBy(c)}>{c === "gap" ? "Gap score" : c[0].toUpperCase() + c.slice(1)}</button>)}
      </div>
      <label className="check"><input type="checkbox" checked={showGaps} onChange={(e) => setShowGaps(e.target.checked)} disabled={!gaps.length} />Top {MARKERS} gaps</label>
      <label className="check"><input type="checkbox" checked={showLabels} onChange={(e) => setShowLabels(e.target.checked)} />Labels</label>
      <button type="button" className="pill" aria-pressed={boxMode} onClick={() => setBoxMode(!boxMode)}>Box select</button>
      {data && <span className="muted small push">{fmt(data.points.length)} items · {fmt(fine.length)} regions</span>}
    </div>
    <ErrorLine error={error} />
    {busy && !data && <p className="muted">Loading the map…</p>}

    {data && <div className="map-layout">
      <div className="stack gap-s map-col">
        <div ref={wrap} className="map-wrap" style={{ height: size.h }}>
          <canvas ref={canvas} role="img" aria-label="Map of all items, clustered into regions" style={{ width: size.w, height: size.h, cursor: boxMode ? "crosshair" : "grab" }}
            onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerLeave={() => setHover(undefined)}
            onWheel={(e) => { const r = canvas.current!.getBoundingClientRect(); zoom(e.deltaY < 0 ? 1.15 : 1 / 1.15, e.clientX - r.left, e.clientY - r.top); }} />
          {labels.map((l) => <button key={l.id} type="button" className="map-label" style={{ left: l.x, top: l.y }} title={l.title}
            onClick={() => ctx.go({ name: "search", query: byRegion.get(l.id)!.terms.slice(0, 5).join(" ") })}>{l.text}</button>)}
          {markers.map(({ g, x, y }) => <button key={g.gap_id} type="button" className="gap-marker" style={{ left: x, top: y }}
            aria-label={`Gap: ${KIND_LABEL[g.kind]} · ${g.label}`} title={`${KIND_LABEL[g.kind]} · ${g.label}`} onClick={() => ctx.go({ name: "gap", id: g.gap_id })} />)}
          {box && <div className="box" style={{ left: Math.min(box.x0, box.x1), top: Math.min(box.y0, box.y1), width: Math.abs(box.x1 - box.x0), height: Math.abs(box.y1 - box.y0) }} />}
          {hover && !box && <div className="tooltip" style={{ left: Math.min(hover.x + 12, size.w - 240), top: hover.y + 12 }}>
            <b>{hover.p.title ?? `item ${hover.p.item_id}`}</b><div className="muted small">{name(hover.p.fine_id)} · {hover.p.role}{hover.p.date ? ` · ${year(hover.p.date)}` : ""}</div></div>}
          <div className="zoom">
            <button type="button" aria-label="Zoom in" onClick={() => zoom(1.5)}>+</button>
            <button type="button" aria-label="Zoom out" onClick={() => zoom(1 / 1.5)}>−</button>
            <button type="button" aria-label="Reset view" onClick={() => setView({ scale: 1, x: 0, y: 0 })}>⟲</button>
          </div>
        </div>
        <div className="legend">
          {colourBy === "region" && <><b>Largest regions</b>{bySize.slice(0, 8).map((r) =>
            <span key={r.region_id}><i style={{ background: colourOfRegion.get(r.region_id) }} />{(r.llm_label ?? r.label).slice(0, 40)}</span>)}</>}
          {colourBy === "role" && <><b>Role</b>{Object.entries(ROLE_COLOUR).map(([k, c]) => <span key={k}><i style={{ background: c }} />{k}</span>)}</>}
          {colourBy === "date" && years && <><b>Year</b><span><i style={{ background: "hsl(220,65%,55%)" }} />{years.min}</span><span><i style={{ background: "hsl(30,65%,45%)" }} />{years.max}</span></>}
          {colourBy === "gap" && <><b>Gap score</b><span><i style={{ background: "rgba(194,70,58,1)" }} />highest</span><span><i style={{ background: "#DADAD6" }} />no gap</span></>}
          {markers.length > 0 && <span><i className="gap-dot" />Gap</span>}
        </div>
      </div>

      <aside aria-labelledby="sel" className="card stack gap">
        <h2 id="sel">Selection · {fmt(selection.length)} items</h2>
        <div className="muted small">Click a point; ⌘-click adds. Shift-drag or Box select picks an area. Scroll to zoom, drag to pan.</div>
        <ul className="plain">{selection.slice(0, 50).map((p) => <li key={p.item_id}>
          <Link url={p.url}>{p.title ?? `item ${p.item_id}`}</Link>
          <div className="muted small">{name(p.fine_id)}{p.noise ? " (loosely attached)" : ""} · {p.role}{p.date ? ` · ${year(p.date)}` : ""}</div>
        </li>)}</ul>
        {selection.length > 50 && <div className="muted small">+ {fmt(selection.length - 50)} more</div>}
        {selection.length > 0 && <div className="row wrap">
          <button type="button" className="btn" onClick={() => ctx.go({ name: "search", query: selection[0].title ?? "" })}>Search near {selection.length > 1 ? "the first" : "this"}</button>
          <button type="button" className="btn" onClick={() => setSelection([])}>Clear</button>
        </div>}
        <div className="muted small">{data.note}</div>
      </aside>
    </div>}
  </div>;
}
