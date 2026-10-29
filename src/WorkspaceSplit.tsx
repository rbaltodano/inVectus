import { useEffect, useLayoutEffect, useRef, useState, type CSSProperties, type ReactNode } from "react";
import { DIVIDER_WIDTH, PANE_MIN_WIDTH, workspaceSplit } from "./workspace-split";

export default function WorkspaceSplit({ children }: { children: [ReactNode, ReactNode] }) {
  const stage = useRef<HTMLDivElement>(null);
  const drag = useRef<{ x: number; left: number; width: number }>();
  const [width, setWidth] = useState(0);
  const [ratio, setRatio] = useState(0.54);
  const [dragging, setDragging] = useState(false);
  const split = workspaceSplit(width, ratio);
  useEffect(() => { if (split.stacked) { drag.current = undefined; setDragging(false); } }, [split.stacked]);

  useLayoutEffect(() => {
    const element = stage.current!;
    const measure = () => {
      const style = getComputedStyle(element);
      setWidth(element.clientWidth - parseFloat(style.paddingLeft) - parseFloat(style.paddingRight));
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(element);
    return () => observer.disconnect();
  }, []);

  const resize = (left: number, measuredWidth = width) => {
    const next = workspaceSplit(measuredWidth, left / (measuredWidth - DIVIDER_WIDTH));
    if (!next.stacked) setRatio(next.left / next.available);
  };
  const endDrag = () => { drag.current = undefined; setDragging(false); };

  return <div ref={stage} className={`stage project-stage${dragging ? " is-resizing" : ""}`} data-stacked={split.stacked}
    style={{ "--content-width": `${split.left}px` } as CSSProperties}>
    {children[0]}
    {!split.stacked && <div className="workspace-divider" role="separator" aria-label="Resize content and map" aria-orientation="vertical"
      aria-valuemin={PANE_MIN_WIDTH} aria-valuemax={Math.floor(split.available - PANE_MIN_WIDTH)} aria-valuenow={Math.round(split.left)}
      aria-valuetext={`Content ${Math.round(split.left)} pixels, map ${Math.round(split.right)} pixels`} tabIndex={0}
      onPointerDown={(event) => {
        if (event.button !== 0) return;
        event.preventDefault(); event.currentTarget.focus();
        event.currentTarget.setPointerCapture(event.pointerId);
        drag.current = { x: event.clientX, left: split.left, width };
        setDragging(true);
      }}
      onPointerMove={(event) => { if (drag.current) resize(drag.current.left + event.clientX - drag.current.x, drag.current.width); }}
      onPointerUp={(event) => { endDrag(); if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId); }}
      onPointerCancel={endDrag} onLostPointerCapture={endDrag}
      onKeyDown={(event) => {
        const step = event.shiftKey ? 64 : 16;
        const next = event.key === "ArrowLeft" ? split.left - step : event.key === "ArrowRight" ? split.left + step
          : event.key === "Home" ? PANE_MIN_WIDTH : event.key === "End" ? split.available - PANE_MIN_WIDTH : undefined;
        if (next !== undefined) { event.preventDefault(); resize(next); }
      }} />}
    {children[1]}
  </div>;
}
