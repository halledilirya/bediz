export interface LanguageFollower {
  refresh: () => Promise<void>;
}

export function followLanguage(
  read: () => Promise<string>,
  apply: (language: string) => void,
): LanguageFollower {
  let reads = 0;
  return {
    refresh: async () => {
      reads += 1;
      const mine = reads;
      const language = await read();
      if (mine === reads) {
        apply(language);
      }
    },
  };
}
