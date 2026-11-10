// How a result looks: as a row, as a card, in the preview pane and shown large. Shared by the panel and the window.

import * as api from "./api";
import { Hit, Moment } from "./api";
import { t } from "./i18n";
import { bytes, clock, day, fileName, folderOf, h, icon, kindIcon, marked, markWords, place, reason, toast } from "./ui";

/** The lens: one piece of glass that slides to whatever is selected. */
export class Lens {
  readonly element = h("div.lens");
  constructor(private container: HTMLElement) {
    container.prepend(this.element);
  }
  /** Call after the container's children were replaced. */
  attach(): void {
    if (this.element.parentElement !== this.container) this.container.prepend(this.element);
  }
  moveTo(target: HTMLElement | null, scroll = true): void {
    this.attach();
    if (!target) {
      this.element.classList.remove("on");
      return;
    }
    const first = !this.element.classList.contains("on");
    if (first) this.element.style.transition = "none";
    this.element.style.width = `${target.offsetWidth}px`;
    this.element.style.height = `${target.offsetHeight}px`;
    this.element.style.transform = `translate(${target.offsetLeft}px, ${target.offsetTop}px)`;
    this.element.classList.add("on");
    if (first) {
      void this.element.offsetWidth;
      this.element.style.transition = "";
    }
    if (scroll) target.scrollIntoView({ block: "nearest" });
  }
}

function thumb(hit: Hit, side: number): HTMLElement {
  const box = h("div.thumb", kindIcon(hit.kind));
  // Explorer has real thumbnails for pictures, videos and PDFs; for the rest its icon says more than a blank page.
  const pictured = hit.kind === "image" || hit.kind === "screenshot" || hit.kind === "video" || hit.kind === "pdf";
  if (!pictured) box.classList.add("contain");
  const show = (url: string) => {
    // An image that isn't in the page yet never loads lazily, so it loads first and goes in when it has.
    const image = h("img", { alt: "", decoding: "async" });
    image.onload = () => {
      // The place badge stays on top of the picture.
      (box.querySelector("svg, img") ?? box.appendChild(document.createElement("i"))).replaceWith(image);
      requestAnimationFrame(() => image.classList.add("ready"));
    };
    image.src = url;
  };
  // A video hit shows the frame that matched, not the video's first one.
  if (hit.kind === "video" && hit.segment_kind === "frame" && hit.loc !== null && side > 100) {
    api.frameImage(hit.path, hit.loc, 480).then((frame) => show(api.fileUrl(frame.path)), () => api.thumbnail(hit.path, side).then(show, () => {}));
  } else {
    api.thumbnail(hit.path, side).then(show, () => {});
  }
  return box;
}

function subline(hit: Hit): HTMLElement {
  const sub = h("div.sub");
  if (hit.excerpt) {
    sub.innerHTML = marked(hit.excerpt);
  } else {
    sub.append(h("span.where", folderOf(hit.path)));
  }
  return sub;
}

export function row(hit: Hit, index: number): HTMLElement {
  const where = place(hit);
  const element = h(
    `div.row${hit.strong ? "" : ".weak"}`,
    { role: "option", id: `hit-${hit.file}`, style: { animationDelay: `${Math.min(index, 12) * 14}ms` } },
    thumb(hit, 96),
    h("div", h("div.name", fileName(hit.path)), subline(hit)),
    h("div.side", where ? h("span.place", where) : null, kindIcon(hit.kind)),
  );
  element.dataset.index = String(index);
  return element;
}

export function card(hit: Hit, index: number): HTMLElement {
  const where = place(hit);
  const picture = thumb(hit, 320);
  if (where) picture.append(h("span.badge", where));
  const element = h(
    `div.card${hit.strong ? "" : ".weak"}`,
    { role: "option", id: `hit-${hit.file}`, style: { animationDelay: `${Math.min(index, 16) * 12}ms` } },
    picture,
    h("div", h("div.name", fileName(hit.path)), subline(hit)),
  );
  element.dataset.index = String(index);
  return element;
}

/** Hits as rows or cards, the weaker ones under their own heading. Returns the elements, in hit order. */
export function list(container: HTMLElement, hits: Hit[], make: (hit: Hit, index: number) => HTMLElement): HTMLElement[] {
  const elements: HTMLElement[] = [];
  const nodes: Node[] = [];
  const strong = hits.filter((hit) => hit.strong).length;
  hits.forEach((hit, index) => {
    if (index === strong && strong > 0) nodes.push(h("div.section-label", t("search.weaker")));
    const element = make(hit, index);
    elements.push(element);
    nodes.push(element);
  });
  container.replaceChildren(...nodes);
  return elements;
}

