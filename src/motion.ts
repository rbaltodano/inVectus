// Angrove's motion, for CSS. Motion.springRelaxed (response 0.5 s, damping 0.8; see
// angrove.app/js/insight-tree.js `Spring`) is simulated once and handed to CSS as a `linear()`
// easing with its settle time. Browsers without `linear()` get the closest cubic-bezier.

function springSamples(response = 0.5, damping = 0.8) {
  const k = (2 * Math.PI / response) ** 2, c = 2 * damping * Math.sqrt(k), h = 1 / 240;
  let v = 0, vel = 0, t = 0;
  const out: [number, number][] = [[0, 0]];
  while (t < 2) {
    for (let i = 0; i < 4; i++) { const a = -k * (v - 1) - c * vel; vel += a * h; v += vel * h; t += h; }
    out.push([t, v]);
    if (t > 0.3 && Math.abs(v - 1) < 0.001 && Math.abs(vel) < 0.01) break;
  }
  return out;
}

const SPRING = springSamples();
SPRING[SPRING.length - 1][1] = 1;
/** Settle time of springRelaxed, in ms. */
export const SPRING_MS = SPRING[SPRING.length - 1][0] * 1000;
/** springRelaxed's progress `ms` after it starts (0 before, 1 once settled), for JS-driven motion. */
export function springAt(ms: number): number {
  if (ms <= 0) return 0;
  if (ms >= SPRING_MS) return 1;
  const t = ms / 1000;
  let i = 1;
  while (SPRING[i][0] < t) i++;
  const [t0, v0] = SPRING[i - 1], [t1, v1] = SPRING[i];
  return v0 + ((v1 - v0) * (t - t0)) / (t1 - t0);
}

export function installMotion(root: HTMLElement = document.documentElement) {
  const s = springSamples();
  const end = s[s.length - 1][0];
  s[s.length - 1][1] = 1; // land exactly on the target
  const curve = `linear(${s.filter((_, i) => i % 3 === 0 || i === s.length - 1)
    .map(([t, v]) => `${v.toFixed(4)} ${((t / end) * 100).toFixed(2)}%`).join(", ")})`;
  const supported = typeof CSS !== "undefined" && CSS.supports("animation-timing-function", "linear(0, 1)");
  root.style.setProperty("--spring-relaxed", supported ? curve : "cubic-bezier(0.25, 1.1, 0.5, 1)");
  root.style.setProperty("--spring-relaxed-duration", `${Math.round(end * 1000)}ms`);
}

/** Per-item delay so a run of `n` items enters within about `total` ms (Angrove staggers 25–55 ms). */
export const stagger = (n: number, total = 650, max = 40) => Math.min(max, total / Math.max(1, n));
