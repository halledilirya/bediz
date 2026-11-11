import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { held, keysOf, pick } from "./hotkey";
import { onLanguage, text } from "./i18n";
import "./styles/shortcuts.css";

type State = "registered" | "snipping-tool" | "taken";
type Name = "region" | "gif";

export interface Shortcuts {
  show: (hotkey: string, gifHotkey: string) => void;
}

interface Row {
  name: Name;
  field: HTMLInputElement;
  keys: HTMLElement;
  record: HTMLElement;
  remove: HTMLElement | null;
  error: HTMLElement;
  status: HTMLElement;
  chip: HTMLElement;
  detail: HTMLElement;
  take: HTMLElement;
  retry: HTMLElement;
  state: State | null;
  problem: (() => string) | null;
}

function findRow(
  form: HTMLFormElement,
  name: Name,
  fieldName: string,
  ids: string[],
  removeId: string | null,
): Row {
  const field = form.elements.namedItem(fieldName);
  const [keys, record, error, status, chip, detail, take, retry] = ids.map(
    (id) => document.getElementById(id),
  );
  const remove = removeId === null ? null : document.getElementById(removeId);
  if (
    !(field instanceof HTMLInputElement) ||
    !keys ||
    !record ||
    !error ||
    !status ||
    !chip ||
    !detail ||
    !take ||
    !retry ||
    (removeId !== null && !remove)
  ) {
    throw new Error(`the ${name} shortcut is incomplete`);
  }
  return {
    name,
    field,
    keys,
    record,
    remove,
    error,
    status,
    chip,
    detail,
    take,
    retry,
    state: null,
    problem: null,
  };
}

