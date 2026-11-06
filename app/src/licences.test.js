import assert from "node:assert/strict";
import { test } from "node:test";
import { NOTICES, readNotices } from "./licences.ts";

const MISSING = "made for a release";

const answer = (ok, type, body) => ({
  ok,
  type,
  text: () => Promise.resolve(body),
});

const serving = (answers) => (path) =>
  answers[path] instanceof Error
    ? Promise.reject(answers[path])
    : Promise.resolve(answers[path]);

const [RUST, FRONTEND] = NOTICES;

test("both notices are shown in order", async () => {
  const fetcher = serving({
    [RUST]: answer(true, "text/plain", "# Rust crates"),
    [FRONTEND]: answer(true, "text/plain; charset=utf-8", "# Licenses"),
  });
  assert.equal(
    await readNotices(fetcher, MISSING),
    "# Rust crates\n\n# Licenses",
  );
});

test("the settings page served in place of a missing file counts as missing", async () => {
  const fetcher = serving({
    [RUST]: answer(true, "text/html", "<!doctype html>"),
    [FRONTEND]: answer(true, "text/plain", "# Licenses"),
  });
  assert.equal(await readNotices(fetcher, MISSING), `${MISSING}\n\n# Licenses`);
});

test("a failed or refused read says once that the notices are missing", async () => {
  const fetcher = serving({
    [RUST]: answer(false, "text/plain", "not found"),
    [FRONTEND]: new Error("offline"),
  });
  assert.equal(await readNotices(fetcher, MISSING), MISSING);
});
