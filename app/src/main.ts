import "./styles/main.css";
import { invoke } from "@tauri-apps/api/core";
import { behaveLikeAnApp } from "./desktop";
import { localize, useLanguage } from "./i18n";
import { enableSearch } from "./search";
import { enableSections } from "./sections";
import { showSettings } from "./settings";

behaveLikeAnApp();

async function start(): Promise<void> {
  useLanguage(await invoke<string>("language"));
  localize(document.body);
  const pane = document.querySelector<HTMLFormElement>("#pane");
  const rail = document.querySelector<HTMLElement>(".rail");
  const search = document.querySelector<HTMLInputElement>("#search");
  const error = document.querySelector<HTMLElement>(".save-error");
  if (!pane || !rail || !search || !error) {
    return;
  }
  await showSettings(pane, error);
  const follow = enableSections(rail, pane);
  enableSearch(search, pane, rail, follow);
}

void start().finally(() => invoke("settings_ready"));
