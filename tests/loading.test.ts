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

test("idle has no progress indicator", () => {
  assert.equal(indexingProgress(undefined), undefined);
});

test("real indexing reports progress and clamps completed work", () => {
  assert.deepEqual(indexingProgress(job()), { percent: 42, label: "Embedding" });
  assert.equal(indexingProgress(job({ done: 110 }))?.percent, 100);
  assert.equal(indexingProgress(job({ done: -10 }))?.percent, 0);
});

test("unknown totals stay indeterminate and finished jobs disappear", () => {
  assert.equal(indexingProgress(job({ total: 0 }))?.percent, undefined);
  assert.equal(indexingProgress(job({ state: "completed" })), undefined);
  assert.equal(indexingProgress(job({ state: "failed" })), undefined);
  assert.equal(indexingProgress(job({ kind: "download" })), undefined);
});
