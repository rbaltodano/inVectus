type FocusArea = { x: number; y: number; radius: number };
type CameraView = { s: number; x: number; y: number };

/** Bridge markers sit at the midpoint; keep both regions in the focused view. */
export function gapFocusArea(a: FocusArea, b?: FocusArea): FocusArea {
  return b ? { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2, radius: Math.hypot(a.x - b.x, a.y - b.y) / 2 + Math.max(a.radius, b.radius) } : a;
}

/** Smoothly pan the camera centre while zooming geometrically. */
export function interpolateCamera(start: CameraView, target: CameraView, progress: number): CameraView {
  if (progress <= 0) return start;
  if (progress >= 1) return target;
  const ease = progress * progress * (3 - 2 * progress);
  const s = start.s * Math.pow(target.s / start.s, ease);
  return {
    s,
    x: (start.x / start.s + (target.x / target.s - start.x / start.s) * ease) * s,
    y: (start.y / start.s + (target.y / target.s - start.y / start.s) * ease) * s,
  };
}

/** Centre a region and fit its outline inside 65% of the map diameter. Display only. */
export function regionFocusView(
  region: FocusArea,
  fit: { k: number; cx: number; cy: number },
  size: number,
  minZoom: number,
  maxZoom: number,
) {
  const radius = Math.max(1, region.radius * fit.k);
  const s = Math.min(maxZoom, Math.max(minZoom, size * 0.325 / radius));
  return { s, x: -(region.x - fit.cx) * fit.k * s, y: -(region.y - fit.cy) * fit.k * s };
}
