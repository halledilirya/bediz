import {
  FluentBundle,
  FluentResource,
  type FluentVariable,
} from "@fluent/bundle";
import english from "../../locales/en/bediz.ftl?raw";
import turkish from "../../locales/tr/bediz.ftl?raw";

const sources = new Map([
  ["en", english],
  ["tr", turkish],
]);

let bundle = create("en", english);

function create(language: string, source: string): FluentBundle {
  const created = new FluentBundle(language);
  created.addResource(new FluentResource(source));
  return created;
}

export function useLanguage(language: string): void {
  const source = sources.get(language);
  const chosen = source === undefined ? "en" : language;
  bundle = create(chosen, source ?? english);
  document.documentElement.lang = chosen;
}

export function text(
  id: string,
  args?: Record<string, FluentVariable>,
): string {
  const pattern = bundle.getMessage(id)?.value ?? null;
  return pattern === null ? id : bundle.formatPattern(pattern, args);
}

export function attribute(id: string, name: string): string {
  const pattern = bundle.getMessage(id)?.attributes[name] ?? null;
  return pattern === null ? "" : bundle.formatPattern(pattern);
}

export function localize(root: ParentNode): void {
  for (const element of root.querySelectorAll<HTMLElement>("[data-l10n-id]")) {
    const id = element.dataset.l10nId;
    if (id !== undefined) {
      element.textContent = text(id);
    }
  }
}
