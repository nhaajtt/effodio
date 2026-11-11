// The first launch: what Effodio is, the model download, which folders to read (with how long that takes on this
// computer), and off it goes.

import * as api from "./api";
import { Config, Estimate } from "./api";
import { languageButton } from "./fx";
import { t } from "./i18n";
import { report } from "./results";
import { bytes, debounce, duration, followPointer, h, icon, number } from "./ui";

export async function mountSetup(container: HTMLElement, onDone: () => void): Promise<void> {
  let settings: Config = await api.getConfig();
  const suggestions = await api.suggestedFolders();
  const chosen = new Set<string>(settings.roots.map((root) => root.path));
  if (!chosen.size) {
    for (const folder of suggestions) {
      if (!folder.cloud && ["Pictures", "Documents", "Downloads", "Desktop"].includes(folder.name)) chosen.add(folder.path);
    }
  }
  const extra: string[] = settings.roots.map((root) => root.path).filter((path) => !suggestions.some((s) => s.path === path));
  let step = 0;
  let modelReady = (await api.status().catch(() => null))?.model_ready ?? false;
  let modelError: string | null = null;
  let progress: { done: number; total: number } | null = null;
  let checking = false;
  let measured = false;

  const stage = h("div.setup");
  const language = languageButton();
  language.classList.add("floating");
  container.replaceChildren(stage, language);
  followPointer(stage);

  const dots = () => h("div.dots", ...[0, 1, 2, 3].map((index) => h(`i${index === step ? ".on" : ""}`)));
  const go = (next: number) => {
    step = next;
    draw();
  };

  const withRoots = (): Config => ({
    ...settings,
    roots: [...chosen].map((path) => settings.roots.find((root) => root.path === path) ?? { path, exclude: [] }),
  });

  let estimateLine: HTMLElement | null = null;
  let estimateRun = 0;
  const estimate = debounce(async () => {
    if (!estimateLine) return;
    const run = ++estimateRun;
    if (!chosen.size) {
      estimateLine.textContent = "";
      return;
    }
    estimateLine.textContent = t("setup.estimate.counting");
    try {
      const found: Estimate = await api.estimate(withRoots());
      if (run !== estimateRun || !estimateLine) return;
      let text = t("setup.estimate", { files: number(found.files), time: duration(found.seconds) });
      if (found.cloud_only > 0) text += ` · ${t("setup.estimate.cloud", { n: number(found.cloud_only) })}`;
      estimateLine.textContent = text;
    } catch (error) {
      if (run === estimateRun) report(error);
    }
  }, 250);

  const startDownload = () => {
    modelError = null;
    api.modelDownload().catch((error) => {
      modelError = String(error);
      draw();
    });
  };

  api.on<{ event: string; done?: number; total?: number; error?: string }>("model", (event) => {
    if (event.event === "progress" && event.done !== undefined && event.total !== undefined) {
      progress = { done: event.done, total: event.total };
      checking = false;
    } else if (event.event === "checking") {
      checking = true;
    } else if (event.event === "finished") {
      modelReady = true;
      // The speed check needs the model; it runs while the folders are being chosen.
      if (!measured) {
        measured = true;
        api.speedCheck().catch(() => {});
      }
    } else if (event.event === "error") {
      modelError = event.error ?? t("error.generic");
    }
    if (step === 1) draw();
  });
  api.on<{ event: string }>("speed", (event) => {
    if (event.event === "finished") estimate();
  });

  const draw = () => {
    estimateLine = null;
    if (step === 0) {
      stage.replaceChildren(
        h(
          "div.step.glass",
          { "data-light": "" },
          h("div.orb.glass", icon("effodio", 34)),
          h("h1", t("setup.welcome")),
          h("p", t("setup.welcome.body")),
          h("div.buttons", dots(), h("button.button.primary.large", { onclick: () => go(1) }, t("setup.start"))),
        ),
      );
      return;
    }
    if (step === 1) {
      if (!modelReady && !progress && !modelError) startDownload();
      if (modelReady && !measured) {
        measured = true;
        api.speedCheck().catch(() => {});
      }
      const share = modelReady ? 1 : progress ? progress.done / Math.max(progress.total, 1) : 0;
      const line = modelReady
        ? t("setup.model.ready")
        : modelError
          ? modelError
          : checking
            ? t("setup.model.checking")
            : progress
              ? t("setup.model.downloading", { done: bytes(progress.done), total: bytes(progress.total) })
              : t("setup.model.downloading", { done: "0 MB", total: "865 MB" });
      const meter = h(`div.meter${!modelReady && !progress && !modelError ? ".endless" : ""}`, h("i", { style: { width: `${share * 100}%` } }));
      stage.replaceChildren(
        h(
          "div.step.glass",
          { "data-light": "" },
          h("div.orb.glass", icon(modelReady ? "check" : "bolt", 32)),
          h("h1", t("setup.model")),
          h("p", t("setup.model.body")),
          meter,
          h("div.estimate", { style: modelError ? { color: "var(--danger)" } : {} }, line),
          h(
            "div.buttons",
            dots(),
            h(
              "div",
              { style: { display: "flex", gap: "8px" } },
              modelError ? h("button.button", { onclick: () => { startDownload(); draw(); } }, t("setup.model.retry")) : null,
              // The folders can be chosen while the model downloads.
              h("button.button.primary.large", { onclick: () => go(2) }, t("setup.next")),
            ),
          ),
        ),
      );
      return;
    }
    if (step === 2) {
      const choices = h("div.choices");
      const add = (name: string, path: string) => {
        const button = h(
          "button.choice",
          { role: "checkbox", "aria-checked": String(chosen.has(path)) },
          h("span.check", icon("check", 14)),
          h("span.text", name, h("small", path)),
        );
        button.addEventListener("click", () => {
          if (chosen.has(path)) chosen.delete(path);
          else chosen.add(path);
          button.setAttribute("aria-checked", String(chosen.has(path)));
          finish.disabled = chosen.size === 0;
          estimate();
        });
        choices.append(button);
      };
      for (const folder of suggestions) add(folder.cloud ? `${folder.name} · OneDrive` : folder.name, folder.path);
      for (const path of extra) add(path.split(/[\\/]/).pop() || path, path);
      estimateLine = h("div.estimate");
      const finish = h("button.button.primary.large", { disabled: chosen.size === 0 }, t("setup.finish"));
      finish.addEventListener("click", async () => {
        finish.disabled = true;
        try {
          settings = await api.setConfig({ ...withRoots(), setup_done: true });
          go(3);
        } catch (error) {
          report(error);
          finish.disabled = false;
        }
      });
      stage.replaceChildren(
        h(
          "div.step.glass",
          { "data-light": "" },
          h("h1", t("setup.folders")),
          h("p", t("setup.folders.body")),
          choices,
          h(
            "div",
            h(
              "button.button",
              {
                onclick: async () => {
                  for (const path of await api.pickFolders()) {
                    if (!extra.includes(path) && !suggestions.some((s) => s.path === path)) extra.push(path);
                    chosen.add(path);
                  }
                  draw();
                },
              },
              icon("plus", 16),
              t("setup.folders.other"),
            ),
          ),
          estimateLine,
          h("div.buttons", dots(), h("div", { style: { display: "flex", gap: "8px" } }, h("button.button.quiet", { onclick: () => go(1) }, t("setup.back")), finish)),
        ),
      );
      estimate();
      return;
    }
    stage.replaceChildren(
      h(
        "div.step.glass",
        { "data-light": "" },
        h("div.orb.glass", icon("check", 34)),
        h("h1", t("setup.done")),
        h("p", t("setup.done.body", { hotkey: settings.hotkey })),
        h("div.buttons", dots(), h("button.button.primary.large", { onclick: onDone }, t("setup.done.open"))),
      ),
    );
  };

  draw();
}
