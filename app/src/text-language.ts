export interface LanguageChoice {
  tag: string;
  name: string;
  missing: boolean;
}

export function nativeName(tag: string): string {
  try {
    const name =
      new Intl.DisplayNames([tag], { type: "language" }).of(tag) ?? tag;
    return name.charAt(0).toLocaleUpperCase(tag) + name.slice(1);
  } catch {
    return tag;
  }
}

export function languageChoices(
  installed: readonly string[] | null,
  chosen: string | null,
  name: (tag: string) => string = nativeName,
): LanguageChoice[] {
  const known = installed ?? [];
  const choices = known
    .map((tag) => ({ tag, name: name(tag), missing: false }))
    .sort((one, other) => one.name.localeCompare(other.name));
  if (chosen !== null && !known.includes(chosen)) {
    choices.push({
      tag: chosen,
      name: name(chosen),
      missing: installed !== null,
    });
  }
  return choices;
}
