import { useLayoutEffect, useRef, useState } from "react";

type Geometry = { left: number; width: number; top: number };

/** Project and Settings tabs share the same label-sized, sliding selection indicator. */
export function useTabIndicator(selectedKey: string, revealSelected = false) {
  const tabsRef = useRef<HTMLDivElement>(null);
  const [tabIndicator, setTabIndicator] = useState<Geometry>();
  useLayoutEffect(() => {
    const tabs = tabsRef.current;
    if (!tabs) return;
    const measure = () => {
      const selected = tabs.querySelector<HTMLElement>('[aria-selected="true"]');
      if (!selected) return;
      if (revealSelected) selected.scrollIntoView({ block: "nearest", inline: "nearest" });
      const style = getComputedStyle(selected);
      const leftPadding = parseFloat(style.paddingLeft), rightPadding = parseFloat(style.paddingRight);
      const next = { left: selected.offsetLeft + leftPadding, width: selected.offsetWidth - leftPadding - rightPadding,
        top: selected.offsetTop + selected.offsetHeight - parseFloat(style.paddingBottom) + 6 };
      setTabIndicator((previous) => previous?.left === next.left && previous.width === next.width && previous.top === next.top ? previous : next);
    };
    measure();
    const observer = new ResizeObserver(measure);
    observer.observe(tabs);
    tabs.querySelectorAll('button[role="tab"]').forEach((button) => observer.observe(button));
    return () => observer.disconnect();
  }, [selectedKey, revealSelected]);
  return { tabsRef, tabIndicator };
}

export default function TabIndicator({ geometry }: { geometry?: Geometry }) {
  return geometry ? <span className="project-tab-indicator" aria-hidden="true" style={{ width: geometry.width, top: geometry.top, transform: `translateX(${geometry.left}px)` }} /> : null;
}
