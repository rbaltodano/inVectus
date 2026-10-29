/** Centre a region and fit its outline inside 65% of the map diameter. Display only. */
export function regionFocusView(
  region: { x: number; y: number; radius: number },
  fit: { k: number; cx: number; cy: number },
  size: number,
  minZoom: number,
  maxZoom: number,
) {
  const radius = Math.max(1, region.radius * fit.k);
  const s = Math.min(maxZoom, Math.max(minZoom, size * 0.325 / radius));
  return { s, x: -(region.x - fit.cx) * fit.k * s, y: -(region.y - fit.cy) * fit.k * s };
}
