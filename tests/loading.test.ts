import assert from "node:assert/strict";
import test from "node:test";
import { LOADER_ORDER, indexingProgress } from "../src/loading.ts";
import type { JobInfo } from "../src/api.ts";

const job = (overrides: Partial<JobInfo> = {}): JobInfo => ({
  id: 1, kind: "index", project: "/tmp/sample.invectus", collection: "papers", space: null,
  models: [], label: "Indexing", phase: "Embedding", done: 42, total: 100, state: "running",
  error: null, report: null, started_ms: 0, finished_ms: null, ...overrides,
});

test("loader loops clockwise around all six squares", () => {
  assert.deepEqual(LOADER_ORDER, [0, 1, 3, 5, 4, 2]);
  assert.equal(new Set(LOADER_ORDER).size, 6);
});

test("preview is visible when idle without claiming an actual job", () => {
  assert.equal(indexingProgress(undefined, false), undefined);
  assert.deepEqual(indexingProgress(undefined, true), { percent: 84, preview: true, label: "Indexing animation preview" });
});

test("real indexing takes precedence over preview and clamps completed work", () => {
  assert.deepEqual(indexingProgress(job(), true), { percent: 42, preview: false, label: "Embedding" });
  assert.equal(indexingProgress(job({ done: 110 }), false)?.percent, 100);
  assert.equal(indexingProgress(job({ done: -10 }), false)?.percent, 0);
});

test("unknown totals stay indeterminate and finished jobs disappear outside preview", () => {
  assert.equal(indexingProgress(job({ total: 0 }), false)?.percent, undefined);
  assert.equal(indexingProgress(job({ state: "completed" }), false), undefined);
  assert.equal(indexingProgress(job({ state: "failed" }), false), undefined);
  assert.equal(indexingProgress(job({ kind: "download" }), false), undefined);
});
