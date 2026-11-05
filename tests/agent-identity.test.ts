import { test } from "node:test";
import assert from "node:assert/strict";
import { clientName } from "../src/agentIdentity.ts";

test("clients show the names people know", () => {
  assert.equal(clientName("codex-mcp-client"), "Codex");
  assert.equal(clientName("codex"), "Codex");
  assert.equal(clientName("claude-code"), "Claude Code");
  assert.equal(clientName("some-new-client"), "some-new-client");
  assert.equal(clientName(null), "An agent");
});
