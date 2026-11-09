import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { onLanguage, text } from "./i18n";

interface UpdateStatus {
  state:
    | "not-checked"
    | "checking"
    | "up-to-date"
    | "available"
    | "downloading"
    | "installing"
    | "check-failed"
    | "install-failed";
  version?: string;
  percent?: number | null;
  problem?: string;
}

const BUSY = new Set(["checking", "downloading", "installing"]);
const INSTALLABLE = new Set(["available", "install-failed"]);

function explain(problem: string | undefined): string {
  switch (problem) {
    case "unreachable":
      return text("settings-update-unreachable");
    case "no-release":
      return text("settings-update-no-release");
    case "signature":
      return text("settings-update-signature");
    case "busy":
      return text("settings-update-busy");
    case "settings-unsaved":
      return text("settings-update-settings-unsaved");
    case "installer":
      return text("settings-update-installer");
    default:
      return text("settings-update-unexpected");
  }
}

function describe(status: UpdateStatus): string {
  const version = status.version ?? "";
  const reason = explain(status.problem);
  switch (status.state) {
    case "checking":
      return text("settings-update-checking");
    case "up-to-date":
      return text("settings-update-up-to-date");
    case "available":
      return text("settings-update-available", { version });
    case "downloading":
      return typeof status.percent === "number"
        ? text("settings-update-downloading", {
            version,
            percent: status.percent,
          })
        : text("settings-update-downloading-unsized", { version });
    case "installing":
      return text("settings-update-installing", { version });
    case "check-failed":
      return text("settings-update-check-failed", { reason });
    case "install-failed":
      return text("settings-update-install-failed", { version, reason });
    case "not-checked":
      return text("settings-update-not-checked");
  }
}

export async function enableUpdates(pane: HTMLElement): Promise<void> {
  if (!(await invoke<boolean>("updates_offered").catch(() => true))) {
    document.getElementById("updates")?.remove();
    document.querySelector('[data-section="updates"]')?.remove();
    return;
  }
  const [name, line, action] = [
    "update-name",
    "update-said",
    "update-action",
  ].map((id) => document.getElementById(id));
  if (!name || !line || !(action instanceof HTMLButtonElement)) {
    throw new Error("the updates section is incomplete");
  }
  const version = await invoke<string>("version").catch(() => "");
  const shown: { status: UpdateStatus; heard: boolean } = {
    status: { state: "not-checked" },
    heard: false,
  };
  const draw = (): void => {
    const { status } = shown;
    const installable = INSTALLABLE.has(status.state);
    name.textContent = text("settings-update-version", { version });
    line.textContent = describe(status);
    action.textContent = installable
      ? text("settings-update-install")
      : text("settings-update-check");
    action.classList.toggle("primary", installable);
    action.disabled = BUSY.has(status.state);
    pane.inert = status.state === "installing";
  };
  const show = (status: UpdateStatus): void => {
    shown.status = status;
    draw();
  };
  onLanguage(draw);
  action.addEventListener("click", () => {
    const before = shown.status;
    const installing = INSTALLABLE.has(before.state);
    invoke(installing ? "install_update" : "check_for_updates").catch(() => {
      show(
        installing
          ? { ...before, state: "install-failed", problem: "other" }
          : { state: "check-failed", problem: "other" },
      );
    });
  });
  await listen<UpdateStatus>("update-status", (event) => {
    shown.heard = true;
    show(event.payload);
  }).catch(() => undefined);
  const status = await invoke<UpdateStatus>("update_status").catch(() => null);
  if (status !== null && !shown.heard) {
    show(status);
  } else {
    draw();
  }
}
