// Ask your files: a question answered from what the files say, by a language model on this computer, with its
// sources. Also where the two downloads-on-request are switched on: that model, and the one that listens to recordings.

import * as api from "./api";
import { Hit } from "./api";
import { locale, t } from "./i18n";
import { memberCard } from "./more";
import { report } from "./results";
import { escapeHtml, h, icon } from "./ui";

interface Kit {
  ready: boolean;
  getting: boolean;
  done: number;
  total: number;
  error: string | null;
}

const megabytes = (bytes: number) => Math.round(bytes / 2 ** 20);

/** A card for something that needs a download first. `ready` draws what's there once it has arrived. */
function kitCard(name: "speech" | "answers", title: string, body: string, ready: () => HTMLElement | null): HTMLElement {
  const slot = h("div.kit-slot");
  const card = h("div.group-card.glass.kit", h("header", title), h("p", body), slot);
  let timer = 0;
  const draw = async () => {
    const kit = (await api.ask<Record<string, Kit>>({ op: "kits" }))[name];
    if (!card.isConnected && timer) {
      clearInterval(timer);
      return;
    }
    if (kit.ready) {
      clearInterval(timer);
      timer = 0;
      slot.replaceChildren(ready() ?? "");
    } else if (kit.getting) {
      const share = kit.total ? (kit.done / kit.total) * 100 : 0;
      slot.replaceChildren(h("div.meter", h("i", { style: { width: `${share}%` } })), h("span.small", t("kit.getting", { done: megabytes(kit.done), total: megabytes(kit.total) })));
      if (!timer) timer = window.setInterval(() => void draw().catch(() => {}), 1500);
    } else {
      clearInterval(timer);
      timer = 0;
      const get = h("button.button.primary", icon("drop", 16), t("kit.get"));
      get.addEventListener("click", () => api.ask({ op: "kit_get", kit: name }).then(draw, report));
      slot.replaceChildren(get, kit.error ? h("span.small", t("kit.failed", { error: kit.error })) : "");
    }
  };
  draw().catch(report);
  return card;
}

/** The answer's text, with each [n] a button that points at its source. */
function answerText(text: string, pick: (index: number) => void): HTMLElement {
  const block = h("div.answer");
  block.innerHTML = escapeHtml(text)
    .replace(/\*\*(.+?)\*\*/g, "<b>$1</b>")
    .replace(/\[(\d+)\]/g, '<button class="cite" data-source="$1">$1</button>')
    .replace(/\n/g, "<br>");
  block.addEventListener("click", (event) => {
    const source = (event.target as HTMLElement).closest<HTMLElement>(".cite")?.dataset.source;
    if (source) pick(Number(source) - 1);
  });
  return block;
}

export async function mountAsk(container: HTMLElement): Promise<void> {
  const body = h("div.groups");
  container.replaceChildren(h("div.view", h("div.heading", h("h2", t("ask.title")), h("p", t("ask.body"))), body));

  const asking = () => {
    const input = h("input", { type: "text", placeholder: t("ask.placeholder"), spellcheck: false });
    const go = h("button.button.primary", icon("spark", 16), t("ask.go"));
    const out = h("div.ask-out");
    const run = async () => {
      const question = input.value.trim();
      if (!question || go.disabled) return;
      go.disabled = true;
      out.replaceChildren(h("div.thinking", h("i"), h("i"), h("i"), h("span", t("ask.thinking"))));
      try {
        const result = await api.ask<{ answer: string | null; sources: Hit[]; seconds: number }>({ op: "ask_files", q: question, language: locale });
        if (!result.answer) {
          out.replaceChildren(h("p.small", t("ask.nothing")));
          return;
        }
        const cards = result.sources.map((hit, index) => {
          const card = memberCard({ file: hit.file, path: hit.path, kind: hit.kind, size: hit.size, modified: hit.modified }, undefined, String(index + 1));
          card.addEventListener("click", (event) => {
            // A source opens at its page or its second, where the answer came from.
            event.stopImmediatePropagation();
            const page = hit.segment_kind === "page" ? hit.loc : null;
            const second = hit.segment_kind === "sound" || hit.segment_kind === "frame" ? hit.loc : null;
            void api.openResult(hit.path, page, second).catch(report);
          }, true);
          return card;
        });
        const flash = (index: number) => {
          const card = cards[index];
          if (!card) return;
          card.scrollIntoView({ block: "nearest", behavior: "smooth" });
          card.classList.remove("flash");
          void card.offsetWidth;
          card.classList.add("flash");
        };
        out.replaceChildren(
          answerText(result.answer, flash),
          h("div.sources-head", t("ask.sources"), h("span.pill", t("ask.took", { s: Math.round(result.seconds) }))),
          h("div.members", ...cards),
        );
      } catch (error) {
        out.replaceChildren();
        report(error);
      } finally {
        go.disabled = false;
      }
    };
    go.addEventListener("click", () => void run());
    input.addEventListener("keydown", (event) => {
      event.stopPropagation();
      if (event.key === "Enter") void run();
    });
    setTimeout(() => input.focus(), 50);
    return h("div.ask", h("div.ask-box", input, go), out);
  };

  const listening = () => {
    const toggle = h("button.button");
    const show = async () => {
      const on = !!(await api.getConfig()).speech;
      toggle.replaceChildren(icon(on ? "check" : "audio", 16), on ? t("kit.turnOff") : t("kit.turnOn"));
      toggle.classList.toggle("primary", !on);
    };
    toggle.addEventListener("click", async () => {
      const settings = await api.getConfig();
      await api.setConfig({ ...settings, speech: !settings.speech });
      if (!settings.speech) await api.indexNow();
      await show();
    });
    void show();
    return toggle;
  };

  body.replaceChildren(
    kitCard("answers", t("ask.title"), t("kit.answers"), asking),
    kitCard("speech", t("kit.speech.title"), t("kit.speech"), listening),
  );
}
