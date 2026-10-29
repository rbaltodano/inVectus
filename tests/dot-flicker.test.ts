import assert from "node:assert/strict";
import test from "node:test";
import { DotFlicker } from "../src/dot-flicker.ts";

test("zero-delay dot flickers immediately and settles after exactly 150 ms", () => {
  const dots = new DotFlicker(() => 0);
  dots.sync(["item:1"], 1000);
  assert.equal(dots.opacity("item:1", 1000), 1);
  assert.equal(dots.opacity("item:1", 1025), 0.2);
  assert.equal(dots.opacity("item:1", 1050), 0.85);
  assert.equal(dots.opacity("item:1", 1149), 0.5);
  assert.equal(dots.opacity("item:1", 1150), 1);
  assert.equal(dots.pending(1150), false);
});

test("maximum delay hides the dot for 250 ms, then flickers for 150 ms", () => {
  const dots = new DotFlicker(() => 1);
  dots.sync(["item:1"], 1000);
  assert.equal(dots.opacity("item:1", 1249), 0);
  assert.equal(dots.opacity("item:1", 1250), 1);
  assert.equal(dots.pending(1399), true);
  assert.equal(dots.opacity("item:1", 1400), 1);
  assert.equal(dots.pending(1400), false);
});

test("each new dot receives its own delay, and redraws retain existing timings", () => {
  const delays = [0, 0.5, 1];
  let calls = 0;
  const dots = new DotFlicker(() => delays[calls++]);
  dots.sync(["a", "b"], 1000);
  assert.equal(dots.opacity("a", 1100), 0.7);
  assert.equal(dots.opacity("b", 1100), 0);
  dots.sync(["a", "b", "c"], 1100);
  assert.equal(calls, 3);
  assert.equal(dots.opacity("a", 1200), 1);
  assert.equal(dots.opacity("b", 1200), 0.35);
  assert.equal(dots.opacity("c", 1200), 0);
});

test("leaving squares are pruned, and reappearing squares get a fresh entrance", () => {
  const dots = new DotFlicker(() => 1);
  dots.sync(["a"], 0);
  dots.sync([], 500);
  assert.equal(dots.pending(500), false);
  dots.sync(["a"], 1000);
  assert.equal(dots.opacity("a", 1000), 0);
  dots.clear();
  assert.equal(dots.pending(1000), false);
});
