import assert from "node:assert/strict";
import test from "node:test";
import { regionFocusView } from "../src/map-focus.ts";

test("Show on map centres an off-centre region and fits its outline", () => {
  const region = { x: 12, y: -7, radius: 2 };
  const fit = { k: 10, cx: 2, cy: 3 };
  const size = 600;
  const view = regionFocusView(region, fit, size, 0.6, 200);
  assert.equal(size / 2 + (region.x - fit.cx) * fit.k * view.s + view.x, size / 2);
  assert.equal(size / 2 + (region.y - fit.cy) * fit.k * view.s + view.y, size / 2);
  assert.equal(region.radius * fit.k * view.s, size * 0.325);
});

test("tiny and very large regions respect map zoom limits", () => {
  const fit = { k: 10, cx: 0, cy: 0 };
  assert.equal(regionFocusView({ x: 0, y: 0, radius: 0 }, fit, 1000, 0.6, 200).s, 200);
  assert.equal(regionFocusView({ x: 0, y: 0, radius: 10000 }, fit, 600, 0.6, 200).s, 0.6);
});
