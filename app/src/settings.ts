import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enableFileName } from "./file-name";
import { enableFolder } from "./folder";
import { isolated, languageName, languages, onLanguage, text } from "./i18n";
import { saveChanges } from "./saving";
import { enableShortcuts } from "./shortcuts";
import "./styles/settings.css";

export interface Settings {
  startWithWindows: boolean;
  language: string | null;
  theme: string;
  accent: string;
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
  checkForUpdates: boolean;
  lastRegion: unknown;
}

const SWITCHES = [
  "startWithWindows",
  "askAboutHotkey",
  "detectWindows",
  "successLabels",
  "keepOutOfHistory",
  "releaseGpuWhileIdle",
  "checkForUpdates",
] as const;

const SAVED_WHILE_TYPING = "fileName";
const LOOK = new Set(["theme", "accent"]);

function applyLook(theme: string, accent: string): void {
  const root = document.documentElement;
  if (theme === "light" || theme === "dark") {
    root.dataset.theme = theme;
  } else {
    delete root.dataset.theme;
  }
  if (accent === "blue") {
    delete root.dataset.accent;
  } else {
    root.dataset.accent = accent;
  }
}

export async function showSettings(
  form: HTMLFormElement,
  error: HTMLElement,
): Promise<void> {
  const control = (name: string): Element | RadioNodeList | null =>
    form.elements.namedItem(name);
  const checked = (name: string): boolean => {
    const found = control(name);
    return found instanceof HTMLInputElement && found.checked;
  };
  const value = (name: string): string => {
    const found = control(name);
    return found instanceof HTMLInputElement ||
      found instanceof HTMLSelectElement ||
      found instanceof RadioNodeList
      ? found.value
      : "";
  };
  const setValue = (name: string, wanted: string): void => {
    const found = control(name);
    if (found instanceof HTMLSelectElement || found instanceof RadioNodeList) {
      found.value = wanted;
    }
  };
  const language = control("language");
  if (language instanceof HTMLSelectElement) {
    for (const code of languages) {
      const option = new Option(isolated(languageName(code)), code);
      option.lang = code;
      language.add(option);
    }
  }
  const warning: { said: (() => string) | null } = { said: null };
  const warn = (said: () => string): void => {
    warning.said = said;
    error.textContent = said();
    error.hidden = false;
  };
  const startError = document.getElementById("start-error");
  const startProblem: { heard: boolean; shown: string | null } = {
    heard: false,
    shown: null,
  };
  const showStartProblem = (problem: string | null): void => {
    startProblem.shown = problem;
    if (startError) {
      startError.textContent =
        problem === null
          ? ""
          : text("settings-start-problem", { reason: problem });
      startError.hidden = problem === null;
    }
  };
  onLanguage(() => {
    if (warning.said) {
      error.textContent = warning.said();
    }
    showStartProblem(startProblem.shown);
  });
  const facts: Pick<Settings, "printScreenYields" | "lastRegion"> = {
    printScreenYields: false,
    lastRegion: null,
  };
  const shortcuts = await enableShortcuts(form);
  const folder = enableFolder(form);
  const fileName = enableFileName(form, () => {
    saver.changed();
  });
  const saver = saveChanges<Settings>({
    read: () => ({
      ...facts,
      startWithWindows: checked("startWithWindows"),
      language: value("language") || null,
      theme: value("theme"),
      accent: value("accent"),
      hotkey: value("hotkey"),
      askAboutHotkey: checked("askAboutHotkey"),
      detectWindows: checked("detectWindows"),
      successLabels: checked("successLabels"),
      enter: value("enter"),
      saveFolder: value("saveFolder") || null,
      fileName: fileName.valid(),
      keepOutOfHistory: checked("keepOutOfHistory"),
      releaseGpuWhileIdle: checked("releaseGpuWhileIdle"),
      checkForUpdates: checked("checkForUpdates"),
    }),
    send: async (wanted, shown) => {
      await invoke("set_settings", { settings: wanted, shown });
    },
    stored: () => invoke<[Settings, number]>("settings"),
    show: (settings) => {
      for (const name of SWITCHES) {
        const found = control(name);
        if (found instanceof HTMLInputElement) {
          found.checked = settings[name];
        }
      }
      setValue("language", settings.language ?? "");
      setValue("theme", settings.theme);
      setValue("accent", settings.accent);
      setValue("enter", settings.enter);
      applyLook(settings.theme, settings.accent);
      facts.printScreenYields = settings.printScreenYields;
      facts.lastRegion = settings.lastRegion;
      shortcuts.show(settings.hotkey);
      folder.show(settings.saveFolder);
      fileName.show(settings.fileName);
    },
    failed: (reason) => {
      warn(() => text("settings-save-failed", { reason: String(reason) }));
    },
    cleared: () => {
      warning.said = null;
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
    warn(() => text("settings-watch-failed", { reason: String(reason) }));
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
      if (LOOK.has(target.name)) {
        applyLook(value("theme"), value("accent"));
      }
    }
  });
  form.addEventListener("submit", (event) => {
    event.preventDefault();
  });
}
