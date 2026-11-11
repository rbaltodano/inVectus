import { useRef, type CSSProperties } from "react";
export const DOT_DELAY_MS = 250;
export const DOT_FLICKER_MS = 150;
const LEVELS = [1, 0.2, 0.85, 0.35, 0.7, 0.5];

/** Track only visible squares; surviving squares keep their timing as the camera moves. */
export class DotFlicker {
  private starts = new Map<string, number>();
  private until = 0;

  private readonly random: () => number;

  constructor(random: () => number = Math.random) { this.random = random; }

  sync(keys: Iterable<string>, now: number) {
    const next = new Map<string, number>();
    this.until = 0;
    for (const key of keys) {
      const start = this.starts.get(key) ?? now + this.random() * DOT_DELAY_MS;
      next.set(key, start);
      this.until = Math.max(this.until, start + DOT_FLICKER_MS);
    }
    this.starts = next;
  }

  opacity(key: string, now: number) {
    const start = this.starts.get(key);
    if (start === undefined || now >= start + DOT_FLICKER_MS) return 1;
    if (now < start) return 0;
    return LEVELS[Math.floor((now - start) / DOT_FLICKER_MS * LEVELS.length)];
  }

  pending(now: number) { return now < this.until; }

  clear() { this.starts.clear(); this.until = 0; }
}

/** Design system: cards (integrations, agents, sources) flicker in like the circle's squares,
 *  each after its own random delay under DOT_DELAY_MS. `flicker(key)` gives a card its delay as
 *  `--card-delay` (used by `.integration-card` and `.flicker-in`); delays stay put for the life
 *  of the component, so re-renders do not replay or reshuffle them. */
export function useFlickerDelays() {
  const delays = useRef(new Map<string, number>());
  return (key: string) => {
    let d = delays.current.get(key);
    if (d === undefined) { d = Math.round(Math.random() * DOT_DELAY_MS); delays.current.set(key, d); }
    return { "--card-delay": `${d}ms` } as CSSProperties;
  };
}
