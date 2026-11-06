export interface Saving<T> {
  read: () => T;
  send: (wanted: T) => Promise<void>;
  stored: () => Promise<[T, number]>;
  show: (settings: T) => void;
  failed: (reason: unknown) => void;
  cleared: () => void;
}

export interface Saver {
  load: () => Promise<void>;
  changed: () => void;
  notSaved: (reason: unknown) => void;
}

export function saveChanges<T>(saving: Saving<T>): Saver {
  let changes = 0;
  let shown = -1;
  const load = async (): Promise<void> => {
    const asked = changes;
    const [stored, revision] = await saving.stored();
    if (changes === asked && revision > shown) {
      shown = revision;
      saving.show(stored);
    }
  };
  return {
    load,
    changed: () => {
      changes += 1;
      saving.cleared();
      void saving.send(saving.read()).catch((reason: unknown) => {
        saving.failed(reason);
        void load().catch(() => undefined);
      });
    },
    notSaved: (reason) => {
      saving.failed(reason);
      void load().catch(() => undefined);
    },
  };
}
