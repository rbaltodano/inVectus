import assert from "node:assert/strict";
import test from "node:test";
import { changesProject, describe, recordActivity, type AgentActivity } from "../src/agent-activity.ts";
const event = (extra: Partial<AgentActivity> = {}): AgentActivity => ({ client: "codex", tool: "suggest_decision", project: "/a", query: null, gap_id: 2, ok: true, error: null, at_ms: 1, call_id: "1", phase: "started", ...extra });

test("in-flight calls are replaced by completion, while concurrent calls remain visible", () => {
  const running = recordActivity([event({call_id: "other"})], event());
  assert.equal(running.length, 2);
  const done = recordActivity(running, event({ phase: "finished", ok: false, error: "busy" }));
  assert.equal(done.length, 2);
  assert.equal(done[0].error, "busy");
  assert.equal(done[1].phase, "started");
  assert.equal(describe(done[0]), "suggest decision failed");
});

test("only successful completed writes refresh the matching project", () => {
  assert.equal(changesProject(event(), "/a"), false);
  assert.equal(changesProject(event({ phase: "finished" }), "/a"), true);
  assert.equal(changesProject(event({ phase: "finished" }), "/b"), false);
  assert.equal(changesProject(event({ phase: "finished", ok: false }), "/a"), false);
  assert.equal(changesProject(event({ phase: "finished", tool: "search" }), "/a"), false);
  for (const tool of ["verify_gap", "submit_description", "submit_region_names", "submit_paraphrases"]) {
    assert.equal(changesProject(event({ phase: "finished", tool }), "/a"), true);
  }
});

test("older clients' completed events remain supported and cancellation wording is precise", () => {
  assert.equal(changesProject(event({phase: undefined}), "/a"), true);
  assert.equal(describe(event({tool: "cancel_job", phase: "finished"})), "requested cancellation");
});
