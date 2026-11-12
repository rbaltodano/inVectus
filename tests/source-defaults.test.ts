import assert from "node:assert/strict";
import test from "node:test";
import { columnDefaults } from "../src/source-defaults.ts";

const zotero = ["key", "item_type", "title", "abstract", "creators", "date", "publication", "doi", "url", "tags", "collections", "date_added", "date_modified", "extra"];
const canny = ["post_id", "title", "details", "status", "votes", "comments", "board", "category", "tags", "author", "created", "eta", "url"];

test("Zotero opens with title and abstract searched and its key, date and url set", () => {
  const d = columnDefaults("zotero", zotero, []);
  assert.deepEqual(d.searched, ["title", "abstract"]);
  assert.deepEqual([d.title, d.id, d.date, d.url], ["title", "key", "date", "url"]);
});

test("Canny opens with title and details searched", () => {
  const d = columnDefaults("canny", canny, []);
  assert.deepEqual(d.searched, ["title", "details"]);
  assert.deepEqual([d.id, d.date, d.url], ["post_id", "created", "url"]);
});

test("Productboard notes and features differ", () => {
  const notes = columnDefaults("productboard", ["note_id", "title", "content", "customer", "company", "tags", "features", "source", "created", "url"], []);
  assert.deepEqual(notes.searched, ["title", "content"]);
  const features = columnDefaults("productboard", ["feature_id", "name", "description", "type", "status", "parent", "timeframe_start", "timeframe_end", "archived", "url"], []);
  assert.deepEqual(features.searched, ["name", "description"]);
  assert.equal(features.title, "name");
});

test("Typeform searches open-text answers, not ids, timestamps, numbers or hidden fields", () => {
  const cols = ["response_id", "submitted_at", "What should we build next?", "How likely are you to recommend us?", "hidden_utm_source"];
  const sample = [["r1", "2026-11-01", "A way to export reports to PDF", "9", "ads"], ["r2", "2026-11-02", "Better search across old projects", "7", "mail"]];
  const d = columnDefaults("typeform", cols, sample);
  assert.deepEqual(d.searched, ["What should we build next?"]);
  assert.deepEqual([d.id, d.date], ["response_id", "submitted_at"]);
});

test("plain files keep the name-based guess; the url never reuses the id column", () => {
  const d = columnDefaults("csv", ["pmid", "title", "abstract", "doi"], []);
  assert.deepEqual(d.searched, ["abstract"]);
  assert.equal(d.id, "pmid");
  assert.notEqual(d.url, d.id);
});
