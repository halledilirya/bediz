import assert from "node:assert/strict";
import { test } from "node:test";
import { LONGEST, problem, sampleAt, stem } from "./pattern.ts";

const SAMPLE = { date: "2026-03-07", time: "090502", width: 1280, height: 720 };

test("the default pattern is valid and names the file by date and time", () => {
  assert.equal(problem("Bediz {date} {time}"), null);
  assert.equal(stem("Bediz {date} {time}", SAMPLE), "Bediz 2026-03-07 090502");
});

test("every part is filled in and the ends are trimmed", () => {
  assert.equal(
    stem("  {width}x{height} {date}{date} ", SAMPLE),
    "1280x720 2026-03-072026-03-07",
  );
});

test("a name a file cannot have is explained", () => {
  assert.deepEqual(problem(""), { kind: "empty" });
  assert.deepEqual(problem("   "), { kind: "empty" });
  assert.deepEqual(problem("a/b"), { kind: "forbidden" });
  assert.deepEqual(problem("what?"), { kind: "forbidden" });
  assert.deepEqual(problem("a\tb"), { kind: "forbidden" });
  assert.deepEqual(problem("a".repeat(LONGEST + 1)), { kind: "long" });
  assert.equal(problem("ş".repeat(LONGEST)), null);
});

test("only the four parts are known", () => {
  assert.deepEqual(problem("{day}"), { kind: "unknown", token: "{day}" });
  assert.deepEqual(problem("Bediz {date"), {
    kind: "unknown",
    token: "{date",
  });
  assert.deepEqual(problem("{}"), { kind: "unknown", token: "{}" });
  assert.equal(problem("a}b"), null);
});

test("a name Windows keeps for a device is explained, as in Rust", () => {
  for (const [pattern, name] of [
    ["NUL", "NUL"],
    ["con", "con"],
    [" Aux ", "Aux"],
    ["prn.shot", "prn"],
    ["COM1", "COM1"],
    ["lpt9", "lpt9"],
    ["COM¹", "COM¹"],
    ["Nul .", "Nul"],
  ]) {
    assert.deepEqual(problem(pattern), { kind: "reserved", name }, pattern);
  }
  for (const pattern of [
    "NULL",
    "COM10",
    "LPT",
    "My CON",
    "{date} NUL",
    "NUL{time}",
    "conference",
  ]) {
    assert.equal(problem(pattern), null, pattern);
  }
});

test("the sample uses the local date and time with leading zeros", () => {
  const sample = sampleAt(new Date(2026, 2, 7, 9, 5, 2), 10, 20);
  assert.deepEqual(sample, {
    date: "2026-03-07",
    time: "090502",
    width: 10,
    height: 20,
  });
});
