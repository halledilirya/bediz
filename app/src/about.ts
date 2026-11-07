import { invoke } from "@tauri-apps/api/core";
import { explain } from "./failure";
import { isolated, onLanguage, text } from "./i18n";
import { type Fetch, readNotices } from "./licences";
import "./styles/about.css";

const fetched: Fetch = async (path) => {
  const answer = await fetch(path);
  return {
    ok: answer.ok,
    type: answer.headers.get("content-type"),
    text: () => answer.text(),
  };
};

export async function enableAbout(): Promise<void> {
  const [name, error, licences, logs, source, dialog, notices] = [
    "about-name",
    "about-error",
    "show-licences",
    "open-logs",
    "open-source",
    "licences",
    "licences-text",
  ].map((id) => document.getElementById(id));
  if (
    !name ||
    !error ||
    !licences ||
    !logs ||
    !source ||
    !(dialog instanceof HTMLDialogElement) ||
    !notices
  ) {
    throw new Error("the about section is incomplete");
  }
  const shown: { failed: boolean; failure: unknown } = {
    failed: false,
    failure: null,
  };
  const failed = (failure: unknown): void => {
    shown.failed = true;
    shown.failure = failure;
    error.textContent = text("settings-about-failed", {
      reason: explain(failure, text),
    });
    error.hidden = false;
  };
  const version = await invoke<string>("version").catch(() => "");
  const named = (): void => {
    name.textContent = text("settings-about", { version });
  };
  named();
  onLanguage(() => {
    named();
    if (shown.failed) {
      failed(shown.failure);
    }
  });
  logs.addEventListener("click", () => {
    invoke("open_logs").catch(failed);
  });
  source.addEventListener("click", () => {
    invoke("open_source").catch(failed);
  });
  licences.addEventListener("click", () => {
    const missing = isolated(text("settings-licences-missing"));
    void readNotices(fetched, missing).then((all) => {
      notices.textContent = all;
      dialog.showModal();
    });
  });
}
