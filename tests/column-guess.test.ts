import assert from "node:assert/strict";
import test from "node:test";
import { guessMapping } from "../src/column-guess.ts";

test("column guesses use a whole word, not a letter sequence inside another name", () => {
  const guessed = guessMapping(["width", "valid", "context", "subtitle", "doing", "paper_id", "full_text", "publication_date"]);
  assert.equal(guessed.id, "paper_id");
  assert.equal(guessed.text, "full_text");
  assert.equal(guessed.date, "publication_date");
  assert.equal(guessed.title, "");
  assert.equal(guessed.url, "");
});

test("exact names still win, including the usual paper columns", () => {
  const guessed = guessMapping(["pmid", "title", "abstract", "pub_date", "doi", "publication_types"]);
  assert.deepEqual(guessed, {
    text: "abstract", title: "title", id: "pmid", date: "pub_date", url: "doi", role: "publication_types",
  });
});