export async function enableShortcuts(
  form: HTMLFormElement,
): Promise<Shortcuts> {
  const region = findRow(
    form,
    "region",
    "hotkey",
    [
      "hotkey-keys",
      "record-hotkey",
      "hotkey-error",
      "hotkey-status",
      "hotkey-chip",
      "hotkey-detail",
      "take-print-screen",
      "retry-hotkey",
    ],
    null,
  );
  const gif = findRow(
    form,
    "gif",
    "gifHotkey",
    [
      "gif-hotkey-keys",
      "record-gif-hotkey",
      "gif-hotkey-error",
      "gif-hotkey-status",
      "gif-hotkey-chip",
      "gif-hotkey-detail",
      "gif-take-print-screen",
      "gif-retry-hotkey",
    ],
    "remove-gif-hotkey",
  );
  const rows = [region, gif];
  const other = (row: Row): Row => (row === region ? gif : region);
  let listening: Row | null = null;

  const drawKeys = (row: Row, names: string[], waiting: boolean): void => {
    const keys = row.keys;
    keys.replaceChildren();
    keys.classList.toggle("listening", waiting);
    if (names.length > 0) {
      keys.dir = "ltr";
    } else {
      keys.removeAttribute("dir");
    }
    if (names.length === 0) {
      keys.textContent = waiting
        ? text("settings-hotkey-listening")
        : text("settings-hotkey-none");
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
  const drawStatus = (row: Row): void => {
    const key = row.field.value;
    const state = key === "" ? null : row.state;
    if (row.remove) {
      row.remove.hidden = key === "" || listening === row;
    }
    row.status.hidden =
      state === null || (row === gif && state === "registered");
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
    row.chip.className = `chip ${look}`;
    row.chip.textContent = said;
    row.detail.textContent = explained;
    row.take.hidden = state !== "snipping-tool";
    row.retry.hidden = state !== "taken";
  };
  const problem = (row: Row, said: (() => string) | null): void => {
    row.problem = said;
    row.error.textContent = said ? said() : "";
    row.error.hidden = said === null;
  };
  const release = (row: Row): void => {
    listening = null;
    row.record.textContent = text("settings-hotkey-change");
    drawKeys(row, keysOf(row.field.value), false);
    drawStatus(row);
  };
  const stop = (): void => {
    if (listening === null) {
      return;
    }
    release(listening);
    void invoke("record_hotkey", { recording: false });
  };
  const startListening = (row: Row): void => {
    const before = listening;
    if (before) {
      release(before);
    }
    listening = row;
    problem(row, null);
    row.record.textContent = text("settings-hotkey-cancel");
    drawKeys(row, [], true);
    drawStatus(row);
    if (!before) {
      void invoke("record_hotkey", { recording: true });
    }
  };
  const taken = (row: Row, hotkey: string): boolean =>
    hotkey !== "" && hotkey === other(row).field.value;
  const choose = (row: Row, hotkey: string): void => {
    if (taken(row, hotkey)) {
      problem(row, () => text("settings-hotkey-same", { key: hotkey }));
      return;
    }
    problem(row, null);
    const changed = hotkey !== row.field.value;
    row.field.value = hotkey;
    if (changed) {
      row.state = null;
    }
    if (listening === row) {
      stop();
    } else {
      drawKeys(row, keysOf(hotkey), false);
      drawStatus(row);
    }
    if (changed) {
      row.field.dispatchEvent(new Event("change", { bubbles: true }));
    }
  };
  const pressed = (row: Row, event: KeyboardEvent): void => {
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
          row,
          held({
            ctrl: event.ctrlKey,
            alt: event.altKey,
            shift: event.shiftKey,
          }),
          true,
        );
        return;
      case "unsupported":
        problem(row, () => text("settings-hotkey-unsupported"));
        return;
      case "needs-modifier":
        problem(row, () =>
          text("settings-hotkey-needs-modifier", { key: picked.key }),
        );
        return;
      case "hotkey":
        choose(row, picked.hotkey);
    }
  };

  for (const row of rows) {
    row.record.addEventListener("click", () => {
      if (listening === row) {
        stop();
      } else {
        startListening(row);
      }
    });
    row.take.addEventListener("click", () => {
      void invoke("take_print_screen");
    });
    row.retry.addEventListener("click", () => {
      void invoke("retry_hotkey");
    });
  }
  gif.remove?.addEventListener("click", () => {
    choose(gif, "");
  });
  document.addEventListener(
    "keydown",
    (event) => {
      if (listening) {
        pressed(listening, event);
      }
    },
    { capture: true },
  );
  // Windows sends Print Screen to a page only as a keyup.
  document.addEventListener(
    "keyup",
    (event) => {
      if (listening && event.code === "PrintScreen") {
        pressed(listening, event);
      }
    },
    { capture: true },
  );
  document.addEventListener("pointerdown", (event) => {
    const onRecord = rows.some((row) => event.target === row.record);
    if (listening && !onRecord) {
      stop();
    }
  });
  window.addEventListener("blur", stop);
  const heard = { region: false, gif: false };
  const hear = async (event: string, row: Row): Promise<void> => {
    await listen<State>(event, (heardEvent) => {
      heard[row.name] = true;
      row.state = heardEvent.payload;
      drawStatus(row);
    }).catch(() => undefined);
  };
  await hear("hotkey-state", region);
  await hear("gif-hotkey-state", gif);
  await listen<Name>("hotkey-pressed", (event) => {
    const row = listening;
    if (!row) {
      return;
    }
    const pressedRow = event.payload === row.name ? row : other(row);
    choose(row, pressedRow.field.value);
  }).catch(() => undefined);
  const asked = await invoke<State | null>("hotkey_state").catch(() => null);
  const askedGif = await invoke<State | null>("gif_hotkey_state").catch(
    () => null,
  );
  if (!heard.region) {
    region.state = asked;
  }
  if (!heard.gif) {
    gif.state = askedGif;
  }
  for (const row of rows) {
    drawStatus(row);
  }
  onLanguage(() => {
    for (const row of rows) {
      problem(row, row.problem);
      if (listening !== row) {
        drawKeys(row, keysOf(row.field.value), false);
      }
      drawStatus(row);
    }
  });

  return {
    show: (hotkey, gifHotkey) => {
      for (const [row, value] of [
        [region, hotkey],
        [gif, gifHotkey],
      ] as const) {
        row.field.value = value;
        if (listening !== row) {
          drawKeys(row, keysOf(value), false);
        }
        drawStatus(row);
      }
    },
  };
}
