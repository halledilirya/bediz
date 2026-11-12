import "./styles/main.css";
import "./styles/button.css";
import "./styles/gif.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { behaveLikeAnApp } from "./desktop";
import { length, percent, size } from "./gif-facts";
import { relocalize, text } from "./i18n";
import { followLanguage } from "./language";
import { applyLook } from "./look";

interface Problem {
  id: string;
  reason: string | null;
}

type Copy =
  | { state: "notYet" }
  | { state: "done" }
  | { state: "failed"; problem: Problem };

type View =
  | { state: "empty" }
  | { state: "saving"; id: number; percent: number | null }
  | {
      state: "saved";
      id: number;
      name: string;
      bytes: number;
      milliseconds: number;
      width: number;
      height: number;
      warning: "gitHub" | "x" | null;
      copy: Copy;
    }
  | { state: "notSaved"; id: number; problem: Problem };

type Failure =
  | { kind: "missing"; path: string }
  | { kind: "changed" }
  | { kind: "notShown" }
  | { kind: "windows"; reason: string }
  | { kind: "unexpected" };

interface Look {
  theme: string;
  accent: string;
}

const LARGEST_PREVIEW = 100_000_000;

behaveLikeAnApp();

function found<T extends HTMLElement>(id: string, kind: new () => T): T | null {
  const element = document.getElementById(id);
  return element instanceof kind ? element : null;
}

function isFailure(value: unknown): value is Failure {
  return typeof value === "object" && value !== null && "kind" in value;
}

function shownId(view: View): number | null {
  return view.state === "empty" ? null : view.id;
}

function problemText(problem: Problem): string {
  return problem.reason === null
    ? text(problem.id)
    : text(problem.id, { reason: problem.reason });
}

function failureText(failure: unknown): string {
  if (!isFailure(failure)) {
    return text("gif-failed", { reason: String(failure) });
  }
  switch (failure.kind) {
    case "missing":
      return text("gif-missing");
    case "changed":
      return text("gif-changed");
    case "windows":
      return text("gif-failed", { reason: failure.reason });
    case "notShown":
    case "unexpected":
      return text("gif-failed", {
        reason: text("settings-reason-unexpected"),
      });
  }
}

