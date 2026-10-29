import assert from "node:assert/strict";
import test from "node:test";
import { assessmentCopy } from "../src/search-presentation.ts";
import type { CheckResult } from "../src/api.ts";

const check = (verdict: CheckResult["verdict"]): CheckResult => ({
  verdict, p_exists: 0.01, summary: "Scoped summary", next_step: "Read the evidence", nearest_similarity: 0.63,
  between: [], position: null, notes: [],
  calibration: { method: "paraphrase", sample: 300, fitted_at: null, holdout_auc: null, stale: false, quality: "Fixture calibration" },
  search: { mode: "exact", exhaustive: true, vectors_compared: 300, compatibility: "same space", model_key: "fixture", notes: [], embed_ms: 0, retrieve_ms: 0,
    hits: [{ rank: 1, similarity: 0.63, via_chunk: null, item: { item_id: 1, source_id: "1", title: "Fixture", url: null, date: null, role: "neutral" } }] },
});

test("positive open copy stays scoped, while related and close matches remain distinct", () => {
  assert.deepEqual(assessmentCopy(check("open")), { label: "Worth exploring", message: "Your idea appears distinct in this collection." });
  assert.equal(assessmentCopy(check("near")).label, "Related ideas");
  assert.equal(assessmentCopy(check("exists")).label, "Close match");
});

test("an empty scope never becomes an opportunity claim", () => {
  const empty = check("open"); empty.search.hits = [];
  assert.equal(assessmentCopy(empty).label, "Nothing to compare");
});

test("stale calibration requires reassessment regardless of the numerical verdict", () => {
  for (const verdict of ["open", "near", "exists"] as const) {
    const stale = check(verdict); stale.calibration.stale = true;
    assert.equal(assessmentCopy(stale).label, "Reassessment needed");
  }
});

test("role-filtered assessment stays within the selected role", () => {
  assert.equal(assessmentCopy(check("open"), "coverage").message, "Your idea appears distinct in the coverage items in this collection.");
});
