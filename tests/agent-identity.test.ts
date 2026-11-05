import { test } from "node:test";
import assert from "node:assert/strict";
import { agentIdentity, clientName } from "../src/agentIdentity.ts";

test("clients show the names people know", () => {
  assert.equal(clientName("codex-mcp-client"), "Codex");
  assert.equal(clientName("codex"), "Codex");
  assert.equal(clientName("claude-code"), "Claude Code");
  assert.equal(clientName("some-new-client"), "some-new-client");
  assert.equal(clientName(null), "An agent");
});

test("model labels preserve version, family and reported effort", () => {
  assert.equal(agentIdentity("agent:codex/gpt-6.1-sol-medium").label, "GPT-6.1 Sol Medium");
  assert.equal(agentIdentity("agent:codex/gpt-6.1-terra-high").label, "GPT-6.1 Terra High");
  assert.equal(agentIdentity("agent:codex/gpt-6-luna").label, "GPT-6 Luna");
  assert.equal(agentIdentity("agent:codex/gpt-6.1").label, "GPT-6.1");
});
