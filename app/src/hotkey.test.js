import assert from "node:assert/strict";
import { test } from "node:test";
import { keysOf, pick } from "./hotkey.ts";

const A = 65;
const Q = 81;
const SEVEN = 55;
const SPACE = 32;
const PAGE_UP = 33;

const pressed = (virtualKey, held = {}) => ({
  virtualKey,
  ctrl: false,
  alt: false,
  shift: false,
  meta: false,
  ...held,
});

test("a letter with Ctrl becomes a shortcut", () => {
  assert.deepEqual(pick(pressed(A, { ctrl: true })), {
    kind: "hotkey",
    hotkey: "Ctrl+A",
  });
});

test("the letter is the one the keyboard layout types, wherever the key sits", () => {
  const azertyA = pressed(A, { ctrl: true, code: "KeyQ" });
  assert.deepEqual(pick(azertyA), { kind: "hotkey", hotkey: "Ctrl+A" });
});

test("modifiers are written in the order the settings file uses", () => {
  const all = { ctrl: true, alt: true, shift: true };
  assert.deepEqual(pick(pressed(SEVEN, all)), {
    kind: "hotkey",
    hotkey: "Ctrl+Alt+Shift+7",
  });
  assert.deepEqual(pick(pressed(PAGE_UP, { alt: true })), {
    kind: "hotkey",
    hotkey: "Alt+Page Up",
  });
});

test("print screen and the function keys may stand alone", () => {
  for (const [virtualKey, hotkey] of [
    [44, "Print Screen"],
    [112, "F1"],
    [116, "F5"],
    [135, "F24"],
    [19, "Pause"],
    [145, "Scroll Lock"],
  ]) {
    assert.deepEqual(pick(pressed(virtualKey)), { kind: "hotkey", hotkey });
  }
  assert.deepEqual(pick(pressed(120, { shift: true })), {
    kind: "hotkey",
    hotkey: "Shift+F9",
  });
});

test("a typing key needs Ctrl or Alt, even with Shift", () => {
  assert.deepEqual(pick(pressed(Q)), { kind: "needs-modifier", key: "Q" });
  assert.deepEqual(pick(pressed(Q, { shift: true })), {
    kind: "needs-modifier",
    key: "Shift+Q",
  });
  assert.deepEqual(pick(pressed(SPACE)), {
    kind: "needs-modifier",
    key: "Space",
  });
});

test("keys a shortcut cannot use are refused", () => {
  for (const virtualKey of [192, 97, 136, 27, 9, 229]) {
    assert.deepEqual(pick(pressed(virtualKey, { ctrl: true })), {
      kind: "unsupported",
    });
  }
  assert.deepEqual(pick(pressed(A, { ctrl: true, meta: true })), {
    kind: "unsupported",
  });
});

test("a modifier on its own waits for the key", () => {
  for (const virtualKey of [16, 17, 18, 91, 92]) {
    assert.deepEqual(pick(pressed(virtualKey)), { kind: "modifiers" });
  }
});

test("a shortcut splits into the keys to draw", () => {
  assert.deepEqual(keysOf("Ctrl+Shift+Print Screen"), [
    "Ctrl",
    "Shift",
    "Print Screen",
  ]);
  assert.deepEqual(keysOf(""), []);
});
