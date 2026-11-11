// One page, two faces: the search panel (`#panel`) and the window (`#main`).

import * as api from "./api";
import { mountGlints } from "./fx";
import { setLanguage } from "./i18n";
import { mountPanel } from "./panel";
import { applyAppearance } from "./ui";
import { mountWindow } from "./window";

async function start(): Promise<void> {
  const root = document.getElementById("app")!;
  const settings = await api.getConfig();
  setLanguage(settings.language);
  applyAppearance(settings.theme, settings.accent);
  matchMedia("(prefers-color-scheme: dark)").addEventListener("change", () =>
    api.getConfig().then((now) => applyAppearance(now.theme, now.accent)),
  );
  api.on<api.Config>("config", (now) => applyAppearance(now.theme, now.accent));
  // The page is the app: no browser menu, no reload keys.
  window.addEventListener("contextmenu", (event) => {
    if (!(event.target instanceof HTMLInputElement || event.target instanceof HTMLTextAreaElement)) event.preventDefault();
  });
  mountGlints();
  if (location.hash === "#panel") await mountPanel(root);
  else await mountWindow(root);
}

void start();
