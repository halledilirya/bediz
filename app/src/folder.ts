import { invoke } from "@tauri-apps/api/core";
import { text } from "./i18n";
import "./styles/folder.css";

interface Folder {
  path: string;
  oneDrive: boolean;
}

export interface FolderRow {
  show: (folder: string | null) => void;
}

export function enableFolder(form: HTMLFormElement): FolderRow {
  const field = form.elements.namedItem("saveFolder");
  const [path, error, note, open, change, reset] = [
    "folder-path",
    "folder-error",
    "onedrive-note",
    "open-folder",
    "change-folder",
    "reset-folder",
  ].map((id) => document.getElementById(id));
  if (
    !(field instanceof HTMLInputElement) ||
    !path ||
    !error ||
    !note ||
    !open ||
    !change ||
    !reset
  ) {
    throw new Error("the folder row is incomplete");
  }
  const chosen = (): string | null => (field.value === "" ? null : field.value);
  const failed = (reason: unknown): void => {
    error.textContent = text("settings-folder-failed", {
      reason: String(reason),
    });
    error.hidden = false;
  };
  let asked = 0;
  const describe = async (): Promise<void> => {
    asked += 1;
    const ask = asked;
    reset.hidden = chosen() === null;
    try {
      const folder = await invoke<Folder>("save_folder", { chosen: chosen() });
      if (ask === asked) {
        path.textContent = folder.path;
        path.title = folder.path;
        note.hidden = !folder.oneDrive;
        error.hidden = true;
      }
    } catch (reason) {
      if (ask === asked) {
        path.textContent = "";
        note.hidden = true;
        failed(reason);
      }
    }
  };
  const use = (folder: string | null): void => {
    field.value = folder ?? "";
    field.dispatchEvent(new Event("change", { bubbles: true }));
    void describe();
  };

  open.addEventListener("click", () => {
    invoke("open_save_folder", { chosen: chosen() }).catch(failed);
  });
  change.addEventListener("click", () => {
    invoke<string | null>("choose_save_folder", { chosen: chosen() })
      .then((picked) => {
        if (picked !== null) {
          use(picked);
        }
      })
      .catch(failed);
  });
  reset.addEventListener("click", () => {
    use(null);
  });

  return {
    show: (folder) => {
      field.value = folder ?? "";
      void describe();
    },
  };
}
