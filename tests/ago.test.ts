import assert from "node:assert/strict";
import test from "node:test";
import { ago } from "../src/ago.ts";

const now = new Date(2026, 10, 9, 12, 0, 0).getTime();

test("recent times read as minutes and hours", () => {
  assert.equal(ago(now - 20_000, now), "just now");
  assert.equal(ago(now - 5 * 60_000, now), "5 min ago");
  assert.equal(ago(now - 3 * 3_600_000, now), "3 h ago");
});

test("days, then dates", () => {
  assert.equal(ago(now - 30 * 3_600_000, now), "yesterday");
  assert.equal(ago(now - 4 * 86_400_000, now), "4 days ago");
  assert.match(ago(new Date(2026, 10, 1).getTime(), now), /1/);
  assert.match(ago(new Date(2025, 10, 1).getTime(), now), /2025/);
});

test("a clock that ran backwards is not in the future", () => {
  assert.equal(ago(now + 60_000, now), "just now");
});
