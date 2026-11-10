// The window: the same search with room to look around, plus the map, the duplicates, the saved searches and
// the settings.

import { getCurrentWindow } from "@tauri-apps/api/window";
import * as api from "./api";
import { IndexState, Kind } from "./api";
import { mountDuplicates, mountMap } from "./extras";
import { Finder } from "./finder";
import { t } from "./i18n";
import { idleView, statusLine } from "./panel";
import { card, Lens, Preview, report } from "./results";
import { mountSettings } from "./settings";
import { mountSetup } from "./setup";
import { followPointer, h, icon, number } from "./ui";

type View = "search" | "map" | "duplicates" | "settings" | "setup";

interface Collection {
  id: number;
  name: string;
  query: string;
}

export async function mountWindow(root: HTMLElement): Promise<void> {
  const settings = await api.getConfig();
  let view: View = settings.setup_done ? "search" : "setup";
  let hasFolders = settings.roots.length > 0;
  let kind: Kind | "all" = "all";
  let files: number | null = null;
  let finder: Finder | null = null;
  let pendingQuery: string | null = null;
  const appWindow = getCurrentWindow();

  // The title bar: drag it to move the window, double-click to fill the screen.
  const input = h("input", { type: "text", placeholder: t("search.placeholder"), spellcheck: false, autocomplete: "off", "aria-label": t("search.placeholder") });
  const searchbox = h("div.searchbox.glass", { "data-light": "" }, h("span.spark", icon("spark", 18)), input);
  const controls = h(
    "div.window-controls",
    h("button", { title: "Minimize", onclick: () => void appWindow.minimize() }, icon("minimize", 15)),
    h("button", { title: "Maximize", onclick: () => void appWindow.toggleMaximize() }, icon("maximize", 13)),
    h("button.close", { title: t("action.close"), onclick: () => void appWindow.close() }, icon("close", 15)),
  );
  const titlebar = h(
    "div.titlebar",
    { "data-tauri-drag-region": "" },
    h("div.brand", { "data-tauri-drag-region": "" }, icon("spark", 18), "Glint"),
    h("div", { "data-tauri-drag-region": "", style: { display: "grid" } }, searchbox),
    controls,
  );

  const sidebar = h("nav.sidebar");
  const content = h("main.content.glass");
  const frame = h("div.window", titlebar, sidebar, content);
  const sheet = h("div.sheet", frame);
  root.replaceChildren(sheet);
  followPointer(root);
  const navLens = new Lens(sidebar);

  // The sidebar
  const navItems = new Map<View, HTMLElement>();
  const navButton = (target: View, iconName: string, label: string) => {
    const button = h("button.nav-item", { onclick: () => go(target) }, icon(iconName, 18), h("span", label));
    navItems.set(target, button);
    return button;
  };
  const collectionsBox = h("div", { style: { display: "contents" } });
  const meter = h("div.meter", h("i"));
  const statusText = h("span");
  const currentFile = h("div.current");
  const pauseButton = h("button.icon-button", { style: { width: "26px", height: "26px" } });
  pauseButton.addEventListener("click", async () => {
    const now = await api.getConfig();
    api.setConfig({ ...now, paused: !now.paused }).catch(report);
  });
  const indexCard = h("div.index-card.glass", h("div.line", statusText, pauseButton), meter, currentFile);
  sidebar.append(
    navButton("search", "search", t("nav.search")),
    navButton("map", "map", t("nav.map")),
    navButton("duplicates", "copies", t("nav.duplicates")),
    h("div.group", t("nav.collections")),
    collectionsBox,
    h("div.spacer"),
    indexCard,
    navButton("settings", "settings", t("nav.settings")),
  );

  const loadCollections = async () => {
    try {
      const { collections } = await api.ask<{ collections: Collection[] }>({ op: "collections" });
      if (!collections.length) {
        collectionsBox.replaceChildren(h("div.hint", t("collections.none")));
        return;
      }
      collectionsBox.replaceChildren(
        ...collections.map((collection) => {
          const remove = h("span.remove.icon-button", { title: t("collections.remove"), style: { width: "24px", height: "24px" } }, icon("close", 12));
          remove.addEventListener("click", (event) => {
            event.stopPropagation();
            api.ask({ op: "collection_remove", id: collection.id }).then(loadCollections, report);
          });
          return h(
            "button.nav-item",
            { title: collection.query, onclick: () => search(collection.query) },
            icon("bookmark", 17),
            h("span", collection.name),
            remove,
          );
        }),
      );
    } catch {
      /* the helper isn't up yet */
    }
  };

  const showIndex = (state: IndexState) => {
    const line = statusLine(state, files);
    statusText.textContent = line.text;
    const reading = state.running && (state.phase === "reading" || state.phase === "deepening") && state.total > 0;
    meter.classList.toggle("endless", state.running && !reading);
    (meter.firstElementChild as HTMLElement).style.width = reading ? `${(state.done / state.total) * 100}%` : state.running ? "" : "0";
    meter.style.visibility = state.running ? "visible" : "hidden";
    currentFile.textContent = state.running && state.current ? state.current : state.failed > 0 ? t("status.failed", { n: number(state.failed) }) : "";
    currentFile.style.display = currentFile.textContent ? "" : "none";
    pauseButton.replaceChildren(icon(state.paused ? "play" : "pause", 14));
    pauseButton.title = state.paused ? t("status.paused") : t("settings.pause");
  };
  const refresh = async () => {
    try {
      const now = await api.status();
      files = now.counts.files;
      showIndex(now.index_state);
    } catch {
      /* the helper isn't up yet */
    }
  };

  // The views
  const mountSearch = () => {
    const pills = h("div", { style: { display: "flex", gap: "6px", flexWrap: "wrap" } });
    const kinds: (Kind | "all")[] = ["all", "image", "screenshot", "pdf", "doc", "audio", "video"];
    for (const option of kinds) {
      const pill = h("button.pill", { "aria-pressed": String(option === kind) }, option === "all" ? null : icon(option, 14), t(`kind.${option}`));
      pill.addEventListener("click", () => {
        kind = option;
        for (const other of pills.children) other.setAttribute("aria-pressed", "false");
        pill.setAttribute("aria-pressed", "true");
        void finder?.run(false);
      });
      pills.append(pill);
    }
    const meta = h("span.meta");
    const save = h("button.button.quiet", { hidden: true }, icon("bookmark", 15), t("action.save"));
    save.addEventListener("click", () => {
      const query = finder?.query.trim();
      if (!query) return;
      api.ask({ op: "collection_add", name: query, q: query }).then(loadCollections, report);
    });
    const filters = h("div.filters", { style: { padding: "0 14px 8px" } });
    const grid = h("div.grid", { role: "listbox" });
    const previewPane = h("div.preview");
    const preview = new Preview(previewPane, { similar: (hit) => finder?.similarTo(hit) });
    const main = h("div.view", { style: { gridTemplateRows: "auto auto minmax(0, 1fr)" } }, h("div.toolbar", pills, h("span.grow"), meta, save), filters, grid);
    content.replaceChildren(main, previewPane);
    finder = new Finder({
      searchbox,
      input,
      filters,
      results: grid,
      preview,
      make: card,
      columns: () => Math.max(1, getComputedStyle(grid).gridTemplateColumns.split(" ").length),
      limit: 120,
      idle: () => idleView((query) => finder?.setQuery(query), hasFolders),
      suffix: () => (kind === "all" ? "" : `kind:${kind === "image" ? "photo" : kind}`),
      onChange: (result, selected) => {
        content.classList.toggle("with-preview", !!selected);
        if (result) meta.textContent = `${t("search.results", { n: number(result.hits.length) })} · ${t("search.ms", { ms: Math.round(result.ms) })}`;
        else if (!selected) meta.textContent = "";
        save.hidden = !finder?.query.trim();
      },
    });
    if (pendingQuery !== null) {
      finder.setQuery(pendingQuery);
      pendingQuery = null;
    } else if (input.value) {
      void finder.run(false);
    }
    finder.focus();
  };

  const go = (next: View) => {
    view = next;
    const setup = view === "setup";
    sidebar.style.display = setup ? "none" : "";
    titlebar.style.gridTemplateColumns = setup ? "228px minmax(0, 1fr) auto" : "";
    searchbox.style.visibility = setup ? "hidden" : "";
    content.style.gridColumn = setup ? "1 / -1" : "";
    content.style.margin = setup ? "0 10px 10px" : "";
    content.classList.remove("with-preview");
    for (const [name, item] of navItems) item.setAttribute("aria-current", String(name === view));
    navLens.moveTo(navItems.get(view) ?? null, false);
    if (view !== "search") finder = null;
    switch (view) {
      case "search":
        mountSearch();
        break;
      case "map":
        void mountMap(content, (point) => {
          pendingQuery = "";
          go("search");
          // What's like the file that was picked.
          void api.similar(point.file, "", 1).then(() => finder?.similarTo({ file: point.file, path: point.path } as api.Hit), report);
        });
        break;
      case "duplicates":
        void mountDuplicates(content);
        break;
      case "settings":
        void mountSettings(content);
        break;
      case "setup":
        void mountSetup(content, () => {
          hasFolders = true;
          go("search");
        });
        break;
    }
  };

  const search = (query: string) => {
    if (view === "search" && finder) {
      finder.setQuery(query);
      finder.focus();
    } else {
      pendingQuery = query;
      go("search");
    }
  };

  // Typing in the box from any view goes to the search.
  input.addEventListener("focus", () => {
    if (view !== "search" && view !== "setup") {
      pendingQuery = input.value;
      go("search");
    }
  });
  window.addEventListener("keydown", (event) => {
    if (view === "setup") return;
    if (event.ctrlKey && (event.key === "f" || event.key === "k" || event.key === "l")) {
      event.preventDefault();
      input.focus();
      input.select();
    } else if (event.ctrlKey && event.key === ",") {
      event.preventDefault();
      go("settings");
    }
  });
  titlebar.addEventListener("dblclick", (event) => {
    if ((event.target as HTMLElement).hasAttribute("data-tauri-drag-region")) void appWindow.toggleMaximize();
  });

  api.on<IndexState>("index", showIndex);
  api.on("index-finished", () => {
    void refresh();
    if (view === "search" && finder && (finder.query || finder.example)) void finder.run(false);
  });
  api.on<api.Config>("config", (next) => {
    hasFolders = next.roots.length > 0;
  });
  api.on<{ view: View | null; query: string | null }>("navigate", (target) => {
    if (target.query !== null && target.query !== undefined) search(target.query);
    else if (target.view) go(target.view);
  });

  go(view);
  void api.launchQuery().then((query) => query && view !== "setup" && search(query));
  void refresh();
  void loadCollections();
  requestAnimationFrame(() => navLens.moveTo(navItems.get(view) ?? null, false));
}
