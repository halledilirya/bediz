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
import { enableUpdates } from "./updates";

behaveLikeAnApp();

const reveal = (): void => {
  const settings = document.querySelector<HTMLElement>(".settings");
  if (settings) {
    settings.hidden = false;
  }
};

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
  await enableUpdates(pane);
  await enableAbout();
  const follow = enableSections(rail, pane);
  enableSearch(search, pane, rail, follow);
  const jump = (section: string | null): void => {
    const button = [
      ...rail.querySelectorAll<HTMLButtonElement>("button[data-section]"),
    ].find((candidate) => candidate.dataset.section === section);
    if (!button) {
      return;
    }
    if (search.value !== "") {
      search.value = "";
      search.dispatchEvent(new Event("input"));
    }
    button.click();
  };
  const jumpWhereAsked = async (): Promise<void> => {
    jump(await invoke<string | null>("take_section").catch(() => null));
  };
  reveal();
  follow();
  await listen("show-section", () => {
    void jumpWhereAsked();
  }).catch(() => undefined);
  await jumpWhereAsked();
}

void start().finally(reveal);
