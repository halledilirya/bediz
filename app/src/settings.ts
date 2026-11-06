import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enableFileName } from "./file-name";
import { enableFolder } from "./folder";
import { text } from "./i18n";
import { saveChanges } from "./saving";
import { enableShortcuts } from "./shortcuts";
import "./styles/settings.css";

export interface Settings {
  startWithWindows: boolean;
  hotkey: string;
  askAboutHotkey: boolean;
  printScreenYields: boolean;
  detectWindows: boolean;
  successLabels: boolean;
  enter: string;
  saveFolder: string | null;
  fileName: string;
  keepOutOfHistory: boolean;
  releaseGpuWhileIdle: boolean;
}

const SWITCHES = [
  "startWithWindows",
  "askAboutHotkey",
  "detectWindows",
  "successLabels",
  "keepOutOfHistory",
  "releaseGpuWhileIdle",
] as const;

const SAVED_WHILE_TYPING = "fileName";

export async function showSettings(
  form: HTMLFormElement,
  error: HTMLElement,
): Promise<void> {
  const control = (
    name: string,
  ): HTMLInputElement | HTMLSelectElement | null => {
    const found = form.elements.namedItem(name);
    return found instanceof HTMLInputElement ||
      found instanceof HTMLSelectElement
      ? found
      : null;
  };
  const checked = (name: string): boolean => {
    const found = control(name);
    return found instanceof HTMLInputElement && found.checked;
  };
  const value = (name: string): string => control(name)?.value ?? "";
  const warn = (message: string): void => {
    error.textContent = message;
    error.hidden = false;
  };
  const startError = document.getElementById("start-error");
  const startProblem: { heard: boolean } = { heard: false };
  const showStartProblem = (problem: string | null): void => {
    if (startError) {
      startError.textContent =
        problem === null
          ? ""
          : text("settings-start-problem", { reason: problem });
      startError.hidden = problem === null;
    }
  };
  let printScreenYields = false;
  const shortcuts = await enableShortcuts(form);
  const folder = enableFolder(form);
  const fileName = enableFileName(form, () => {
    saver.changed();
  });
  const saver = saveChanges<Settings>({
    read: () => ({
      startWithWindows: checked("startWithWindows"),
      hotkey: value("hotkey"),
      askAboutHotkey: checked("askAboutHotkey"),
      printScreenYields,
      detectWindows: checked("detectWindows"),
      successLabels: checked("successLabels"),
      enter: value("enter"),
      saveFolder: value("saveFolder") || null,
      fileName: fileName.valid(),
      keepOutOfHistory: checked("keepOutOfHistory"),
      releaseGpuWhileIdle: checked("releaseGpuWhileIdle"),
    }),
    send: async (wanted) => {
      await invoke("set_settings", { settings: wanted });
    },
    stored: () => invoke<[Settings, number]>("settings"),
    show: (settings) => {
      for (const name of SWITCHES) {
        const found = control(name);
        if (found instanceof HTMLInputElement) {
          found.checked = settings[name];
        }
      }
      const enter = control("enter");
      if (enter) {
        enter.value = settings.enter;
      }
      printScreenYields = settings.printScreenYields;
      shortcuts.show(settings.hotkey);
      folder.show(settings.saveFolder);
      fileName.show(settings.fileName);
    },
    failed: (reason) => {
      warn(text("settings-save-failed", { reason: String(reason) }));
    },
    cleared: () => {
      error.hidden = true;
    },
  });
  try {
    await listen<string>("settings-not-saved", (event) => {
      saver.notSaved(event.payload);
    });
    await listen("settings-changed", () => {
      void saver.load().catch(() => undefined);
    });
    await listen<string | null>("start-with-windows-problem", (event) => {
      startProblem.heard = true;
      showStartProblem(event.payload);
    });
  } catch (reason) {
    warn(text("settings-watch-failed", { reason: String(reason) }));
  }
  await saver.load();
  const problem = await invoke<string | null>(
    "start_with_windows_problem",
  ).catch(() => null);
  if (!startProblem.heard) {
    showStartProblem(problem);
  }
  form.addEventListener("change", (event) => {
    const target = event.target;
    if (
      (target instanceof HTMLInputElement ||
        target instanceof HTMLSelectElement) &&
      target.name !== "" &&
      target.name !== SAVED_WHILE_TYPING
    ) {
      saver.changed();
    }
  });
  form.addEventListener("submit", (event) => {
    event.preventDefault();
  });
}
