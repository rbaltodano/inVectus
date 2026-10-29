import assert from "node:assert/strict";
import test from "node:test";
import { workspaceSplit } from "../src/workspace-split.ts";

test("dragging either direction cannot shrink either pane below 400px", () => {
  assert.deepEqual(workspaceSplit(1224, -1), { stacked: false, left: 400, right: 800, available: 1200 });
  assert.deepEqual(workspaceSplit(1224, 2), { stacked: false, left: 800, right: 400, available: 1200 });
  assert.deepEqual(workspaceSplit(1224, 0.5), { stacked: false, left: 600, right: 600, available: 1200 });
});

test("the measured workspace stacks only when both minima and the divider cannot fit", () => {
  assert.equal(workspaceSplit(823, 0.54).stacked, true);
  assert.deepEqual(workspaceSplit(824, 0.54), { stacked: false, left: 400, right: 400, available: 800 });
});

test("window resizing preserves the preferred ratio where it fits and clamps otherwise", () => {
  assert.equal(workspaceSplit(1624, 0.7).left, 1120);
  const smaller = workspaceSplit(924, 0.7);
  assert.equal(smaller.left, 500);
  assert.equal(smaller.right, 400);
  assert.equal(workspaceSplit(1624, 0.7).left, 1120);
});