function facts(hit: Hit): HTMLElement {
  const out = h("div.facts");
  const add = (text: string | null | undefined) => text && out.append(h("span.pill", text));
  add(t(`one.${hit.kind}`));
  add(reason(hit));
  if (hit.info.width && hit.info.height) add(`${hit.info.width} × ${hit.info.height}`);
  if (hit.info.duration) add(clock(Number(hit.info.duration)));
  add(bytes(hit.size));
  add(day(hit.modified));
  if (hit.info.taken) add(String(hit.info.taken).slice(0, 10).replace(/:/g, "-"));
  if (hit.info.camera) add(String(hit.info.camera));
  return out;
}

/** The strip that shows where in a recording the query matches; a click seeks. */
function momentStrip(hit: Hit, found: Moment[], media: HTMLMediaElement | null): HTMLElement {
  const total = Math.max(Number(hit.info.duration) || 0, ...found.map((m) => m.loc_end ?? m.loc + 3), 1);
  const strip = h("div.moments");
  const top = Math.max(2.5, ...found.map((m) => m.z));
  for (const moment of found) {
    const strength = Math.max(0, Math.min(1, (moment.z - 0.5) / (top - 0.5)));
    if (strength <= 0.04) continue;
    const sound = moment.kind === "sound";
    const start = moment.loc / total;
    const width = sound ? ((moment.loc_end ?? moment.loc + 30) - moment.loc) / total : 3 / total;
    strip.append(
      h(`div.bar${sound ? ".sound" : ""}`, {
        title: clock(moment.loc),
        style: { left: `${start * 100}%`, width: `max(3px, ${width * 100}%)`, height: `${12 + strength * 88}%` },
        onclick: (event: Event) => {
          event.stopPropagation();
          if (media) {
            media.currentTime = moment.loc;
            void media.play();
          }
        },
      }),
    );
  }
  const head = h("div.head");
  strip.append(head);
  const place = () => (head.style.left = `${((media?.currentTime ?? hit.loc ?? 0) / total) * 100}%`);
  place();
  media?.addEventListener("timeupdate", place);
  strip.addEventListener("click", (event) => {
    if (!media) return;
    const box = strip.getBoundingClientRect();
    media.currentTime = ((event.clientX - box.left) / box.width) * total;
  });
  return strip;
}

export interface PreviewActions {
  similar(hit: Hit): void;
  opened?(): void;
}

export class Preview {
  private shown = 0;
  constructor(private container: HTMLElement, private actions: PreviewActions) {}

  clear(): void {
    this.shown += 1;
    this.container.replaceChildren();
  }

