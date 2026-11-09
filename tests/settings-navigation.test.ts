import assert from "node:assert/strict";
import test from "node:test";
import { SETTINGS_SECTIONS, searchSettings, settingsSection } from "../src/settingsNavigation.ts";

test("a stored settings tab is kept only when it still exists", () => {
  assert.equal(settingsSection("calibration"), "calibration");
  assert.equal(settingsSection("retired-tab"), "appearance");
  assert.equal(settingsSection(undefined), "appearance");
});

test("settings search finds controls by provider and preference, including project controls", () => {
  assert.deepEqual(searchSettings("grok").map((s) => s.id), ["agents"]);
  assert.deepEqual(searchSettings("gemini").map((s) => s.id), ["agents"]);
  assert.deepEqual(searchSettings("lm studio").map((s) => s.id), ["agents"]);
  assert.deepEqual(searchSettings("vs code").map((s) => s.id), ["agents"]);
  assert.deepEqual(searchSettings("  DARK  ").map((s) => s.id), ["appearance"]);
  assert.deepEqual(searchSettings("coverage momentum").map((s) => s.id), ["scoring"]);
  assert.deepEqual(searchSettings("offline").map((s) => s.id), ["network"]);
  assert.deepEqual(searchSettings("runtime").map((s) => s.id), ["embedding"]);
  assert.deepEqual(searchSettings("recent coverage penalty").map((s) => s.id), ["scoring"]);
  assert.deepEqual(searchSettings("new-local-model", { models: "new-local-model" }).map((s) => s.id), ["models"]);
});

test("clearing settings search restores every category; unknown terms produce no results", () => {
  assert.deepEqual(searchSettings("  "), SETTINGS_SECTIONS);
  assert.deepEqual(searchSettings("not-a-setting"), []);
});

test("settings tabs wrap with arrow keys and support Home and End", async () => {
  const { settingsTabIndex } = await import("../src/settingsNavigation.ts");
  assert.equal(settingsTabIndex("ArrowLeft", 0), SETTINGS_SECTIONS.length - 1);
  assert.equal(settingsTabIndex("ArrowRight", SETTINGS_SECTIONS.length - 1), 0);
  assert.equal(settingsTabIndex("Home", 4), 0);
  assert.equal(settingsTabIndex("End", 0), SETTINGS_SECTIONS.length - 1);
  assert.equal(settingsTabIndex("Tab", 0), undefined);
});
