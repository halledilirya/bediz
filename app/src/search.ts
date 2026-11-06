import { attribute, text } from "./i18n";
import "./styles/search.css";

function fold(value: string): string {
  return value
    .toLocaleLowerCase(document.documentElement.lang)
    .replaceAll("ı", "i")
    .normalize("NFD")
    .replace(/\p{M}/gu, "");
}

export function enableSearch(
  input: HTMLInputElement,
  pane: HTMLElement,
  rail: HTMLElement,
  filtered: () => void,
): void {
  input.placeholder = text("settings-search-placeholder");
  const rows = [...pane.querySelectorAll<HTMLElement>("[data-search]")].map(
    (row) => {
      const shown = [...row.querySelectorAll(".row-text > *")].map(
        (element) => element.textContent,
      );
      const keywords = attribute(row.dataset.search ?? "", "keywords");
      return { row, words: fold([...shown, keywords].join(" ")) };
    },
  );
  const sections = [...pane.querySelectorAll<HTMLElement>("section")];
  const empty = pane.querySelector<HTMLElement>(".empty");
  const apply = (): void => {
    const terms = fold(input.value)
      .split(/\s+/)
      .filter((term) => term !== "");
    pane.classList.toggle("searching", terms.length > 0);
    for (const { row, words } of rows) {
      row.hidden = !terms.every((term) => words.includes(term));
    }
    let found = false;
    for (const section of sections) {
      const shown =
        section.querySelector("[data-search]:not([hidden])") !== null;
      section.hidden = !shown;
      found ||= shown;
      const button = rail.querySelector<HTMLElement>(
        `[data-section="${section.id}"]`,
      );
      if (button) {
        button.hidden = !shown;
      }
    }
    if (empty) {
      empty.hidden = found;
      empty.textContent = found
        ? ""
        : text("settings-search-empty", { query: input.value.trim() });
    }
    filtered();
  };
  input.addEventListener("input", apply);
  input.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && input.value !== "") {
      input.value = "";
      apply();
    }
  });
  document.addEventListener("keydown", (event) => {
    const find = event.key.toLowerCase() === "f";
    if (find && event.ctrlKey && !event.altKey && !event.shiftKey) {
      input.focus();
      input.select();
    }
  });
}
