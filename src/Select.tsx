// The app's dropdown, in the style of the agent menu: a trigger that shows the current choice and
// a list where each option can carry a line of detail (a count, a sample, what it means) and a
// figure on the right. Keyboard: arrows move, Enter or Space picks, Escape closes.
import { useEffect, useId, useLayoutEffect, useRef, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Check, ChevronDown } from "lucide-react";
import { useFlickerDelays } from "./dot-flicker";
import "./Select.css";

export interface SelectOption<T extends string> {
  value: T; label: string;
  /** A line of detail under the label. */
  note?: string;
  /** A short figure on the right, e.g. a count. */
  meta?: string;
  icon?: ReactNode;
  disabled?: boolean;
}

export default function Select<T extends string>({ value, options, onChange, disabled = false, variant = "field", id, ariaLabel, title, menuTitle }: {
  value: T; options: SelectOption<T>[]; onChange: (value: T) => void; disabled?: boolean;
  /** `field` looks like an input; `bare` takes its look from the surrounding control. */
  variant?: "field" | "bare"; id?: string; ariaLabel?: string; title?: string;
  /** A heading at the top of the list. */
  menuTitle?: string;
}) {
  const [open, setOpen] = useState(false);
  const [active, setActive] = useState(0);
  const [place, setPlace] = useState({ top: 0, left: 0, width: 0, maxHeight: 360 });
  const trigger = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLDivElement>(null);
  const listId = useId();
  const flicker = useFlickerDelays();
  const current = options.find((o) => o.value === value);

  const enabled = (i: number) => !!options[i] && !options[i].disabled;
  const step = (from: number, dir: 1 | -1) => {
    for (let i = from + dir, n = 0; n < options.length; i += dir, n++) {
      const j = (i + options.length) % options.length;
      if (enabled(j)) return j;
    }
    return from;
  };
  const show = () => { if (disabled) return; setActive(Math.max(0, options.findIndex((o) => o.value === value))); setOpen(true); };
  const pick = (i: number) => { if (!enabled(i)) return; onChange(options[i].value); setOpen(false); trigger.current?.focus(); };

  useLayoutEffect(() => {
    if (!open) return;
    const place = () => {
      const r = trigger.current!.getBoundingClientRect();
      const below = window.innerHeight - r.bottom - 12, above = r.top - 12;
      const maxHeight = Math.min(360, Math.max(below, above));
      // Triggers on the right half open their list aligned to their right edge.
      const width = Math.min(Math.max(r.width, 240), window.innerWidth - 16);
      setPlace({ top: below >= Math.min(maxHeight, 200) ? r.bottom + 6 : r.top - 6 - maxHeight, left: Math.max(8, Math.min(r.left > window.innerWidth / 2 ? r.right - width : r.left, window.innerWidth - width - 8)), width, maxHeight });
    };
    place();
    window.addEventListener("resize", place);
    window.addEventListener("scroll", place, true);
    return () => { window.removeEventListener("resize", place); window.removeEventListener("scroll", place, true); };
  }, [open]);
  useEffect(() => {
    if (!open) return;
    const outside = (e: PointerEvent) => { if (!menu.current?.contains(e.target as Node) && !trigger.current?.contains(e.target as Node)) setOpen(false); };
    document.addEventListener("pointerdown", outside);
    return () => document.removeEventListener("pointerdown", outside);
  }, [open]);
  useEffect(() => { if (open) menu.current?.querySelector<HTMLElement>(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" }); }, [open, active]);

  const onKey = (e: React.KeyboardEvent) => {
    if (!open) {
      if (["ArrowDown", "ArrowUp", "Enter", " "].includes(e.key)) { e.preventDefault(); show(); }
      return;
    }
    if (e.key === "Escape") { e.preventDefault(); setOpen(false); }
    else if (e.key === "ArrowDown") { e.preventDefault(); setActive((a) => step(a, 1)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setActive((a) => step(a, -1)); }
    else if (e.key === "Enter" || e.key === " ") { e.preventDefault(); pick(active); }
    else if (e.key === "Tab") setOpen(false);
  };

  return <>
    <button type="button" ref={trigger} id={id} className={`select-trigger ${variant}`} disabled={disabled} title={title}
      aria-haspopup="listbox" aria-expanded={open} aria-controls={open ? listId : undefined} aria-label={ariaLabel}
      onClick={() => (open ? setOpen(false) : show())} onKeyDown={onKey}>
      {current?.icon && <span className="select-icon">{current.icon}</span>}
      <span className="select-value">{current?.label ?? ""}</span>
      <ChevronDown size={12} aria-hidden="true" className="select-chevron" />
    </button>
    {open && createPortal(<div ref={menu} className="select-menu" style={place} onKeyDown={onKey}>
      {menuTitle && <div className="select-menu-head">{menuTitle}</div>}
      <div role="listbox" id={listId} aria-label={ariaLabel ?? menuTitle} aria-activedescendant={`${listId}-${active}`}>
        {options.map((o, i) => <div key={o.value || `empty-${i}`} id={`${listId}-${i}`} data-index={i} role="option" aria-selected={o.value === value} aria-disabled={o.disabled || undefined}
          className={`select-option flicker-in${i === active ? " active" : ""}`} style={flicker(o.value || `empty-${i}`)}
          onPointerEnter={() => enabled(i) && setActive(i)} onClick={() => pick(i)}>
          {o.icon && <span className="select-icon">{o.icon}</span>}
          <span className="select-option-text"><span>{o.label}</span>{o.note && <span className="select-option-note">{o.note}</span>}</span>
          {o.meta && <span className="select-option-meta">{o.meta}</span>}
          <span className="select-option-check">{o.value === value && <Check size={14} aria-hidden="true" />}</span>
        </div>)}
      </div>
    </div>, document.body)}
  </>;
}
