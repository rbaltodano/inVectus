import assert from "node:assert/strict";
import test from "node:test";
import { covers, toggle } from "../src/choice-cover.ts";

const lib = { spec: "zotero:library", title: "Whole library", group: "My Library" };
const glau = { spec: "zotero:collection:AAAAAAAA", title: "Glaucoma", group: "My Library" };
const oct = { spec: "zotero:collection:BBBBBBBB", title: "Glaucoma / OCT", group: "My Library" };
const glauX = { spec: "zotero:collection:CCCCCCCC", title: "Glaucoma surgery", group: "My Library" };
const lab = { spec: "zotero:group:1", title: "Whole library", group: "Lab" };
const labCol = { spec: "zotero:group:1/collection:DDDDDDDD", title: "Glaucoma", group: "Lab" };

test("a whole library covers its own collections, not another library's", () => {
  assert.ok(covers(lib, glau) && covers(lib, oct));
  assert.ok(!covers(lib, labCol) && covers(lab, labCol));
});

test("a collection covers its sub-collections but not a sibling with a similar name", () => {
  assert.ok(covers(glau, oct));
  assert.ok(!covers(glau, glauX) && !covers(oct, glau));
});

test("tables cover their views; All boards covers each board", () => {
  const t = { spec: "airtable:appA/tblA", title: "Requests", group: "Product" };
  const v = { spec: "airtable:appA/tblA/viwA", title: "Requests / Open", group: "Product" };
  assert.ok(covers(t, v) && !covers(v, t));
  assert.ok(covers({ spec: "canny:all", title: "All boards" }, { spec: "canny:board:5f1", title: "Bugs" }));
});

test("toggling: any number can be picked; a covering pick drops what it covers", () => {
  let p = toggle(oct, []);
  p = toggle(glauX, p);
  p = toggle(labCol, p);
  assert.equal(p.length, 3);
  p = toggle(glau, p); // covers OCT
  assert.deepEqual(p.map((c) => c.title).sort(), ["Glaucoma", "Glaucoma", "Glaucoma surgery"]);
  assert.equal(toggle(oct, p), p, "covered choices stay out");
  assert.equal(toggle(glau, p).length, 2, "toggling off");
});
