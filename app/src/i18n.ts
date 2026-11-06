import {
  FluentBundle,
  FluentResource,
  type FluentVariable,
} from "@fluent/bundle";

const ENGLISH = "en";
const RIGHT_TO_LEFT = new Set(["ar"]);
const FIRST_STRONG_ISOLATE = String.fromCodePoint(0x2068);
const POP_DIRECTIONAL_ISOLATE = String.fromCodePoint(0x2069);

const sources = new Map<string, string>();
for (const [path, source] of Object.entries(
  import.meta.glob<string>("../../locales/*/bediz.ftl", {
    query: "?raw",
    import: "default",
    eager: true,
  }),
)) {
  const code = /([^/]+)\/bediz\.ftl$/.exec(path)?.[1];
  if (code !== undefined) {
    sources.set(code, source);
  }
}

export const languages: readonly string[] = [
  ENGLISH,
  ...[...sources.keys()].filter((code) => code !== ENGLISH).sort(),
];

let bundle = create(ENGLISH);

function create(language: string): FluentBundle {
  const created = new FluentBundle(language);
  created.addResource(new FluentResource(sources.get(language) ?? ""));
  return created;
}

export function languageName(code: string): string {
  const named = create(code);
  const pattern = named.getMessage("language-name")?.value ?? null;
  return pattern === null ? code : named.formatPattern(pattern);
}

export function isolated(phrase: string): string {
  return `${FIRST_STRONG_ISOLATE}${phrase}${POP_DIRECTIONAL_ISOLATE}`;
}

export function useLanguage(language: string): void {
  const chosen = sources.has(language) ? language : ENGLISH;
  bundle = create(chosen);
  document.documentElement.lang = chosen;
  document.documentElement.dir = RIGHT_TO_LEFT.has(chosen) ? "rtl" : "ltr";
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

const redraws: (() => void)[] = [];

export function onLanguage(redraw: () => void): void {
  redraws.push(redraw);
}

export function relocalize(language: string): void {
  useLanguage(language);
  localize(document.body);
  for (const redraw of redraws) {
    redraw();
  }
}
