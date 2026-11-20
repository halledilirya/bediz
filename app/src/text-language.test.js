import assert from "node:assert/strict";
import { test } from "node:test";
import { languageChoices, nativeName } from "./text-language.ts";

const names = { "tr-TR": "Türkçe", "en-US": "English", "ja-JP": "日本語" };
const named = (tag) => names[tag] ?? tag;

test("installed languages are offered by their own names in order", () => {
  assert.deepEqual(languageChoices(["tr-TR", "en-US"], null, named), [
    { tag: "en-US", name: "English", missing: false },
    { tag: "tr-TR", name: "Türkçe", missing: false },
  ]);
});

test("a chosen language Windows no longer has is offered as missing", () => {
  assert.deepEqual(languageChoices(["en-US"], "ja-JP", named), [
    { tag: "en-US", name: "English", missing: false },
    { tag: "ja-JP", name: "日本語", missing: true },
  ]);
  assert.deepEqual(languageChoices(["en-US", "ja-JP"], "ja-JP", named), [
    { tag: "en-US", name: "English", missing: false },
    { tag: "ja-JP", name: "日本語", missing: false },
  ]);
});

test("before the installed languages are known the chosen one is kept and not called missing", () => {
  assert.deepEqual(languageChoices(null, "ja-JP", named), [
    { tag: "ja-JP", name: "日本語", missing: false },
  ]);
  assert.deepEqual(languageChoices(null, null, named), []);
});

test("a language is named in its own language with a capital, and a tag that is no language stays as it is", () => {
  assert.equal(nativeName("en"), "English");
  assert.equal(nativeName("fr"), "Français");
  assert.equal(nativeName("x"), "x");
  assert.equal(nativeName("!!"), "!!");
});
