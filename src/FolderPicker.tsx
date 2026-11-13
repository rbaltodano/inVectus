// Settings → Storage → Agent folders: choose a file or folder AI agents may read, on a canvas.
// The folder sits at the left and what it holds branches out to the right (three levels); drag
// to look around (the dotted grid moves with it, 1:1). Chips flicker in, roughly left to right. Clicking one lights it and everything
// inside it (light chips, one level after another) while the rest drops to 25%; allowing it
// flickers the rest away, and a second later the window closes (FlickerPresence flickers it out).
// Double-click a folder to open it; "Up a level" and the system's own picker reach anywhere else
// (a folder chosen there opens already selected). The path box shows what is chosen, and takes a
// pasted path (Enter opens it). "Set as default" makes the picker open there next time.
import { useEffect, useLayoutEffect, useMemo, useRef, useState, type CSSProperties, type PointerEvent as ReactPointerEvent } from "react";
import { createPortal } from "react-dom";
import { ArrowUp, File, Folder } from "lucide-react";
import { api, local, pickDirectory, type FolderNode, type FolderView } from "./api";
import { ModalBackdrop } from "./Modal";
import "./FolderPicker.css";

const ROW = 40, PAD = 28, GAP = 64, CHIP_H = 32;
/** How long the window stays after "Allow" before it closes. */
const CLOSE_AFTER_MS = 1000;

interface Placed { node: FolderNode; key: string; parent?: string; depth: number; x: number; y: number; w: number; more?: number }

/** Until a chip is drawn and measured, a rough width from its label. */
const estimate = (label: string, badge: string) => Math.min(300, 46 + label.length * 7.2 + (badge ? badge.length * 6 + 8 : 0));

/** Leaves get evenly spaced rows; a folder sits at the middle of what it holds. Each level is a
 *  column as wide as its widest chip. */
function layout(root: FolderNode, widths: Map<string, number>): Placed[] {
  const chipWidth = (key: string, label: string, badge: string) => widths.get(key) ?? estimate(label, badge);
  const out: Placed[] = [];
  let row = 0;
  const place = (node: FolderNode, depth: number, parent?: string): Placed => {
    const p: Placed = { node, key: node.path, parent, depth, x: 0, y: 0, w: chipWidth(node.path, node.name, node.dir ? String(node.count) : "") };
    out.push(p);
    const kids = node.children.map((c) => place(c, depth + 1, node.path));
    if (node.children.length && node.more > 0) {
      const label = `${node.more} more`;
      const m: Placed = { node: { name: label, path: `${node.path}\0more`, dir: false, count: 0, children: [], more: 0 }, key: `${node.path}\0more`, parent: node.path, depth: depth + 1, x: 0, y: PAD + row * ROW + CHIP_H / 2, w: chipWidth(`${node.path}\0more`, label, ""), more: node.more };
      row++;
      out.push(m);
      kids.push(m);
    }
    if (!kids.length) { p.y = PAD + row * ROW + CHIP_H / 2; row++; }
    else p.y = (kids[0].y + kids[kids.length - 1].y) / 2;
    return p;
  };
  place(root, 0);
  const widest: number[] = [];
  for (const p of out) widest[p.depth] = Math.max(widest[p.depth] ?? 0, p.w);
  const xs = widest.reduce<number[]>((acc, _, d) => [...acc, d === 0 ? PAD : acc[d - 1] + widest[d - 1] + GAP], []);
  for (const p of out) p.x = xs[p.depth];
  return out;
}

const home = (p: string) => p.replace(/^\/Users\/[^/]+|^\/home\/[^/]+/, "~");
/** Where the picker opens ("Set as default"; this computer only). */
const START_KEY = "invectus.agent-picker-start";
const trimPath = (p: string) => { const t = p.trim(); return t.length > 1 ? t.replace(/\/+$/, "") : t; };
const parentOf = (p: string) => p.slice(0, Math.max(1, p.lastIndexOf("/")));

