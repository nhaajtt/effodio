// Settings: the folders, what's left out, how files are read, the app itself, and the local search door.

import * as api from "./api";
import { Config, Status } from "./api";
import { setLanguage, t } from "./i18n";
import { report } from "./results";
import { applyAppearance, bytes, h, icon, number, toast } from "./ui";

export function toggle(checked: boolean, onChange: (value: boolean) => void, label: string): HTMLButtonElement {
  const element = h("button.switch", { role: "switch", "aria-checked": String(checked), "aria-label": label });
  element.addEventListener("click", () => {
    const next = element.getAttribute("aria-checked") !== "true";
    element.setAttribute("aria-checked", String(next));
    onChange(next);
  });
  return element;
}

export function segmented<T extends string>(options: [T, string][], value: T, onChange: (value: T) => void): HTMLElement {
  const group = h("div.segmented", { role: "group" });
  for (const [key, label] of options) {
    const button = h("button", { "aria-pressed": String(key === value) }, label);
    button.addEventListener("click", () => {
      for (const other of group.children) other.setAttribute("aria-pressed", "false");
      button.setAttribute("aria-pressed", "true");
      onChange(key);
    });
    group.append(button);
  }
  return group;
}

function field(label: string, control: HTMLElement, note?: string): HTMLElement {
  return h("div.field", h("div.label", h("span", label), note ? h("small", note) : null), control);
}

/** "Ctrl+Shift+Space" from a key press, or null while only modifiers are down. */
function shortcutOf(event: KeyboardEvent): string | null {
  if (["Control", "Shift", "Alt", "Meta"].includes(event.key)) return null;
  const parts: string[] = [];
  if (event.ctrlKey) parts.push("Ctrl");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (event.metaKey) parts.push("Super");
  if (!parts.length) return null;
  const key =
    event.code === "Space" ? "Space"
    : event.code.startsWith("Key") ? event.code.slice(3)
    : event.code.startsWith("Digit") ? event.code.slice(5)
    : /^F\d{1,2}$/.test(event.key) ? event.key
    : null;
  return key ? [...parts, key].join("+") : null;
}

