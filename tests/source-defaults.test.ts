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

test("with no column named like text, the prose columns are searched", () => {
  const cols = ["record_id", "created_time", "Name", "Request", "Votes", "Link"];
  const sample = [
    ["rec1", "2026-01-01", "Ada Park", "Export to PDF loses figure captions", "4", "https://x.io/1"],
    ["rec2", "2026-01-02", "Lee Chen", "Offline search for clinical notes", "9", "https://x.io/2"],
  ];
  const d = columnDefaults("airtable", cols, sample);
  assert.deepEqual(d.searched, ["Name", "Request"]);
  assert.equal(d.id, "record_id");
  assert.deepEqual(columnDefaults("file", ["a", "b"], [["1", "2"]]).searched, [], "nothing prose-like: the person chooses");
});

test("Notion: the title property is whatever comes after the fixed columns; prose properties and body are searched", () => {
  const cols = ["page_id", "url", "created_time", "last_edited_time", "Idea", "Area", "Done", "Link", "Submitted", "Summary", "Tags", "Votes", "body"];
  const sample = [
    ["p1", "https://app.notion.com/p/1", "2026-11-12", "2026-11-12", "Offline search on phones", "Search", "true", "https://example.com/1", "2026-10-02", "Let people search without signal.", "mobile", "14", "Notes\nRaised in three calls."],
    ["p2", "https://app.notion.com/p/2", "2026-11-12", "2026-11-12", "Student discount", "Pricing", "false", "https://example.com/2", "2026-10-21", "Half price with a school email address.", "", "3", ""],
  ];
  const d = columnDefaults("notion", cols, sample);
  assert.equal(d.title, "Idea");
  assert.deepEqual(d.searched, ["Idea", "Summary", "body"]);
  assert.deepEqual([d.id, d.url], ["page_id", "url"]);
});
