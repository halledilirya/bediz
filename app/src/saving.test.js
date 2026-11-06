import assert from "node:assert/strict";
import { test } from "node:test";
import { saveChanges } from "./saving.ts";

function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((settle, fail) => {
    resolve = settle;
    reject = fail;
  });
  return { promise, resolve, reject };
}

function fakeWindow(initial) {
  const state = {
    switches: { ...initial },
    sent: [],
    reads: [],
    failures: [],
    cleared: 0,
  };
  const saver = saveChanges({
    read: () => ({ ...state.switches }),
    send: (wanted) => {
      const pending = deferred();
      state.sent.push({ wanted, ...pending });
      return pending.promise;
    },
    stored: () => {
      const pending = deferred();
      state.reads.push(pending);
      return pending.promise;
    },
    show: (settings) => {
      state.switches = { ...settings };
    },
    failed: (reason) => {
      state.failures.push(reason);
    },
    cleared: () => {
      state.cleared += 1;
    },
  });
  return { state, saver };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));
const OFF = { a: false, b: false };
const A_ON = { a: true, b: false };

test("loading shows the stored settings", async () => {
  const { state, saver } = fakeWindow(OFF);
  const loading = saver.load();
  state.reads.shift().resolve([A_ON, 4]);
  await loading;
  assert.deepEqual(state.switches, A_ON);
});

test("every change sends all the switches at once and in order", () => {
  const { state, saver } = fakeWindow(OFF);
  state.switches.a = true;
  saver.changed();
  state.switches.b = true;
  saver.changed();
  assert.deepEqual(
    state.sent.map((send) => send.wanted),
    [A_ON, { a: true, b: true }],
  );
});

test("a change hides the last error", () => {
  const { state, saver } = fakeWindow(OFF);
  saver.notSaved("the disk is full");
  saver.changed();
  assert.equal(state.cleared, 1);
});

test("a write that failed later shows what is stored", async () => {
  const { state, saver } = fakeWindow(OFF);
  state.switches.a = true;
  saver.changed();
  saver.notSaved("the disk is full");
  assert.deepEqual(state.failures, ["the disk is full"]);
  state.reads.shift().resolve([OFF, 2]);
  await settle();
  assert.deepEqual(state.switches, OFF);
});

test("an answer that arrives after a newer change does not redraw the switches", async () => {
  const { state, saver } = fakeWindow(OFF);
  state.switches.a = true;
  saver.changed();
  saver.notSaved("the disk is full");
  state.switches.b = true;
  saver.changed();
  state.reads.shift().resolve([OFF, 2]);
  await settle();
  assert.deepEqual(state.switches, { a: true, b: true });
  assert.deepEqual(state.sent.at(-1).wanted, { a: true, b: true });
});

test("a failure heard while the first load is on its way wins over it", async () => {
  const { state, saver } = fakeWindow(OFF);
  const loading = saver.load();
  saver.notSaved("the disk is full");
  state.reads[1].resolve([OFF, 2]);
  await settle();
  state.reads[0].resolve([A_ON, 1]);
  await loading;
  assert.deepEqual(state.switches, OFF);
  assert.deepEqual(state.failures, ["the disk is full"]);
});

test("an older answer that arrives last is not drawn", async () => {
  const { state, saver } = fakeWindow(OFF);
  const first = saver.load();
  const second = saver.load();
  state.reads[1].resolve([OFF, 6]);
  await second;
  state.reads[0].resolve([A_ON, 5]);
  await first;
  assert.deepEqual(state.switches, OFF);
});

test("a refused change reports and shows what is stored", async () => {
  const { state, saver } = fakeWindow(OFF);
  state.switches.a = true;
  saver.changed();
  state.sent.shift().reject("refused");
  await settle();
  assert.deepEqual(state.failures, ["refused"]);
  state.reads.shift().resolve([OFF, 2]);
  await settle();
  assert.deepEqual(state.switches, OFF);
});

test("when the stored settings cannot be read, the switches stay as they are", async () => {
  const { state, saver } = fakeWindow(OFF);
  state.switches.a = true;
  saver.notSaved("the disk is full");
  state.reads.shift().reject("gone");
  await settle();
  assert.deepEqual(state.switches, A_ON);
});
