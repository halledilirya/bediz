import "./styles/main.css";
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { enableAbout } from "./about";
import { behaveLikeAnApp } from "./desktop";
import { relocalize } from "./i18n";
import { followLanguage } from "./language";
import { enableSearch } from "./search";
import { enableSections } from "./sections";
import { showSettings } from "./settings";

behaveLikeAnApp();

async function start(): Promise<void> {
  const language = followLanguage(() => invoke<string>("language"), relocalize);
  await listen("language-changed", () => {
    void language.refresh().catch(() => undefined);
  }).catch(() => undefined);
  await language.refresh();
  const pane = document.querySelector<HTMLFormElement>("#pane");
  const rail = document.querySelector<HTMLElement>(".rail");
  const search = document.querySelector<HTMLInputElement>("#search");
  const error = document.querySelector<HTMLElement>(".save-error");
  if (!pane || !rail || !search || !error) {
    return;
  }
  await showSettings(pane, error);
  await enableAbout();
  const follow = enableSections(rail, pane);
  enableSearch(search, pane, rail, follow);
}

void start().finally(() => invoke("settings_ready"));