export default function FolderPicker({ onAllowed, onClose }: { onAllowed: (path: string) => void; onClose: () => void }) {
  const [view, setView] = useState<FolderView>();
  const [error, setError] = useState<string>();
  // The path box: filled by clicking a chip, or typed/pasted. A chip is chosen when it matches.
  const [path, setPath] = useState("");
  const [asDefault, setAsDefault] = useState(false);
  const [touched, setTouched] = useState(false);
  const [phase, setPhase] = useState<"choosing" | "allowing" | "allowed">("choosing");
  const [pan, setPan] = useState({ x: 0, y: 0 });
  const drag = useRef<{ x: number; y: number; px: number; py: number; moved: boolean }>();
  const canvas = useRef<HTMLDivElement>(null);

  /** Show `dir` (default: the saved start, else the projects folder); `select` chooses it too. */
  const load = async (dir?: string, select = false) => {
    setError(undefined);
    try {
      const v = await api.folderTree(dir);
      setView(v); setPan({ x: 0, y: 0 }); setPath(select ? v.tree.path : "");
      return true;
    } catch (e) { setError(String(e)); return false; }
  };
  useEffect(() => {
    const start = local.get<string | null>(START_KEY, null);
    void (start ? load(start).then((ok) => { if (!ok) void load(); }) : load());
  }, []);
  /** A typed or pasted path: open the folder selected, or a file's folder with the file chosen. */
  const goTo = async () => {
    const want = trimPath(path);
    if (!want) return;
    if (await load(want, true)) return;
    const shown = await api.folderTree(parentOf(want)).catch(() => undefined);
    if (shown) { setView(shown); setPan({ x: 0, y: 0 }); setPath(want); setError(undefined); }
    else setError(`Nothing to show at ${want}`);
  };
  useEffect(() => {
    const key = (e: KeyboardEvent) => { if (e.key === "Escape" && phase === "choosing") onClose(); };
    document.addEventListener("keydown", key);
    return () => document.removeEventListener("keydown", key);
  }, [onClose, phase]);

  // Chips size to their names; once drawn, their widths set the columns (before paint).
  const [widths, setWidths] = useState<Map<string, number>>(new Map());
  const world = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    const m = new Map<string, number>();
    world.current?.querySelectorAll<HTMLElement>("[data-key]").forEach((el) => m.set(el.dataset.key!, el.offsetWidth));
    if (m.size && [...m].some(([k, w]) => widths.get(k) !== w)) setWidths(m);
  }, [view, widths]);
  const placed = useMemo(() => (view ? layout(view.tree, widths) : []), [view, widths]);
  const byKey = useMemo(() => new Map(placed.map((p) => [p.key, p])), [placed]);
  const want = trimPath(path);
  const chosen = byKey.has(want) && !byKey.get(want)!.more ? want : undefined;
  // Entrance: each chip after its own delay, deeper levels later (left to right).
  const enter = useMemo(() => new Map(placed.map((p) => [p.key, 80 + p.depth * 110 + Math.random() * 380])), [placed]);
  const inside = useMemo(() => {
    const set = new Set<string>();
    if (!chosen) return set;
    for (const p of placed) {
      let k: string | undefined = p.key;
      while (k && k !== chosen) k = byKey.get(k)?.parent;
      if (k === chosen) set.add(p.key);
    }
    return set;
  }, [chosen, placed, byKey]);
  const chosenNode = chosen ? byKey.get(chosen) : undefined;
  const litDelay = useMemo(() => new Map(placed.map((p) => [p.key, Math.random() * 60])), [placed, chosen]);
  const size = useMemo(() => ({
    w: Math.max(...placed.map((p) => p.x + p.w), 0) + PAD * 2,
    h: Math.max(...placed.map((p) => p.y), 0) + CHIP_H + PAD,
  }), [placed]);

  const down = (e: ReactPointerEvent) => {
    if (e.button !== 0) return;
    drag.current = { x: e.clientX, y: e.clientY, px: pan.x, py: pan.y, moved: false };
  };
  const move = (e: ReactPointerEvent) => {
    const d = drag.current;
    if (!d) return;
    const dx = e.clientX - d.x, dy = e.clientY - d.y;
    if (!d.moved && Math.hypot(dx, dy) < 4) return;
    if (!d.moved) { d.moved = true; canvas.current?.setPointerCapture(e.pointerId); }
    const box = canvas.current?.getBoundingClientRect();
    const minX = box ? Math.min(0, box.width - size.w) : 0, minY = box ? Math.min(0, box.height - size.h) : 0;
    setPan({ x: Math.max(minX - 80, Math.min(80, d.px + dx)), y: Math.max(minY - 80, Math.min(80, d.py + dy)) });
  };
  const up = () => { setTimeout(() => { drag.current = undefined; }, 0); };
  const choose = (p: Placed) => {
    if (drag.current?.moved || phase !== "choosing" || p.more) return;
    setPath(p.key === chosen ? "" : p.key);
  };

  const allow = async () => {
    if (!want) return;
    setPhase("allowing"); setError(undefined);
    try {
      const real = await api.allowAgentFolder(want);
      if (asDefault) local.set(START_KEY, byKey.get(want)?.node.dir === false ? parentOf(real) : real);
      setPhase("allowed");
      onAllowed(real);
      setTimeout(onClose, CLOSE_AFTER_MS);
    } catch (e) { setError(String(e)); setPhase("choosing"); }
  };
  const browse = async () => {
    const p = await pickDirectory(view?.tree.path, "Choose a folder for agents");
    if (p) void load(p, true);
  };

  const state = (key: string) => !chosen ? "" : inside.has(key) ? " lit" : phase === "allowed" ? " gone" : " dim";
  const nodeStyle = (p: Placed) => ({
    left: p.x, top: p.y,
    "--card-delay": `${enter.get(p.key) ?? 0}ms`,
    "--lit-delay": chosenNode ? `${(p.depth - chosenNode.depth) * 180 + (p.key === chosen ? 0 : litDelay.get(p.key) ?? 0)}ms` : "0ms",
  }) as CSSProperties;
  // A line belongs to its child; the line into the chosen chip goes with the rest.
  const edgeState = (p: Placed) => !chosen ? "" : inside.has(p.key) && p.key !== chosen ? "" : phase === "allowed" ? " gone" : " dim";

  return createPortal(<ModalBackdrop onClick={(e) => { if (e.target === e.currentTarget && phase === "choosing") onClose(); }}>
    <div className="integration-modal fp-modal" role="dialog" aria-modal="true" aria-labelledby="fp-title">
      <div className="fp-head">
        <h3 id="fp-title">Agent Access</h3>
        <div className="fp-path-row">
          <input className="fp-path mono" value={path} spellCheck={false} placeholder="Paste a path, or click a file or folder below" aria-label="Path agents may read (and everything inside)"
            disabled={phase !== "choosing"} onChange={(e) => setPath(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); void goTo(); } }} />
          <label className="fp-default small" title="Open the picker here next time">
            <input type="checkbox" checked={asDefault} disabled={phase !== "choosing"} onChange={(e) => { setAsDefault(e.target.checked); setTouched(true); }} />
            <span className={`fp-default-box${touched ? " touched" : ""}`} aria-hidden="true" />Set as default
          </label>
        </div>
      </div>
      <div ref={canvas} className="fp-canvas" style={{ backgroundPosition: `${pan.x}px ${pan.y}px` }} onPointerDown={down} onPointerMove={move} onPointerUp={up} onPointerCancel={up}>
        {view && <div ref={world} className="fp-world" style={{ transform: `translate(${pan.x}px, ${pan.y}px)`, width: size.w, height: size.h }}>
          <svg className="fp-edges" width={size.w} height={size.h} aria-hidden="true">
            {placed.filter((p) => p.parent).map((p) => {
              const a = byKey.get(p.parent!)!;
              const x1 = a.x + a.w, x2 = p.x, mx = (x1 + x2) / 2;
              return <path key={p.key} className={`fp-edge${edgeState(p)}`} style={{ "--card-delay": `${(enter.get(p.key) ?? 0) - 40}ms` } as CSSProperties}
                d={`M${x1} ${a.y} C${mx} ${a.y}, ${mx} ${p.y}, ${x2} ${p.y}`} />;
            })}
          </svg>
          {placed.map((p) => p.more
            ? <span key={p.key} data-key={p.key} className={`fp-node fp-more${state(p.key)}`} style={nodeStyle(p)}>{p.node.name}</span>
            : <button key={p.key} data-key={p.key} type="button" className={`fp-node${p.node.dir ? " dir" : ""}${p.depth === 0 ? " root" : ""}${state(p.key)}`} style={nodeStyle(p)}
              aria-pressed={p.key === chosen} title={home(p.node.path)} disabled={phase !== "choosing" && !inside.has(p.key)}
              onClick={() => choose(p)} onDoubleClick={() => { if (p.node.dir && p.depth > 0 && phase === "choosing") load(p.node.path); }}>
              {p.node.dir ? <Folder size={14} strokeWidth={1.5} aria-hidden="true" /> : <File size={14} strokeWidth={1.5} aria-hidden="true" />}
              <span className="fp-name">{p.node.name}</span>
              {p.node.dir && <span className="fp-count">{p.node.count}</span>}
            </button>)}
        </div>}
        {!view && !error && <p className="fp-empty dim small">Reading folders…</p>}
        {view && <span className="fp-hint small">Drag to look around · double-click a folder to open it</span>}
        {view && <span className="fp-where mono">{home(view.tree.path)}</span>}
      </div>
      {error && <p className="err small" role="alert">{error}</p>}
      <div className="fp-foot">
        <span className="fp-pick small dim">{chosenNode ? (chosenNode.node.dir ? `${chosenNode.node.name}: ${chosenNode.node.count} inside` : `${chosenNode.node.name}: file`) : ""}</span>
        <button type="button" className="text-btn" disabled={!view?.parent || phase !== "choosing"} onClick={() => load(view?.parent ?? undefined)}><ArrowUp size={13} aria-hidden="true" /> Up a level</button>
        <button type="button" className="text-btn" disabled={phase !== "choosing"} onClick={browse}>Browse…</button>
        <button type="button" className="btn" disabled={phase !== "choosing"} onClick={onClose}>Cancel</button>
        <button type="button" className="btn primary" disabled={!want || phase !== "choosing"} onClick={allow}>{phase === "allowed" ? "Allowed" : phase === "allowing" ? "Allowing…" : "Allow agents"}</button>
      </div>
    </div>
  </ModalBackdrop>, document.body);
}