async function start(): Promise<void> {
  const page = document.querySelector<HTMLElement>(".gif");
  const picture = found("picture", HTMLImageElement);
  const waiting = found("waiting", HTMLElement);
  const noPreview = found("no-preview", HTMLElement);
  const name = found("name", HTMLElement);
  const details = found("details", HTMLElement);
  const warning = found("warning", HTMLElement);
  const copied = found("copied", HTMLElement);
  const problem = found("problem", HTMLElement);
  const buttons = ["copy", "show", "open", "delete"].map((id) =>
    found(id, HTMLButtonElement),
  );
  const [copy, show, open, remove] = buttons;
  if (
    !page ||
    !picture ||
    !waiting ||
    !noPreview ||
    !name ||
    !details ||
    !warning ||
    !copied ||
    !problem ||
    !copy ||
    !show ||
    !open ||
    !remove
  ) {
    return;
  }
  const state: {
    view: View;
    previewOf: number | null;
    preview: string | null;
    previewFailed: boolean;
    failure: unknown;
    asked: number;
  } = {
    view: { state: "empty" },
    previewOf: null,
    preview: null,
    previewFailed: false,
    failure: null,
    asked: 0,
  };

  const render = (): void => {
    const { view } = state;
    const language = document.documentElement.lang;
    const saved = view.state === "saved" ? view : null;
    const saving = view.state === "saving" ? view : null;
    waiting.hidden = saving === null;
    waiting.textContent =
      saving?.percent == null
        ? text("gif-saving")
        : text("gif-saving-percent", {
            percent: percent(saving.percent, language),
          });
    picture.hidden = saved === null || state.preview === null;
    noPreview.hidden = saved === null || !state.previewFailed;
    name.textContent = saved?.name ?? "";
    details.textContent =
      saved === null
        ? ""
        : text("gif-details", {
            width: String(saved.width),
            height: String(saved.height),
            length: length(saved.milliseconds, language),
            size: size(saved.bytes, language),
          });
    const warned =
      saved?.warning === "x"
        ? text("gif-warning-x")
        : saved?.warning === "gitHub"
          ? text("gif-warning-github")
          : null;
    warning.textContent = warned ?? "";
    warning.hidden = warned === null;
    copied.textContent = text("gif-copied");
    copied.hidden = saved?.copy.state !== "done";
    const said =
      state.failure !== null
        ? failureText(state.failure)
        : view.state === "notSaved"
          ? problemText(view.problem)
          : saved?.copy.state === "failed"
            ? problemText(saved.copy.problem)
            : null;
    problem.textContent = said ?? "";
    problem.hidden = said === null;
    for (const button of buttons) {
      if (button) {
        button.disabled = saved === null;
      }
    }
  };

  const forgetPreview = (): void => {
    if (state.preview !== null) {
      URL.revokeObjectURL(state.preview);
    }
    state.previewOf = null;
    state.preview = null;
    state.previewFailed = false;
    picture.removeAttribute("src");
  };

  const loadPreview = async (id: number): Promise<void> => {
    try {
      const bytes = await invoke<ArrayBuffer>("gif_preview", { id });
      if (state.previewOf !== id) {
        return;
      }
      state.preview = URL.createObjectURL(
        new Blob([bytes], { type: "image/gif" }),
      );
      picture.src = state.preview;
    } catch {
      if (state.previewOf !== id) {
        return;
      }
      state.previewFailed = true;
    }
    render();
  };

  const load = async (): Promise<void> => {
    state.asked += 1;
    const asked = state.asked;
    const view = await invoke<View>("gif").catch((): View => ({
      state: "empty",
    }));
    if (asked !== state.asked) {
      return;
    }
    if (view.state === "empty") {
      await getCurrentWindow().close();
      return;
    }
    if (view.id !== shownId(state.view)) {
      state.failure = null;
    }
    const wanted = view.state === "saved" ? view.id : null;
    if (wanted !== state.previewOf) {
      forgetPreview();
      if (view.state === "saved") {
        state.previewOf = view.id;
        if (view.bytes > LARGEST_PREVIEW) {
          state.previewFailed = true;
        } else {
          void loadPreview(view.id);
        }
      }
    }
    state.view = view;
    render();
    page.hidden = false;
  };

  const act = (command: string): void => {
    if (state.view.state !== "saved") {
      return;
    }
    const { id } = state.view;
    state.failure = null;
    render();
    void invoke(command, { id }).catch((failure: unknown) => {
      if (shownId(state.view) !== id) {
        return;
      }
      if (isFailure(failure) && failure.kind === "notShown") {
        void load();
        return;
      }
      state.failure = failure;
      render();
    });
  };

  const showLook = async (): Promise<void> => {
    const [look] = await invoke<[Look, number]>("settings");
    applyLook(look.theme, look.accent);
  };

  const language = followLanguage(
    () => invoke<string>("language"),
    (code) => {
      relocalize(code);
      render();
    },
  );
  await listen("gif-changed", () => {
    void load();
  });
  await listen("language-changed", () => {
    void language.refresh().catch(() => undefined);
  });
  await listen("look-changed", () => {
    void showLook().catch(() => undefined);
  });
  await showLook().catch(() => undefined);
  await language.refresh();
  copy.addEventListener("click", () => {
    copied.hidden = true;
    act("copy_gif");
  });
  show.addEventListener("click", () => {
    act("show_gif");
  });
  open.addEventListener("click", () => {
    act("open_gif");
  });
  remove.addEventListener("click", () => {
    act("delete_gif");
  });
  document.addEventListener("keydown", (event) => {
    if (event.key === "Escape") {
      void getCurrentWindow().close();
    }
  });
  await load();
}

void start().catch(() => {
  const page = document.querySelector<HTMLElement>(".gif");
  if (page) {
    page.hidden = false;
  }
});
