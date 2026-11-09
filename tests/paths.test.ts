import assert from "node:assert/strict";
import test from "node:test";
import { baseName, collectionIdFromPath, joinPath } from "../src/paths.ts";

test("file names come from the last segment on either separator", () => {
  assert.equal(baseName("/Users/you/data/papers.csv"), "papers.csv");
  assert.equal(baseName("C:\\Users\\you\\data\\papers.csv"), "papers.csv");
  assert.equal(collectionIdFromPath("C:\\data\\Eye Research.csv"), "eye-research");
});

test("a project folder keeps the directory's own separator", () => {
  assert.equal(joinPath("/Users/you/Documents", "eye"), "/Users/you/Documents/eye");
  assert.equal(joinPath("C:\\Users\\you\\Documents\\", "eye"), "C:\\Users\\you\\Documents\\eye");
  assert.equal(joinPath("  ", "eye"), "eye");
});
