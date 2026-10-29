import assert from "node:assert/strict";
import test from "node:test";
import { navigateTo, returnFromDetail, pageTab, tabKeyIndex, PROJECT_TABS, type TabStacks } from "../src/navigation.ts";

test("project destinations map to the correct tabs", () => {
  assert.equal(pageTab({ kind: "search" }), "search");
  assert.equal(pageTab({ kind: "gaps" }), "gaps");
  assert.equal(pageTab({ kind: "import" }), "collections");
  assert.equal(pageTab({ kind: "index" }), "collections");
});

test("import opens under Collections and preserves Search history", () => {
  const stacks: TabStacks = { search: [{ kind: "search", query: "retinal imaging" }] };
  const next = navigateTo(stacks, "search", { kind: "import", path: "/tmp/papers.csv" });
  assert.equal(next.tab, "collections");
  assert.deepEqual(next.stacks.collections, [{ kind: "index" }, { kind: "import", path: "/tmp/papers.csv" }]);
  assert.equal(next.stacks.search, stacks.search);
  assert.equal(stacks.collections, undefined);
});

test("details stay in their originating tab with a return path", () => {
  const stacks: TabStacks = { search: [{ kind: "search", query: "imaging" }], gaps: [{ kind: "gaps" }] };
  const next = navigateTo(stacks, "search", { kind: "gap", id: 42 });
  assert.equal(next.tab, "search");
  assert.deepEqual(next.stacks.search?.slice(0, -1), stacks.search);
  assert.equal(next.stacks.gaps, stacks.gaps);
});

test("cross-tab destinations replace the root without accumulating Back steps", () => {
  const next = navigateTo({ search: [{ kind: "search", query: "old" }, { kind: "gap", id: 42 }] }, "gaps", { kind: "search", query: "new" });
  assert.equal(next.tab, "search");
  assert.deepEqual(next.stacks.search, [{ kind: "search", query: "new" }]);
});

test("tab keyboard navigation wraps and handles Home and End", () => {
  assert.equal(tabKeyIndex("ArrowRight", 2), 0);
  assert.equal(tabKeyIndex("ArrowLeft", 0), 2);
  assert.equal(tabKeyIndex("Home", 2), 0);
  assert.equal(tabKeyIndex("End", 1), 2);
  assert.equal(tabKeyIndex("Tab", 1), undefined);
});

test("only Search, Gaps and Collections remain", () => {
  assert.deepEqual(PROJECT_TABS.map((t) => t.id), ["search", "gaps", "collections"]);
});

test("Back skips successive point selections and preserves the existing Search query", () => {
  const point = { item_id: 1, title: "First", role: "demand" as const, date: null, url: null, x: 0, y: 0, fine_id: 1, broad_id: 1, noise: false };
  const stacks: TabStacks = { search: [{ kind: "search", query: "retinal imaging" }, { kind: "item", point }, { kind: "item", point: { ...point, item_id: 2 } }] };
  const next = returnFromDetail(stacks, "search");
  assert.equal(next.tab, "search");
  assert.deepEqual(next.stacks.search, [{ kind: "search", query: "retinal imaging" }]);
  assert.equal(stacks.search?.length, 3);
});

test("Back from a gap returns to the gap list and leaves Search alone", () => {
  const search: TabStacks["search"] = [{ kind: "search", query: "existing query" }, { kind: "gap", id: 1 }];
  const stacks: TabStacks = { search, gaps: [{ kind: "gaps" }, { kind: "gap", id: 2 }, { kind: "gap", id: 3 }] };
  const next = returnFromDetail(stacks, "gaps");
  assert.equal(next.tab, "gaps");
  assert.deepEqual(next.stacks.gaps, [{ kind: "gaps" }]);
  assert.equal(next.stacks.search, search);
});

test("Back creates Search when it has not been visited", () => {
  const next = returnFromDetail({ collections: [{ kind: "index" }, { kind: "import" }] }, "collections");
  assert.deepEqual(next.stacks.search, [{ kind: "search" }]);
  assert.deepEqual(next.stacks.collections, [{ kind: "index" }]);
});
