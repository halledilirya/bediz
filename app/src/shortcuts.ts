import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { held, keysOf, pick } from "./hotkey";
import { onLanguage, text } from "./i18n";
import "./styles/shortcuts.css";

type State = "registered" | "snipping-tool" | "taken";

export interface Shortcuts {
  show: (hotkey: string) => void;
}

export async function enableShortcuts(
  form: HTMLFormElement,
): Promise<Shortcuts> {
  const field = form.elements.namedItem("hotkey");
  const [keys, record, error, status, chip, detail, take, retry] = [
    "hotkey-keys",
    "record-hotkey",
    "hotkey-error",
    "hotkey-status",
    "hotkey-chip",
    "hotkey-detail",
    "take-print-screen",
    "retry-hotkey",
  ].map((id) => document.getElementById(id));
  if (
    !(field instanceof HTMLInputElement) ||
    !keys ||
    !record ||
    !error ||
    !status ||
    !chip ||
    !detail ||
    !take ||
    !retry
  ) {
    throw new Error("the shortcuts section is incomplete");
  }
  let state: State | null = null;
  let recording = false;

  const drawKeys = (names: string[], waiting: boolean): void => {
    keys.replaceChildren();
    keys.classList.toggle("listening", waiting);
    if (waiting && names.length === 0) {
      keys.textContent = text("settings-hotkey-listening");
      return;
    }
    names.forEach((name, index) => {
      if (index > 0) {
        keys.append("+");
      }
      const cap = document.createElement("kbd");
      cap.textContent = name;
      keys.append(cap);
    });
    if (waiting) {
      keys.append("+ …");
    }
  };
  const drawStatus = (): void => {
    status.hidden = state === null;
    const key = field.value;
    const [look, said, explained] =
      state === "registered"
        ? [
            "ok",
            text("settings-hotkey-registered", { key }),
            text("settings-hotkey-registered-detail"),
          ]
        : state === "taken"
          ? [
              "danger",
              text("settings-hotkey-taken", { key }),
              text("settings-hotkey-taken-detail"),
            ]
          : [
              "warn",
              text("settings-hotkey-snipping-tool"),
              text("settings-hotkey-snipping-tool-detail"),
            ];
    chip.className = `chip ${look}`;
    chip.textContent = said;
    detail.textContent = explained;
    take.hidden = state !== "snipping-tool";
    retry.hidden = state !== "taken";
  };
  const shown: { problem: (() => string) | null } = { problem: null };
  const problem = (said: (() => string) | null): void => {
    shown.problem = said;
    error.textContent = said ? said() : "";
    error.hidden = said === null;
  };
  const stop = (): void => {
    if (!recording) {
      return;
    }
    recording = false;
    record.textContent = text("settings-hotkey-change");
    drawKeys(keysOf(field.value), false);
    void invoke("record_hotkey", { recording: false });
  };
  const choose = (hotkey: string): void => {
    problem(null);
    const changed = hotkey !== field.value;
    field.value = hotkey;
    stop();
    drawStatus();
    if (changed) {
      field.dispatchEvent(new Event("change", { bubbles: true }));
    }
  };
  const pressed = (event: KeyboardEvent): void => {
    event.preventDefault();
    event.stopImmediatePropagation();
    const bare = !event.ctrlKey && !event.altKey && !event.shiftKey;
    if (event.key === "Escape" && bare) {
      stop();
      return;
    }
    const picked = pick({
      // keyCode is the layout's virtual key RegisterHotKey matches; code only names a US key position.
      // eslint-disable-next-line @typescript-eslint/no-deprecated
      virtualKey: event.keyCode,
      ctrl: event.ctrlKey,
      alt: event.altKey,
      shift: event.shiftKey,
      meta: event.metaKey,
    });
    switch (picked.kind) {
      case "modifiers":
        drawKeys(
          held({
            ctrl: event.ctrlKey,
            alt: event.altKey,
            shift: event.shiftKey,
          }),
          true,
        );
        return;
      case "unsupported":
        problem(() => text("settings-hotkey-unsupported"));
        return;
      case "needs-modifier":
        problem(() =>
          text("settings-hotkey-needs-modifier", { key: picked.key }),
        );
        return;
      case "hotkey":
        choose(picked.hotkey);
    }
  };

  record.addEventListener("click", () => {
    if (recording) {
      stop();
      return;
    }
    recording = true;
    problem(null);
    record.textContent = text("settings-hotkey-cancel");
    drawKeys([], true);
    void invoke("record_hotkey", { recording: true });
  });
  document.addEventListener(
    "keydown",
    (event) => {
      if (recording) {
        pressed(event);
      }
    },
    { capture: true },
  );
  // Windows sends Print Screen to a page only as a keyup.
  document.addEventListener(
    "keyup",
    (event) => {
      if (recording && event.code === "PrintScreen") {
        pressed(event);
      }
    },
    { capture: true },
  );
  document.addEventListener("pointerdown", (event) => {
    if (recording && event.target !== record) {
      stop();
    }
  });
  window.addEventListener("blur", stop);
  take.addEventListener("click", () => {
    void invoke("take_print_screen");
  });
  retry.addEventListener("click", () => {
    void invoke("retry_hotkey");
  });
  const heard: { state: State | null } = { state: null };
  await listen<State>("hotkey-state", (event) => {
    heard.state = event.payload;
    state = event.payload;
    drawStatus();
  }).catch(() => undefined);
  await listen("hotkey-pressed", () => {
    if (recording) {
      choose(field.value);
    }
  }).catch(() => undefined);
  const asked = await invoke<State | null>("hotkey_state").catch(() => null);
  state = heard.state ?? asked;
  drawStatus();
  onLanguage(() => {
    problem(shown.problem);
    drawStatus();
  });

  return {
    show: (hotkey) => {
      field.value = hotkey;
      if (!recording) {
        drawKeys(keysOf(hotkey), false);
      }
      drawStatus();
    },
  };
}
