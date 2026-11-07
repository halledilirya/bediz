export interface Saving<T> {
  read: () => T;
  send: (wanted: T, shown: T) => Promise<void>;
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
  let showing = saving.read();
  const load = async (): Promise<void> => {
    // An answer asked for before a change may not hold it yet, so it is asked for again.
    for (;;) {
      const asked = changes;
      const [stored, revision] = await saving.stored();
      if (changes === asked) {
        if (revision > shown) {
          shown = revision;
          saving.show(stored);
          showing = saving.read();
        }
        return;
      }
    }
  };
  return {
    load,
    changed: () => {
      changes += 1;
      saving.cleared();
      const before = showing;
      showing = saving.read();
      void saving.send(showing, before).catch((reason: unknown) => {
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
