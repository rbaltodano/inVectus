import assert from "node:assert/strict";
import test from "node:test";
import { gapFocusArea, interpolateCamera, regionFocusView } from "../src/map-focus.ts";

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

test("bridge focus centres the actual midpoint and includes both region outlines", () => {
  const a = { x: -8, y: 0, radius: 2 }, b = { x: 8, y: 0, radius: 3 };
  const area = gapFocusArea(a, b);
  assert.deepEqual(area, { x: 0, y: 0, radius: 11 });
  for (const region of [a, b]) assert.ok(Math.hypot(region.x - area.x, region.y - area.y) + region.radius <= area.radius);
  assert.equal(gapFocusArea(a), a);
});

test("camera transition reaches exact endpoints and pans its centre while zooming", () => {
  const start = { s: 1, x: 0, y: 0 }, target = { s: 16, x: -160, y: 80 };
  assert.deepEqual(interpolateCamera(start, target, 0), start);
  assert.deepEqual(interpolateCamera(start, target, 1), target);
  const halfway = interpolateCamera(start, target, 0.5);
  assert.deepEqual(halfway, { s: 4, x: -20, y: 10 });
  assert.equal(-halfway.x / halfway.s, 5);
  assert.equal(-halfway.y / halfway.s, -2.5);
});
