// The search panel: it opens over whatever you're doing, finds the file, and gets out of the way.

import * as api from "./api";
import { IndexState } from "./api";
import { Finder } from "./finder";
import { t } from "./i18n";
import { Preview, row } from "./results";
import { followPointer, h, icon, number } from "./ui";

export function statusLine(state: IndexState, files: number | null): { text: string; mood: "" | "working" | "waiting" } {
  if (state.waiting_for === "model") return { text: t("status.model"), mood: "waiting" };
  if (state.paused || state.waiting_for === "paused") return { text: t("status.paused"), mood: "waiting" };
  if (state.waiting_for === "battery") return { text: t("status.battery"), mood: "waiting" };
  if (state.running) {
    if (state.phase === "crawling") return { text: t("status.crawling"), mood: "working" };
    if (state.phase === "loading") return { text: t("status.loading"), mood: "working" };
    const key = state.phase === "deepening" ? "status.deepening" : "status.reading";
    return { text: t(key, { done: number(Math.min(state.done, state.total)), total: number(state.total) }), mood: "working" };
  }
  return { text: files === null ? t("status.upToDate") : t("status.files", { n: number(files) }), mood: "" };
}

export function idleView(onTry: (query: string) => void, hasFolders: boolean): HTMLElement {
  if (!hasFolders) {
    return h(
      "div.empty",
      h("div.orb.glass", icon("folder", 30)),
      h("h2", t("search.empty.noFolders")),
      h("button.button.primary", { onclick: () => void api.openMain("settings") }, t("search.empty.choose")),
    );
  }
  const tries = (["example.1", "example.2", "example.3", "example.4", "example.5"] as const).map((key) =>
    h("button.pill", { onclick: () => onTry(t(key)) }, t(key)),
  );
  return h(
    "div.empty",
    h("div.orb.glass", { "data-light": "" }, icon("magpie", 30)),
    h("h2", t("search.empty.title")),
    h("p", t("search.empty.body")),
    h("div.tries", ...tries),
    h("div.drop-hint", icon("drop", 15), t("search.empty.drop")),
  );
}

export async function mountPanel(root: HTMLElement): Promise<void> {
  const input = h("input", { type: "text", placeholder: t("search.placeholder"), spellcheck: false, autocomplete: "off", "aria-label": t("search.placeholder") });
  const searchbox = h("div.searchbox", h("span.spark", icon("spark", 22)), input);
  const filters = h("div.filters");
  const results = h("div.results", { role: "listbox" });
  const previewPane = h("div.preview");
  const body = h("div.body.single", results, previewPane);
  const status = h("div.status", h("span.dot"), h("span"));
  const hint = (keys: string[], label: string) => h("span", ...keys.map((key) => h("kbd", key)), label);
  const footer = h(
    "div.footer",
    status,
    h(
      "div.hints",
      hint(["↵"], t("hint.open")),
      hint(["Ctrl", "↵"], t("hint.reveal")),
      hint(["Space"], t("hint.preview")),
      hint(["Tab"], t("hint.similar")),
      hint(["Ctrl", "O"], t("hint.window")),
    ),
  );
  root.replaceChildren(h("div.sheet", { "data-tauri-drag-region": "" }, h("div.panel", searchbox, filters, body, footer)));
  followPointer(root);

  let hasFolders = (await api.getConfig()).roots.length > 0;
  let files: number | null = null;
  let finder: Finder;
  const preview = new Preview(previewPane, { similar: (hit) => finder.similarTo(hit) });
  finder = new Finder({
    searchbox,
    input,
    filters,
    results,
    preview,
    make: row,
    columns: () => 1,
    limit: 24,
    idle: () => idleView((query) => finder.setQuery(query), hasFolders),
    onChange: (_, selected) => body.classList.toggle("single", !selected),
    onLeave: () => void api.hidePanel(),
  });

  window.addEventListener("keydown", (event) => {
    if (event.ctrlKey && (event.key === "o" || event.key === "O")) {
      event.preventDefault();
      void api.openMain("search", finder.query);
    } else if (event.key === "Escape" && document.activeElement !== input) {
      void api.hidePanel();
    } else if (document.activeElement !== input && !event.ctrlKey && !event.altKey && event.key.length === 1) {
      // Typing anywhere goes to the search box.
      input.focus();
    }
  });

  const showStatus = (state: IndexState) => {
    const line = statusLine(state, files);
    status.className = `status ${line.mood}`;
    status.lastElementChild!.textContent = line.text;
  };
  const refresh = async () => {
    try {
      const now = await api.status();
      files = now.counts.files;
      showStatus(now.index_state);
    } catch {
      /* the helper isn't up yet */
    }
  };
  api.on<IndexState>("index", showStatus);
  api.on("index-finished", () => {
    void refresh();
    if (finder.query || finder.example) void finder.run(false);
  });
  api.on<api.Config>("config", (settings) => (hasFolders = settings.roots.length > 0));
  api.on("panel-shown", () => {
    finder.focus();
    void refresh();
  });
  void refresh();
  finder.focus();
}
