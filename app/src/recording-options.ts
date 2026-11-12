import { limit } from "./gif-facts";
import { onLanguage, text } from "./i18n";

export interface RecordingOptions {
  gifFrameRate: number;
  gifLongestSide: number | null;
  gifTimeLimit: number;
}

const KEPT = "";

export function enableRecordingOptions(form: HTMLFormElement): {
  read: () => RecordingOptions;
  show: (options: RecordingOptions) => void;
} {
  const select = (name: string): HTMLSelectElement | null => {
    const found = form.elements.namedItem(name);
    return found instanceof HTMLSelectElement ? found : null;
  };
  const number = (name: string): number => Number(select(name)?.value);
  const setValue = (name: string, wanted: string): void => {
    const found = select(name);
    if (found) {
      found.value = wanted;
    }
  };
  const label = (): void => {
    const language = document.documentElement.lang;
    for (const option of select("gifTimeLimit")?.options ?? []) {
      option.textContent = limit(Number(option.value), language);
    }
    for (const option of select("gifLongestSide")?.options ?? []) {
      if (option.value !== KEPT) {
        option.textContent = text("settings-gif-pixels", {
          pixels: option.value,
        });
      }
    }
  };
  label();
  onLanguage(label);
  return {
    read: () => {
      const side = select("gifLongestSide")?.value ?? KEPT;
      return {
        gifFrameRate: number("gifFrameRate"),
        gifLongestSide: side === KEPT ? null : Number(side),
        gifTimeLimit: number("gifTimeLimit"),
      };
    },
    show: (options) => {
      setValue("gifFrameRate", String(options.gifFrameRate));
      setValue(
        "gifLongestSide",
        options.gifLongestSide === null ? KEPT : String(options.gifLongestSide),
      );
      setValue("gifTimeLimit", String(options.gifTimeLimit));
    },
  };
}
