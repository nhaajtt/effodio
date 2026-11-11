// The search itself, as both the panel and the window run it: what's typed (or the picture searched by), the
// results, the one that's selected, and the keys that move through them.

import { getCurrentWebview } from "@tauri-apps/api/webview";
import * as api from "./api";
import { Filters, Hit, SearchResult } from "./api";
import { t } from "./i18n";
import { Lens, lightbox, list, Preview, report } from "./results";
import { day, debounce, fileName, h, icon, toast } from "./ui";

export interface FinderParts {
  searchbox: HTMLElement;
  input: HTMLInputElement;
  filters: HTMLElement;
  results: HTMLElement;
  preview: Preview | null;
  make: (hit: Hit, index: number) => HTMLElement;
  /** How many results sit side by side (1 in a list). */
  columns: () => number;
  limit: number;
  /** What the results area shows before anything is typed. */
  idle: () => HTMLElement;
  /** Words added to every search (the window's kind filter). */
  suffix?: () => string;
  onChange?: (result: SearchResult | null, selected: Hit | null) => void;
  /** Escape with nothing left to clear. */
  onLeave?: () => void;
}

const MEDIA = /\.(jpe?g|png|webp|gif|bmp|heic|heif|tiff?|mp3|wav|m4a|flac|ogg|opus|mp4|mov|mkv|webm|avi|m4v)$/i;

export class Finder {
  hits: Hit[] = [];
  selected = -1;
  /** A picture, recording or video searched by, instead of words. */
  example: string | null = null;
  /** The example is a face to look for, not a picture to find look-alikes of. */
  faces = false;
  /** A result whose likes are shown. */
  likeness: Hit | null = null;
  private elements: HTMLElement[] = [];
  private lens: Lens;
  private sequence = 0;
  /** The arrows were used: Space now previews rather than types. */
  private moved = false;
  private chip: HTMLElement | null = null;

  constructor(private parts: FinderParts) {
    this.lens = new Lens(parts.results);
    const typed = debounce(() => void this.run(true), 70);
    parts.input.addEventListener("input", () => {
      this.moved = false;
      typed();
    });
    parts.input.addEventListener("keydown", (event) => this.key(event));
    parts.input.addEventListener("paste", (event) => this.paste(event));
    parts.results.addEventListener("click", (event) => {
      const target = (event.target as HTMLElement).closest<HTMLElement>("[data-index]");
      if (target) this.select(Number(target.dataset.index), false);
    });
    parts.results.addEventListener("dblclick", (event) => {
      if ((event.target as HTMLElement).closest("[data-index]")) this.open();
    });
    new ResizeObserver(() => this.lens.moveTo(this.elements[this.selected] ?? null, false)).observe(parts.results);
    void getCurrentWebview().onDragDropEvent((event) => {
      const payload = event.payload;
      document.body.classList.toggle("dropping", payload.type === "enter" || payload.type === "over");
      if (payload.type === "drop") {
        const path = payload.paths.find((candidate) => MEDIA.test(candidate));
        if (path) this.searchBy(path);
        else if (payload.paths.length) toast(t("search.empty.drop"));
      }
    });
    this.showIdle();
  }

  get query(): string {
    return this.parts.input.value;
  }

  get current(): Hit | null {
    return this.hits[this.selected] ?? null;
  }

  setQuery(text: string): void {
    this.parts.input.value = text;
    this.clearSpecial(false);
    void this.run(false);
  }

  focus(): void {
    this.parts.input.focus();
    this.parts.input.select();
  }

  private fullQuery(): string {
    return `${this.query} ${this.parts.suffix?.() ?? ""}`.trim();
  }

  private showIdle(): void {
    this.hits = [];
    this.elements = [];
    this.selected = -1;
    this.parts.results.replaceChildren(this.parts.idle());
    this.lens.moveTo(null);
    this.parts.filters.replaceChildren();
    this.parts.preview?.clear();
    this.parts.onChange?.(null, null);
  }

  /** Runs the search for what's in the box now. `typing`: the last word may be unfinished. */
  async run(typing: boolean): Promise<void> {
    const sequence = ++this.sequence;
    const words = this.fullQuery();
    if (!words && !this.example && !this.likeness) {
      this.showIdle();
      return;
    }
    this.parts.searchbox.classList.add("busy");
    try {
      const result = this.example
        ? this.faces
          ? await api.byFace(this.example, this.parts.limit)
          : await api.byExample(this.example, words, this.parts.limit)
        : this.likeness
          ? await api.similar(this.likeness.file, words, this.parts.limit)
          : await api.search(words, { typing, limit: this.parts.limit });
      if (sequence !== this.sequence) return;
      this.show(result);
    } catch (error) {
      if (sequence === this.sequence) report(error);
    } finally {
      if (sequence === this.sequence) this.parts.searchbox.classList.remove("busy");
    }
  }

  private show(result: SearchResult): void {
    const keep = this.current?.file;
    this.hits = result.hits;
    this.renderFilters(result.filters);
    if (!result.hits.length) {
      this.elements = [];
      this.selected = -1;
      this.parts.results.replaceChildren(
        h("div.empty", h("h2", t("search.nothing")), h("p", t("search.nothingHint"))),
      );
      this.lens.moveTo(null);
      this.parts.preview?.clear();
      this.parts.onChange?.(result, null);
      return;
    }
    this.elements = list(this.parts.results, result.hits, this.parts.make);
    this.parts.results.scrollTop = 0;
    // While typing, the selection stays on the same file when it's still there.
    const again = keep === undefined ? -1 : result.hits.findIndex((hit) => hit.file === keep);
    this.selected = -1;
    this.select(again >= 0 && this.moved ? again : 0, false, result);
  }

