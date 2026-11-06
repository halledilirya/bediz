import assert from "node:assert/strict";
import { test } from "node:test";
import { followLanguage } from "./language.ts";

function deferred() {
  let resolve;
  const promise = new Promise((settle) => {
    resolve = settle;
  });
  return { promise, resolve };
}

function follower() {
  const reads = [];
  const applied = [];
  const follow = followLanguage(
    () => {
      const read = deferred();
      reads.push(read);
      return read.promise;
    },
    (language) => {
      applied.push(language);
    },
  );
  return { follow, reads, applied };
}

test("a single read is applied", async () => {
  const { follow, reads, applied } = follower();
  const refreshed = follow.refresh();
  reads[0].resolve("tr");
  await refreshed;
  assert.deepEqual(applied, ["tr"]);
});

test("an older answer that arrives last never replaces the newer language", async () => {
  const { follow, reads, applied } = follower();
  const first = follow.refresh();
  const second = follow.refresh();
  reads[1].resolve("tr");
  await second;
  reads[0].resolve("en");
  await first;
  assert.deepEqual(applied, ["tr"]);
});

test("answers in order apply only the last one asked for", async () => {
  const { follow, reads, applied } = follower();
  const first = follow.refresh();
  const second = follow.refresh();
  reads[0].resolve("en");
  await first;
  reads[1].resolve("tr");
  await second;
  assert.deepEqual(applied, ["tr"]);
});
