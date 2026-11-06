import { text } from "./i18n";
import { LONGEST, type Problem, problem, sampleAt, stem } from "./pattern";
import "./styles/file-name.css";

const EXAMPLE_WIDTH = 1280;
const EXAMPLE_HEIGHT = 720;

export interface FileName {
  show: (pattern: string) => void;
  valid: () => string;
}

function explain(found: Problem): string {
  switch (found.kind) {
    case "empty":
      return text("settings-file-name-empty");
    case "long":
      return text("settings-file-name-long", { limit: LONGEST });
    case "forbidden":
      return text("settings-file-name-forbidden");
    case "unknown":
      return text("settings-file-name-unknown", { token: found.token });
  }
}

export function enableFileName(
  form: HTMLFormElement,
  changed: () => void,
): FileName {
  const input = form.elements.namedItem("fileName");
  const example = document.getElementById("file-name-example");
  if (!(input instanceof HTMLInputElement) || example === null) {
    throw new Error("the file name field is missing");
  }
  let valid = input.value;
  const describe = (): boolean => {
    const found = problem(input.value);
    example.classList.toggle("bad", found !== null);
    if (found !== null) {
      example.textContent = explain(found);
      return false;
    }
    const sample = sampleAt(new Date(), EXAMPLE_WIDTH, EXAMPLE_HEIGHT);
    example.textContent = text("settings-file-name-example", {
      name: `${stem(input.value, sample)}.png`,
    });
    return true;
  };

  input.addEventListener("input", () => {
    if (describe()) {
      valid = input.value;
      changed();
    }
  });
  for (const button of document.querySelectorAll<HTMLButtonElement>(
    "[data-token]",
  )) {
    button.addEventListener("click", () => {
      const start = input.selectionStart ?? input.value.length;
      const end = input.selectionEnd ?? start;
      input.setRangeText(button.dataset.token ?? "", start, end, "end");
      input.focus();
      input.dispatchEvent(new Event("input"));
    });
  }

  return {
    show: (pattern) => {
      input.value = pattern;
      valid = pattern;
      describe();
    },
    valid: () => valid,
  };
}
