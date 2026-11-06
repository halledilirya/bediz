import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { text } from "./i18n";
import { saveChanges } from "./saving";
import "./styles/settings.css";

export interface Settings {
  detectWindows: boolean;
  successLabels: boolean;
  releaseGpuWhileIdle: boolean;
}

const SWITCHES = [
  "detectWindows",
  "successLabels",
  "releaseGpuWhileIdle",
] as const;

type Switch = (typeof SWITCHES)[number];

export async function showSettings(
  form: HTMLFormElement,
  error: HTMLElement,
): Promise<void> {
  const switchFor = (name: Switch): HTMLInputElement | null => {
    const input = form.elements.namedItem(name);
    return input instanceof HTMLInputElement ? input : null;
  };
  const checked = (name: Switch): boolean => switchFor(name)?.checked ?? false;
  const warn = (message: string): void => {
    error.textContent = message;
    error.hidden = false;
  };
  const saver = saveChanges<Settings>({
    read: () => ({
      detectWindows: checked("detectWindows"),
      successLabels: checked("successLabels"),
      releaseGpuWhileIdle: checked("releaseGpuWhileIdle"),
    }),
    send: async (wanted) => {
      await invoke("set_settings", { settings: wanted });
    },
    stored: () => invoke<[Settings, number]>("settings"),
    show: (settings) => {
      for (const name of SWITCHES) {
        const input = switchFor(name);
        if (input) {
          input.checked = settings[name];
        }
      }
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
  } catch (reason) {
    warn(text("settings-watch-failed", { reason: String(reason) }));
  }
  await saver.load();
  form.addEventListener("change", (event) => {
    if (
      event.target instanceof HTMLInputElement &&
      event.target.role === "switch"
    ) {
      saver.changed();
    }
  });
  form.addEventListener("submit", (event) => {
    event.preventDefault();
  });
}
