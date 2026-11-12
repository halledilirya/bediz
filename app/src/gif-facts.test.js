import assert from "node:assert/strict";
import { test } from "node:test";
import { length, limit, percent, size } from "./gif-facts.ts";

test("a size is counted in 1,024 bytes as Explorer counts it, kilobytes below a megabyte", () => {
  assert.equal(size(1_572_864, "en"), "1.5 MB");
  assert.equal(size(12_345_678, "en"), "11.8 MB");
  assert.equal(size(256_000, "en"), "250 kB");
  assert.equal(size(12, "en"), "1 kB");
  assert.equal(size(1_572_864, "de"), "1,5 MB");
  assert.equal(size(150_100_000, "tr"), "143,1 MB");
});

test("a length is shown in seconds with one decimal at most", () => {
  assert.equal(length(12_345, "en"), "12.3 sec");
  assert.equal(length(4_000, "en"), "4 sec");
});

test("a share is written the way each language writes percentages", () => {
  assert.equal(percent(45, "en"), "45%");
  assert.equal(percent(0, "en"), "0%");
  assert.equal(percent(45, "tr"), "%45");
});

test("a time limit is said in whole minutes from a minute on", () => {
  assert.equal(limit(15, "en"), "15 seconds");
  assert.equal(limit(60, "en"), "1 minute");
  assert.equal(limit(300, "en"), "5 minutes");
  assert.equal(limit(90, "en"), "90 seconds");
  assert.equal(limit(120, "tr"), "2 dakika");
});