  private renderFilters(filters: Filters): void {
    const chips: HTMLElement[] = [];
    const add = (text: string) => chips.push(h("span.pill.accent", text));
    for (const kind of filters.kinds) add(t(`kind.${kind}`));
    for (const folder of filters.folders) add(t("filter.in", { folder }));
    for (const ext of filters.extensions) add(t("filter.ext", { ext }));
    if (filters.after !== null) add(t("filter.after", { date: day(filters.after) }));
    if (filters.before !== null) add(t("filter.before", { date: day(filters.before) }));
    for (const word of filters.negative) add(t("filter.not", { word }));
    for (const phrase of filters.phrases) add(t("filter.phrase", { phrase }));
    this.parts.filters.replaceChildren(...chips);
  }

  select(index: number, scroll = true, result: SearchResult | null = null): void {
    if (!this.hits.length) return;
    const next = Math.max(0, Math.min(this.hits.length - 1, index));
    const changed = next !== this.selected;
    this.elements[this.selected]?.removeAttribute("aria-selected");
    this.selected = next;
    const element = this.elements[next];
    element?.setAttribute("aria-selected", "true");
    this.lens.moveTo(element ?? null, scroll);
    if (changed || result) {
      this.parts.preview?.show(this.hits[next], this.fullQuery());
      this.parts.onChange?.(result, this.hits[next]);
    }
  }

  open(): void {
    const hit = this.current;
    if (!hit) return;
    const page = hit.segment_kind === "page" ? hit.loc : null;
    const second = hit.segment_kind === "sound" || hit.segment_kind === "frame" ? hit.loc : null;
    api.openResult(hit.path, page, second).catch(report);
  }

  /** Searches by a picture, a recording or a video. */
  searchBy(path: string, faces = false): void {
    this.faces = faces;
    this.likeness = null;
    this.example = path;
    this.setChip(fileName(path), path);
    void this.run(false);
  }

  /** Shows what's like a result. */
  similarTo(hit: Hit): void {
    this.example = null;
    this.likeness = hit;
    this.parts.input.value = "";
    this.setChip(fileName(hit.path), hit.path);
    void this.run(false);
  }

  private setChip(name: string, path: string): void {
    this.chip?.remove();
    const picture = h("img", { alt: "" });
    api.thumbnail(path, 96).then((url) => (picture.src = url), () => picture.remove());
    this.chip = h(
      "div.example-chip.glass",
      { title: `${t("search.byExample")} ${name}` },
      picture,
      h("span", { style: { maxWidth: "130px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, name),
      h("button.icon-button", { title: t("search.clearExample"), style: { width: "24px", height: "24px" }, onclick: () => this.clearSpecial(true) }, icon("close", 13)),
    );
    this.parts.input.before(this.chip);
    this.parts.input.focus();
  }

  private clearSpecial(rerun: boolean): boolean {
    if (!this.example && !this.likeness) return false;
    this.example = null;
    this.likeness = null;
    this.chip?.remove();
    this.chip = null;
    if (rerun) void this.run(false);
    return true;
  }

  private paste(event: ClipboardEvent): void {
    const file = Array.from(event.clipboardData?.files ?? []).find((candidate) => candidate.type.startsWith("image/"));
    if (!file) return;
    event.preventDefault();
    const extension = file.type.split("/")[1]?.replace("jpeg", "jpg") ?? "png";
    file
      .arrayBuffer()
      .then((buffer) => api.saveExample(new Uint8Array(buffer), extension))
      .then((path) => this.searchBy(path), report);
  }

  /** The keys that work in the search box. Returns whether the key was used. */
  key(event: KeyboardEvent): boolean {
    const columns = Math.max(1, this.parts.columns());
    const used = () => {
      event.preventDefault();
      return true;
    };
    const hit = this.current;
    switch (event.key) {
      case "ArrowDown":
        this.moved = true;
        this.select(this.selected + columns);
        return used();
      case "ArrowUp":
        this.moved = true;
        this.select(this.selected - columns);
        return used();
      case "ArrowRight":
      case "ArrowLeft": {
        // In a grid, once you're moving among results; in a list the arrows stay with the text.
        if (columns === 1 || !this.moved) return false;
        this.select(this.selected + (event.key === "ArrowRight" ? 1 : -1));
        return used();
      }
      case "PageDown":
        this.moved = true;
        this.select(this.selected + columns * 5);
        return used();
      case "PageUp":
        this.moved = true;
        this.select(this.selected - columns * 5);
        return used();
      case "Enter":
        if (!hit) return false;
        if (event.ctrlKey) api.reveal(hit.path).catch(report);
        else this.open();
        return used();
      case " ":
        if (!hit || !this.moved) return false;
        lightbox(hit, this.fullQuery());
        return used();
      case "Tab":
        if (!hit || event.shiftKey) return false;
        this.similarTo(hit);
        return used();
      case "c":
      case "C": {
        const input = this.parts.input;
        if (!event.ctrlKey || !hit || input.selectionStart !== input.selectionEnd) return false;
        api.copyFiles([hit.path]).then(() => toast(t("action.copied")), report);
        return used();
      }
      case "Backspace":
        if (this.query === "" && this.clearSpecial(true)) return used();
        return false;
      case "Escape":
        if (this.clearSpecial(true)) return used();
        if (this.query) {
          this.setQuery("");
          return used();
        }
        this.parts.onLeave?.();
        return used();
      default:
        return false;
    }
  }
}
