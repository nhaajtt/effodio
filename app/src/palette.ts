// The command palette (Ctrl+K): every place in the app and a few actions, found by typing. What matches nothing is
// a search.

import { t } from "./i18n";
import { fold, h, icon } from "./ui";

export interface Command {
  label: string;
  icon: string;
  hint?: string;
  run(): void;
}

let open: HTMLElement | null = null;

export function togglePalette(commands: () => Command[], search: (words: string) => void): void {
  if (open) {
    open.remove();
    open = null;
    return;
  }
  const all = commands();
  const input = h("input", { type: "text", placeholder: t("palette.placeholder"), spellcheck: false, autocomplete: "off" });
  const list = h("div.gcmd-list");
  let shown: Command[] = [];
  let at = 0;
  const close = () => {
    overlay.remove();
    open = null;
  };
  const draw = () => {
    const words = fold(input.value.trim());
    shown = all.filter((command) => !words || fold(command.label).includes(words));
    // Typed words that aren't a command's name: look for them in the files.
    if (input.value.trim()) {
      const typed = input.value.trim();
      shown.push({ label: `${t("palette.search")} “${typed}”`, icon: "search", hint: "↵", run: () => search(typed) });
    }
    at = Math.min(at, Math.max(0, shown.length - 1));
    list.replaceChildren(
      ...shown.map((command, index) => {
        const item = h("button.gcmd-item", { "data-selected": String(index === at) }, h("span.gcmd-ico", icon(command.icon, 17)), h("span.gcmd-label", command.label), h("span.gcmd-hint", command.hint ?? ""));
        item.addEventListener("pointermove", () => {
          if (at === index) return;
          at = index;
          for (const [other, element] of [...list.children].entries()) element.setAttribute("data-selected", String(other === at));
        });
        item.addEventListener("click", () => {
          close();
          command.run();
        });
        return item;
      }),
    );
  };
  input.addEventListener("input", () => {
    at = 0;
    draw();
  });
  input.addEventListener("keydown", (event) => {
    event.stopPropagation();
    if (event.key === "Escape" || (event.ctrlKey && event.key === "k")) {
      event.preventDefault();
      close();
    } else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      at = (at + (event.key === "ArrowDown" ? 1 : shown.length - 1)) % Math.max(1, shown.length);
      draw();
      list.children[at]?.scrollIntoView({ block: "nearest" });
    } else if (event.key === "Enter" && shown[at]) {
      event.preventDefault();
      const command = shown[at];
      close();
      command.run();
    }
  });
  const overlay = h("div.gcmd-overlay", h("div.gcmd-content", h("div.gcmd-search", h("span.gcmd-search-ico", icon("search", 17)), input, h("kbd", "esc")), list));
  overlay.addEventListener("pointerdown", (event) => {
    if (event.target === overlay) close();
  });
  document.body.append(overlay);
  open = overlay;
  draw();
  input.focus();
}
