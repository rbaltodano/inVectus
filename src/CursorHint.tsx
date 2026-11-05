import { useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";

/** Keep pointer help outside scroll containers; keyboard focus anchors it to the setting. */
export default function CursorHint({ children, text, className }: { children: ReactNode; text: string; className?: string }) {
  const [point, setPoint] = useState<{ x: number; y: number }>();
  const [dismissed, setDismissed] = useState(false);
  const tip = useRef<HTMLDivElement>(null);
  useLayoutEffect(() => {
    if (!point || !tip.current) return;
    const box = tip.current.getBoundingClientRect();
    tip.current.style.left = `${Math.max(8, Math.min(point.x + 12, window.innerWidth - box.width - 8))}px`;
    tip.current.style.top = `${Math.max(8, Math.min(point.y + 16, window.innerHeight - box.height - 8))}px`;
  }, [point]);
  return <div className={className}
    onPointerEnter={(e) => { if (e.pointerType !== "touch") { setDismissed(false); setPoint({ x: e.clientX, y: e.clientY }); } }}
    onPointerMove={(e) => { if (!dismissed && e.pointerType !== "touch") setPoint({ x: e.clientX, y: e.clientY }); }}
    onPointerLeave={() => setPoint(undefined)}
    onFocus={(e) => { const box = e.currentTarget.getBoundingClientRect(); setDismissed(false); setPoint({ x: box.left, y: box.bottom }); }}
    onBlur={() => setPoint(undefined)}
    onKeyDown={(e) => { if (e.key === "Escape") { setPoint(undefined); setDismissed(true); } }}>
    {children}
    {point && !dismissed && createPortal(<div ref={tip} className="cursor-hint" aria-hidden="true">{text}</div>, document.body)}
  </div>;
}