export async function mountSettings(container: HTMLElement): Promise<void> {
  let settings: Config = await api.getConfig();
  let current: Status | null = null;
  const page = h("div.settings");
  container.replaceChildren(h("div.view", h("div.heading", h("h2", t("settings.title"))), page));

  const save = async (change: (draft: Config) => void, redraw = false) => {
    const draft: Config = structuredClone(settings);
    change(draft);
    try {
      settings = await api.setConfig(draft);
    } catch (error) {
      report(error);
      settings = await api.getConfig();
      redraw = true;
    }
    applyAppearance(settings.theme, settings.accent);
    if (redraw) draw();
  };

  const draw = () => {
    // Folders
    const folders = h("div.folder-list");
    if (!settings.roots.length) folders.append(h("div.note", t("settings.folders.none")));
    for (const root of settings.roots) {
      folders.append(
        h(
          "div.folder",
          icon("folder", 17),
          h("span.path", { title: root.path }, root.path),
          h(
            "button.button.quiet",
            { onclick: () => void save((draft) => (draft.roots = draft.roots.filter((other) => other.path !== root.path)), true) },
            t("settings.folders.remove"),
          ),
        ),
      );
    }
    const addFolders = h(
      "button.button",
      {
        onclick: async () => {
          const picked = await api.pickFolders();
          if (!picked.length) return;
          await save((draft) => {
            for (const path of picked) {
              if (!draft.roots.some((root) => root.path.toLowerCase() === path.toLowerCase())) draft.roots.push({ path, exclude: [] });
            }
          }, true);
        },
      },
      icon("plus", 16),
      t("settings.folders.add"),
    );
    const skipFolders = h("textarea", { placeholder: t("settings.skip.folders"), spellcheck: false, value: settings.rules.skip_folders.join("\n") });
    skipFolders.addEventListener("change", () =>
      void save((draft) => (draft.rules.skip_folders = skipFolders.value.split("\n").map((line) => line.trim()).filter(Boolean))),
    );
    const skipTypes = h("input.input", { placeholder: t("settings.skip.extensions"), spellcheck: false, value: settings.rules.skip_extensions.join(", "), style: { width: "260px" } });
    skipTypes.addEventListener("change", () =>
      void save((draft) => (draft.rules.skip_extensions = skipTypes.value.split(/[\s,;]+/).map((ext) => ext.replace(/^\./, "").trim().toLowerCase()).filter(Boolean))),
    );

    // Reading
    const speed = current?.speed ?? null;
    const speedNote = h(
      "small",
      speed
        ? t("settings.speed.result", {
            image: speed.image.toFixed(2),
            passage: speed.passage.toFixed(2),
            sound: speed.sound.toFixed(2),
            device: speed.device === "gpu" ? (speed.gpu_name ?? t("settings.device.gpu")) : t("settings.device.cpu"),
          })
        : t("settings.speed.none"),
    );
    const measure = h("button.button", t("settings.speed.run"));
    measure.addEventListener("click", () => {
      measure.disabled = true;
      measure.textContent = t("settings.speed.running");
      api.speedCheck().catch(report);
    });
    const languages = h("small");
    languages.textContent = t("settings.ocr.body", { languages: "…" });

    // The door
    const door = settings.door;
    const address = `http://127.0.0.1:${door.port}/search?q=…`;
    const doorDetails = door.enabled
      ? h(
          "div",
          { style: { display: "grid", gap: "10px" } },
          h("div.field", h("div.label", h("span", t("settings.door.address")), h("span.mono", address))),
          h(
            "div.field",
            h("div.label", h("span", t("settings.door.token")), h("span.mono", `${door.token.slice(0, 10)}…${door.token.slice(-6)}`)),
            h("button.button", { onclick: () => void navigator.clipboard.writeText(door.token).then(() => toast(t("action.copied"))) }, t("settings.door.copy")),
          ),
          h("div.mono", { style: { color: "var(--ink-3)" } }, `curl -H "Authorization: Bearer <token>" "http://127.0.0.1:${door.port}/search?q=dog+on+the+beach"`),
        )
      : null;

    // The index
    const counts = current?.counts;
    const indexLine = counts
      ? t("settings.index.body", { files: number(counts.files), vectors: number(counts.vectors), size: bytes(current!.index_bytes) })
      : "…";

    page.replaceChildren(
      h(
        "section.glass",
        { "data-light": "" },
        h("h3", t("settings.folders")),
        h("p.note", t("settings.folders.body")),
        folders,
        h("div", addFolders),
        h("h3", { style: { marginTop: "6px" } }, t("settings.skip")),
        skipFolders,
        skipTypes,
      ),
      h(
        "section.glass",
        { "data-light": "" },
        h("h3", t("settings.reading")),
        current && !current.ffmpeg ? h("div.warning", t("settings.ffmpeg.missing")) : null,
        h("div.field", h("div.label", h("span", t("settings.ocr")), languages), toggle(settings.ocr, (value) => void save((draft) => (draft.ocr = value)), t("settings.ocr"))),
        field(t("settings.pause"), toggle(settings.paused, (value) => void save((draft) => (draft.paused = value)), t("settings.pause"))),
        field(t("settings.power"), toggle(settings.only_on_power, (value) => void save((draft) => (draft.only_on_power = value)), t("settings.power"))),
        h(
          "div.field",
          h("div.label", h("span", t("settings.device")), speedNote),
          h(
            "div",
            { style: { display: "flex", gap: "8px", alignItems: "center", flexWrap: "wrap", justifyContent: "flex-end" } },
            segmented(
              [["auto", t("settings.device.auto")], ["cpu", t("settings.device.cpu")], ["gpu", t("settings.device.gpu")]],
              settings.device,
              (value) => void save((draft) => (draft.device = value)),
            ),
            measure,
          ),
        ),
      ),
      h(
        "section.glass",
        { "data-light": "" },
        h("h3", t("settings.index")),
        h("p.note", indexLine),
        counts && counts.failed > 0
          ? h("div.field", h("div.label", h("span", t("settings.index.failed", { n: number(counts.failed) }))), h("button.button", { onclick: () => void api.indexRetry().catch(report) }, t("settings.index.retry")))
          : null,
        h("div", h("button.button", { onclick: () => void api.indexNow() }, t("settings.index.now"))),
      ),
      h(
        "section.glass",
        { "data-light": "" },
        h("h3", t("settings.app")),
        h("div.field", h("div.label", h("span", t("settings.hotkey"))), hotkeyButton()),
        field(t("settings.autostart"), toggle(settings.autostart, (value) => void save((draft) => (draft.autostart = value)), t("settings.autostart"))),
        field(
          t("settings.theme"),
          segmented(
            [["system", t("settings.theme.system")], ["light", t("settings.theme.light")], ["dark", t("settings.theme.dark")]],
            settings.theme,
            (value) => void save((draft) => (draft.theme = value)),
          ),
        ),
        field(t("settings.accent"), accentPicker()),
        field(
          t("settings.language"),
          segmented(
            [["auto", t("settings.language.auto")], ["en", "English"], ["vi", "Tiếng Việt"]],
            settings.language,
            (value) =>
              void save((draft) => (draft.language = value)).then(() => {
                setLanguage(settings.language);
                location.reload();
              }),
          ),
        ),
      ),
      h(
        "section.glass",
        { "data-light": "" },
        h("h3", { style: { display: "flex", gap: "8px", alignItems: "center" } }, icon("shield", 17), t("settings.door")),
        h("p.note", t("settings.door.body")),
        field(t("settings.door.enable"), toggle(door.enabled, (value) => void save((draft) => (draft.door.enabled = value), true), t("settings.door.enable"))),
        doorDetails,
      ),
      h("p.note", { style: { margin: "0 4px" } }, t("settings.about", { version: current?.version ?? "" })),
    );

    api.ocrLanguages().then(
      (found) => (languages.textContent = t("settings.ocr.body", { languages: found.join(", ") || "—" })),
      () => (languages.textContent = t("settings.ocr.body", { languages: "—" })),
    );
  };

  const hotkeyButton = () => {
    const button = h("button.button", h("kbd", settings.hotkey));
    button.addEventListener("click", () => {
      button.replaceChildren(t("settings.hotkey.record"));
      const onKey = (event: KeyboardEvent) => {
        event.preventDefault();
        event.stopPropagation();
        if (event.key === "Escape") return done(settings.hotkey);
        const shortcut = shortcutOf(event);
        if (shortcut) done(shortcut);
      };
      const done = (shortcut: string) => {
        window.removeEventListener("keydown", onKey, true);
        void save((draft) => (draft.hotkey = shortcut), true);
      };
      window.addEventListener("keydown", onKey, true);
    });
    return button;
  };

  const accentPicker = () => {
    const colours: [Config["accent"], string][] = [["gold", "#d9a63a"], ["navy", "#3f6bd8"], ["violet", "#8a6bf0"]];
    const group = h("div.swatches");
    for (const [name, colour] of colours) {
      group.append(
        h("button", {
          title: name,
          "aria-label": name,
          "aria-pressed": String(settings.accent === name),
          style: { background: colour },
          onclick: () => void save((draft) => (draft.accent = name), true),
        }),
      );
    }
    return group;
  };

  draw();
  const refresh = async () => {
    try {
      current = await api.status();
      if (container.contains(page)) draw();
    } catch {
      /* the helper isn't there yet */
    }
  };
  api.on<{ event: string }>("speed", (event) => {
    if (event.event === "finished" || event.event === "error") void refresh();
  });
  api.on("index-finished", () => void refresh());
  void refresh();
}