  show(hit: Hit, query: string): void {
    const token = ++this.shown;
    const current = () => token === this.shown;
    const stage = h("div.stage.loading");
    const extra = h("div", { style: { display: "contents" } });
    let media: HTMLMediaElement | null = null;

    const still = (url: string, page = false) => {
      const image = h("img", { alt: "" });
      image.onload = () => {
        if (!current()) return;
        stage.classList.remove("loading");
        if (page) stage.classList.add("page");
        stage.replaceChildren(image);
      };
      image.onerror = () => stage.classList.remove("loading");
      image.src = url;
    };

    if (hit.kind === "image" || hit.kind === "screenshot") {
      still(api.fileUrl(hit.path));
    } else if (hit.kind === "pdf") {
      api.pageImage(hit.path, hit.loc ?? 1, 1100).then(
        (page) => current() && still(api.fileUrl(page.path), true),
        () => api.thumbnail(hit.path, 512).then((url) => current() && still(url)),
      );
    } else if (hit.kind === "video") {
      const video = h("video", { controls: true, preload: "metadata", playsInline: true });
      video.src = `${api.fileUrl(hit.path)}#t=${hit.loc ?? 0}`;
      video.onloadeddata = () => stage.classList.remove("loading");
      video.onerror = () => {
        stage.classList.remove("loading");
        api.thumbnail(hit.path, 512).then((url) => current() && still(url));
      };
      stage.replaceChildren(video);
      media = video;
    } else if (hit.kind === "audio") {
      stage.classList.remove("loading");
      stage.replaceChildren(icon("audio", 56));
      const audio = h("audio", { controls: true, preload: "metadata" });
      audio.src = `${api.fileUrl(hit.path)}#t=${hit.loc ?? 0}`;
      extra.append(audio);
      media = audio;
    } else {
      api.thumbnail(hit.path, 256).then((url) => current() && still(url), () => stage.classList.remove("loading"));
      stage.style.minHeight = "96px";
      stage.style.maxHeight = "120px";
    }

    if ((hit.kind === "video" || hit.kind === "audio") && query.trim()) {
      const holder = h("div", { style: { display: "contents" } });
      extra.append(holder);
      api.moments(hit.file, query).then(({ moments }) => {
        if (!current() || moments.length < 2) return;
        const total = Math.max(Number(hit.info.duration) || 0, 1);
        holder.append(momentStrip(hit, moments, media), h("div.moments-caption", h("span", "0:00"), h("span", clock(total))));
      }, () => {});
    }

    const quote = h("p.quote");
    if (hit.excerpt) {
      quote.innerHTML = marked(hit.excerpt);
    } else {
      quote.hidden = true;
    }
    // The whole passage, page or text read in a picture, once it arrives.
    if (hit.segment_kind === "chunk" || hit.segment_kind === "page" || hit.segment_kind === "ocr") {
      api.segmentText(hit.segment).then(({ text }) => {
        if (!current() || !text) return;
        quote.hidden = false;
        quote.innerHTML = markWords(text.length > 2400 ? text.slice(0, 2400) + "…" : text, query);
        quote.querySelector("mark")?.scrollIntoView({ block: "center" });
      }, () => {});
    }

    const page = hit.segment_kind === "page" ? hit.loc : null;
    const second = hit.segment_kind === "sound" || hit.segment_kind === "frame" ? hit.loc : null;
    const actions = h(
      "div.actions",
      h("button.button.primary", { onclick: () => void api.openResult(hit.path, page, second).then(() => this.actions.opened?.(), report) }, icon("open", 16), t("action.open")),
      h("button.button", { onclick: () => void api.reveal(hit.path).catch(report) }, icon("folder", 16), t("action.reveal")),
      h("button.button", { onclick: () => this.actions.similar(hit) }, icon("similar", 16), t("action.similar")),
      h("button.button.quiet", { onclick: () => void api.copyFiles([hit.path]).then(() => toast(t("action.copied")), report) }, icon("copies", 16), t("action.copy")),
    );

    this.container.replaceChildren(
      stage,
      extra,
      h("div", h("h3", fileName(hit.path)), h("div.path", hit.path)),
      facts(hit),
      quote,
      actions,
    );
    this.container.scrollTop = 0;
  }
}

export function report(error: unknown): void {
  toast(typeof error === "string" ? error : error instanceof Error ? error.message : t("error.generic"));
}

/** A result shown large over everything, at its page or moment. Any key or a click closes it. */
export function lightbox(hit: Hit, query: string): void {
  document.querySelector(".lightbox")?.remove();
  const overlay = h("div.lightbox", { tabIndex: -1 });
  const close = () => {
    overlay.remove();
    window.removeEventListener("keydown", onKey, true);
  };
  const onKey = (event: KeyboardEvent) => {
    if (event.key === "Escape" || event.key === " ") {
      event.preventDefault();
      event.stopPropagation();
      close();
    }
  };
  overlay.addEventListener("click", (event) => event.target === overlay && close());
  window.addEventListener("keydown", onKey, true);
  if (hit.kind === "image" || hit.kind === "screenshot") {
    overlay.append(h("img", { src: api.fileUrl(hit.path), alt: "" }));
  } else if (hit.kind === "video" || hit.kind === "audio") {
    const tag = hit.kind === "video" ? "video" : "audio";
    const media = h(tag, { controls: true, autoplay: true });
    media.src = `${api.fileUrl(hit.path)}#t=${hit.loc ?? 0}`;
    overlay.append(media);
  } else if (hit.kind === "pdf") {
    const image = h("img", { alt: "", style: { background: "#fff" } });
    overlay.append(image);
    api.pageImage(hit.path, hit.loc ?? 1, 2000).then((page) => (image.src = api.fileUrl(page.path)), report);
  } else {
    const reader = h("div.reader.glass", hit.excerpt ?? "");
    overlay.append(reader);
    api.segmentText(hit.segment).then(({ text }) => {
      if (text) reader.innerHTML = markWords(text, query);
      reader.querySelector("mark")?.scrollIntoView({ block: "center" });
    }, () => {});
  }
  document.body.append(overlay);
  overlay.focus();
}

