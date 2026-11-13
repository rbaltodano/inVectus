// Popup windows leave the way the data circle's squares do: the whole window, backdrop included,
// flickers out (levels 1, .5, .7, .35, .85, .2, 0 over 150 ms) before it is removed, however it
// closed (Cancel, Escape, a click outside, or the work finishing). Settings → Window flicker turns
// it off.
//
//   <FlickerPresence show={open}>{open && <ModalBackdrop …>…</ModalBackdrop>}</FlickerPresence>
//
// FlickerPresence keeps the last shown children mounted for one flicker after `show` turns off
// (same component instance, so its state holds); ModalBackdrop reads that and plays the exit.
import { createContext, useContext, useEffect, useRef, useState, type HTMLAttributes, type ReactNode } from "react";
import { DOT_FLICKER_MS, noFlicker } from "./dot-flicker";

const Leaving = createContext(false);

export function FlickerPresence({ show, children }: { show: boolean; children: ReactNode }) {
  const last = useRef<ReactNode>(null);
  const [leaving, setLeaving] = useState(false);
  const [prevShow, setPrevShow] = useState(show);
  // Derived during render, so the first frame without `show` already plays the exit (no blink).
  if (show !== prevShow) {
    setPrevShow(show);
    setLeaving(!show && !noFlicker("windows"));
  }
  if (show) last.current = children;
  useEffect(() => {
    if (!leaving) return;
    const t = setTimeout(() => setLeaving(false), DOT_FLICKER_MS);
    return () => clearTimeout(t);
  }, [leaving]);
  if (show) return <Leaving.Provider value={false}>{children}</Leaving.Provider>;
  if (!leaving) return null;
  return <Leaving.Provider value={true}>{last.current}</Leaving.Provider>;
}

/** The dimmed layer behind a popup window; flickers out (with its window) while leaving. */
export function ModalBackdrop({ className = "", children, ...rest }: HTMLAttributes<HTMLDivElement> & { children: ReactNode }) {
  const leaving = useContext(Leaving);
  return <div {...rest} className={`integration-modal-backdrop ${className}${leaving ? " is-leaving" : ""}`.trim()}
    aria-hidden={leaving || undefined}>{children}</div>;
}
