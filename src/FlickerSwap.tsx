// Swaps flicker like the data circle's squares: when the value changes, the old one flickers out
// and the new one flickers in at the same time, in the same place. FlickerText does it letter by
// letter, each letter after its own random delay; FlickerSwap does it for any node (an icon).
import { useEffect, useRef, useState, type ReactNode } from "react";
import { DOT_DELAY_MS, DOT_FLICKER_MS } from "./dot-flicker";
import "./FlickerSwap.css";

/** How long a leaving layer stays: the latest delay plus one flicker. */
const LEAVE_MS = DOT_DELAY_MS + DOT_FLICKER_MS;
const delay = () => `${Math.round(Math.random() * DOT_DELAY_MS)}ms`;

interface Layer<T> { key: number; value: T; leaving: boolean; animate: boolean; delays: string[] }

/** The current value is always rendered live (layers[last].value is replaced on render); only
 *  leaving layers keep a snapshot of what they showed. */
function useLayers<T>(value: T, swapKey: string, count: (v: T) => number) {
  const next = useRef(1);
  const latest = useRef(value);
  const [layers, setLayers] = useState<Layer<T>[]>([{ key: 0, value, leaving: false, animate: false, delays: [] }]);
  const shown = useRef(swapKey);
  useEffect(() => {
    if (shown.current === swapKey) return;
    shown.current = swapKey;
    const before = latest.current;
    setLayers((ls) => [
      ...ls.filter((l) => l.leaving),
      { key: ls[ls.length - 1].key, value: before, leaving: true, animate: false, delays: Array.from({ length: count(before) }, delay) },
      { key: next.current++, value, leaving: false, animate: true, delays: Array.from({ length: count(value) }, delay) },
    ]);
    const t = setTimeout(() => setLayers((ls) => ls.filter((l) => !l.leaving)), LEAVE_MS);
    return () => clearTimeout(t);
  }, [swapKey]);
  // After the swap effect has read the previous value, remember this one for next time.
  useEffect(() => { latest.current = value; });
  return layers.map((l, i) => (i === layers.length - 1 && !l.leaving ? { ...l, value } : l));
}

export function FlickerText({ text }: { text: string }) {
  const layers = useLayers(text, text, (t) => Array.from(t).length);
  return <span className="fx-swap">
    {layers.map((l) => <span key={l.key} className="fx-layer" aria-hidden={l.leaving || undefined}>
      {Array.from(l.value).map((ch, i) => <span key={i} className={l.leaving ? "fx-out" : l.animate ? "fx-in" : undefined}
        style={l.leaving || l.animate ? { animationDelay: l.delays[i] } : undefined}>{ch === " " ? " " : ch}</span>)}
    </span>)}
  </span>;
}

export function FlickerSwap({ swapKey, children }: { swapKey: string; children: ReactNode }) {
  const layers = useLayers(children, swapKey, () => 1);
  return <span className="fx-swap">
    {layers.map((l) => <span key={l.key} className={`fx-layer ${l.leaving ? "fx-out" : l.animate ? "fx-in" : ""}`} aria-hidden={l.leaving || undefined}
      style={l.leaving || l.animate ? { animationDelay: l.delays[0] } : undefined}>{l.value}</span>)}
  </span>;
}
