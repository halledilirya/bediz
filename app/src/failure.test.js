import assert from "node:assert/strict";
import { test } from "node:test";
import { explain } from "./failure.ts";

const text = (id, args) => (args ? `${id} ${JSON.stringify(args)}` : id);

test("a missing folder is named in the user's language", () => {
  assert.equal(
    explain({ kind: "missing", path: "C:/Users/Ada/Pictures/2.png" }, text),
    'settings-reason-missing {"path":"C:/Users/Ada/Pictures/2.png"}',
  );
});

test("Windows' own text is shown as Windows wrote it", () => {
  assert.equal(
    explain({ kind: "windows", reason: "Erişim engellendi." }, text),
    "Erişim engellendi.",
  );
});

test("anything else says something unexpected happened", () => {
  for (const failure of [
    { kind: "unexpected" },
    { kind: "missing" },
    { kind: "windows", reason: 5 },
    "an English error from Tauri",
    null,
    undefined,
  ]) {
    assert.equal(explain(failure, text), "settings-reason-unexpected");
  }
});
