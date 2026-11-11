// Small pieces the views share: building elements, icons, formatting, and the light that follows the pointer.

import { Hit, Kind } from "./api";
import { locale, t } from "./i18n";

type Child = Node | string | null | undefined | false;
type Props = Record<string, unknown>;

/** `h("div.row.active", { title: "…", onclick }, child, …)` */
export function h<K extends keyof HTMLElementTagNameMap>(
  spec: K | `${K}.${string}`,
  props?: Props | Child,
  ...children: Child[]
): HTMLElementTagNameMap[K] {
  const [tag, ...classes] = (spec as string).split(".");
  const element = document.createElement(tag) as HTMLElementTagNameMap[K];
  if (classes.length) element.className = classes.join(" ");
  let rest = children;
  if (props instanceof Node || typeof props === "string" || props === null || props === undefined || props === false) {
    rest = [props as Child, ...children];
  } else {
    for (const [name, value] of Object.entries(props)) {
      if (value === undefined || value === null || value === false) continue;
      if (name.startsWith("on") && typeof value === "function") {
        element.addEventListener(name.slice(2), value as EventListener);
      } else if (name === "html") {
        element.innerHTML = value as string;
      } else if (name === "style" && typeof value === "object") {
        Object.assign(element.style, value);
      } else if (name in element && name !== "list") {
        (element as unknown as Props)[name] = value;
      } else {
        element.setAttribute(name, value === true ? "" : String(value));
      }
    }
  }
  for (const child of rest) {
    if (child === null || child === undefined || child === false) continue;
    element.append(child);
  }
  return element;
}

const paths: Record<string, string> = {
  spade: '<path d="M14.500 3.500 20.500 9.500"/><path d="M17.500 6.500 9.800 14.200"/><path d="M6.200 10.600l7.200 7.200-2.300 2.300c-1.900 1.900-5 2.200-7.200.6l-.6-.600c-1.600-2.200-1.300-5.300.6-7.200z"/>',
  ask: '<path d="M5 5.500h14a1.500 1.500 0 0 1 1.500 1.500v8a1.500 1.500 0 0 1-1.500 1.500h-6l-4.500 3.500v-3.500H5A1.500 1.500 0 0 1 3.500 15V7A1.500 1.500 0 0 1 5 5.500Z"/><path d="M9.500 9.500a2.500 2.500 0 1 1 3.500 2.300c-.6.3-1 .7-1 1.400M12 15.300v.2"/>',
  tidy: '<path d="M5 19 16 8M14.5 6.5l3 3M18 3.5v3M16.5 5h3M6 5.5v2M5 6.5h2"/>',
  album: '<rect x="4" y="8" width="16" height="11.5" rx="2.5"/><path d="M7 5.5h10M9 3h6"/>',
  people: '<circle cx="9" cy="9" r="3.2"/><path d="M3.5 19c.6-3.2 2.8-5 5.5-5s4.900 1.800 5.500 5"/><circle cx="17" cy="8" r="2.4"/><path d="M16.5 13.200c2.200.2 3.700 1.700 4.200 4.300"/>',
  thread: '<circle cx="5.5" cy="6" r="2"/><circle cx="18.5" cy="18" r="2"/><path d="M7.5 6h6a3 3 0 0 1 0 6h-3a3 3 0 0 0 0 6h6"/>',
  pencil: '<path d="M4.5 19.500l1-4L16 5l3 3L8.500 18.500Z"/><path d="m14 7 3 3"/>',
  effodio: '<path d="M14.6 6.400a3.100 3.100 0 0 1 5.200 1.300l2 .8-2 1c-.3 3.500-2.900 6.200-6.600 6.800L3.500 20.500l5.400-7.300c.9-2.900 2.800-5.400 5.700-6.800Z"/><path d="M9.600 13.400c2.300.7 4.500.2 6.200-1.500"/><circle cx="17.200" cy="8.400" r=".5"/>',
  spark: '<path d="M12 2.5c.6 4.9 2.6 6.9 7.5 7.5-4.9.6-6.9 2.600-7.5 7.500-.6-4.900-2.600-6.900-7.500-7.500 4.900-.6 6.900-2.600 7.500-7.500Z"/><path d="M19 15.500c.3 2 1 2.700 3 3-2 .3-2.700 1-3 3-.3-2-1-2.700-3-3 2-.3 2.700-1 3-3Z"/>',
  search: '<circle cx="11" cy="11" r="6.500"/><path d="m16 16 4.500 4.500"/>',
  image: '<rect x="3.500" y="4.500" width="17" height="15" rx="3"/><circle cx="9" cy="10" r="1.600"/><path d="m4 17 5-4.500 3.500 3 3-2.500 4.500 4"/>',
  screenshot: '<rect x="3.500" y="5" width="17" height="12" rx="2.500"/><path d="M9 20.500h6M12 17v3.500"/>',
  pdf: '<path d="M7 3.500h7l4.500 4.500v11a1.500 1.500 0 0 1-1.500 1.500H7A1.500 1.500 0 0 1 5.500 19V5A1.500 1.500 0 0 1 7 3.500Z"/><path d="M14 3.500V8h4.500M8.500 13h7M8.500 16.500h5"/>',
  doc: '<path d="M7 3.500h10A1.500 1.500 0 0 1 18.500 5v14a1.500 1.500 0 0 1-1.500 1.500H7A1.500 1.500 0 0 1 5.500 19V5A1.500 1.500 0 0 1 7 3.500Z"/><path d="M8.500 8.500h7M8.500 12h7M8.500 15.500h4"/>',
  audio: '<path d="M5 10v4M8.500 7v10M12 4.500v15M15.500 8v8M19 10.500v3"/>',
  video: '<rect x="3.500" y="5.500" width="12.500" height="13" rx="3"/><path d="m16 10.500 4.500-2.500v8L16 13.500"/>',
  map: '<circle cx="6.500" cy="7" r="2"/><circle cx="16.500" cy="5.500" r="1.500"/><circle cx="12" cy="13" r="2.500"/><circle cx="18" cy="17" r="2"/><circle cx="6" cy="18" r="1.500"/>',
  copies: '<rect x="8" y="8" width="12" height="12" rx="2.500"/><path d="M16 8V6.500A2.500 2.500 0 0 0 13.500 4h-7A2.500 2.500 0 0 0 4 6.500v7A2.500 2.500 0 0 0 6.500 16H8"/>',
  settings: '<circle cx="12" cy="12" r="3"/><path d="M12 3.500v2.200M12 18.300v2.200M3.500 12h2.200M18.300 12h2.200M6 6l1.600 1.600M16.400 16.400 18 18M18 6l-1.600 1.600M7.600 16.400 6 18"/>',
  folder: '<path d="M3.500 7A1.500 1.500 0 0 1 5 5.500h4l2 2.500h8A1.500 1.500 0 0 1 20.500 9.500V17a1.500 1.500 0 0 1-1.500 1.500H5A1.500 1.500 0 0 1 3.500 17Z"/>',
  open: '<path d="M14 4.500h5.500V10M19.500 4.500 11 13M17.500 13.500V18a1.500 1.500 0 0 1-1.500 1.500H6A1.500 1.500 0 0 1 4.500 18V8A1.500 1.500 0 0 1 6 6.500h4.500"/>',
  similar: '<circle cx="8.500" cy="12" r="5"/><circle cx="15.500" cy="12" r="5"/>',
  bookmark: '<path d="M7 4.500h10a1 1 0 0 1 1 1V20l-6-4-6 4V5.500a1 1 0 0 1 1-1Z"/>',
  close: '<path d="m6 6 12 12M18 6 6 18"/>',
  minimize: '<path d="M6 12h12"/>',
  maximize: '<rect x="6" y="6" width="12" height="12" rx="2"/>',
  check: '<path d="m5 12.500 4.500 4.500L19 7.500"/>',
  pause: '<path d="M8.500 5.500v13M15.500 5.500v13"/>',
  play: '<path d="M8 5.500v13l10.500-6.500Z"/>',
  plus: '<path d="M12 5v14M5 12h14"/>',
  trash: '<path d="M5 7h14M10 7V5h4v2M7 7l.8 12h8.400L17 7"/>',
  chevron: '<path d="m9 6 6 6-6 6"/>',
  drop: '<path d="M12 4v10M8 10.500l4 4 4-4M5 19.500h14"/>',
  bolt: '<path d="M13 3.500 6 13.500h5l-1 7 7-10h-5Z"/>',
  shield: '<path d="M12 3.500 5 6v5.500c0 4.300 2.900 7.300 7 9 4.100-1.700 7-4.700 7-9V6Z"/>',
};

export function icon(name: string, size = 18): SVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("width", String(size));
  svg.setAttribute("height", String(size));
  svg.setAttribute("fill", name === "spark" || name === "play" ? "currentColor" : "none");
  svg.setAttribute("stroke", name === "spark" || name === "play" ? "none" : "currentColor");
  svg.setAttribute("stroke-width", "1.600");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  svg.setAttribute("aria-hidden", "true");
  svg.classList.add("icon");
  svg.innerHTML = paths[name] ?? "";
  return svg;
}

export const kindIcon = (kind: Kind) => icon(kind);

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]!);
}

/** An excerpt with its matches (wrapped in « » by the engine) marked. */
export function marked(excerpt: string): string {
  return escapeHtml(excerpt).replace(/«/g, "<mark>").replace(/»/g, "</mark>");
}

/** `text` with every one of the query's words marked, for a preview of a whole passage. */
export function markWords(text: string, query: string): string {
  const words = Array.from(new Set(fold(query).split(/[^\p{L}\p{N}]+/u).filter((word) => word.length >= 2)));
  if (!words.length) return escapeHtml(text);
  const folded = fold(text);
  if (folded.length !== text.length) return escapeHtml(text);
  const spans: [number, number][] = [];
  const pattern = /[\p{L}\p{N}]+/gu;
  for (let match = pattern.exec(folded); match; match = pattern.exec(folded)) {
    if (words.some((word) => match![0] === word || (word.length >= 3 && match![0].startsWith(word)))) {
      spans.push([match.index, match.index + match[0].length]);
    }
  }
  let out = "";
  let cursor = 0;
  for (const [start, end] of spans) {
    out += escapeHtml(text.slice(cursor, start)) + "<mark>" + escapeHtml(text.slice(start, end)) + "</mark>";
    cursor = end;
  }
  return out + escapeHtml(text.slice(cursor));
}

/** Lowercase, without accents: as the engine matches words. */
export function fold(text: string): string {
  return text.normalize("NFD").replace(/\p{M}/gu, "").replace(/[đĐ]/g, "d").toLowerCase().normalize("NFC");
}

export function fileName(path: string): string {
  return path.slice(Math.max(path.lastIndexOf("\\"), path.lastIndexOf("/")) + 1);
}

/** The folder a file is in, shortened to its last two parts. */
export function folderOf(path: string): string {
  const parts = path.split(/[\\/]/).slice(0, -1);
  return parts.length <= 2 ? parts.join(" › ") : "… › " + parts.slice(-2).join(" › ");
}

export function clock(seconds: number): string {
  const whole = Math.max(0, Math.floor(seconds));
  const hours = Math.floor(whole / 3600);
  const minutes = Math.floor((whole % 3600) / 60);
  const rest = String(whole % 60).padStart(2, "0");
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, "0")}:${rest}` : `${minutes}:${rest}`;
}

export function bytes(size: number): string {
  if (size < 1024) return `${size} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = size / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value >= 100 ? value.toFixed(0) : value.toFixed(1)} ${units[unit]}`;
}

export function day(seconds: number): string {
  return new Date(seconds * 1000).toLocaleDateString(locale === "vi" ? "vi-VN" : undefined, {
    year: "numeric",
    month: "short",
    day: "numeric",
  });
}

export function duration(seconds: number): string {
  if (seconds < 90) return t("time.seconds", { n: Math.max(1, Math.round(seconds)) });
  if (seconds < 3600) return t("time.minutes", { n: Math.round(seconds / 60) });
  const hours = Math.floor(seconds / 3600);
  const minutes = Math.round((seconds % 3600) / 60);
  return minutes ? t("time.hm", { h: hours, m: minutes }) : t("time.hours", { n: hours });
}

export const number = (value: number) => value.toLocaleString(locale === "vi" ? "vi-VN" : undefined);

/** Where in the file a hit points: "Page 3", "at 1:24". */
export function place(hit: Hit): string | null {
  if (hit.loc === null) return null;
  if (hit.segment_kind === "page") {
    const total = Number(hit.info.pages);
    return total > 1 ? t("place.pageOf", { n: hit.loc, total }) : null;
  }
  if (hit.segment_kind === "sound" || hit.segment_kind === "frame") return t("place.at", { time: clock(hit.loc) });
  if (hit.segment_kind === "chunk" && Number(hit.info.passages) > 1) return t("place.passage", { n: hit.loc });
  return null;
}

/** Why a result is there, in a few words. */
export function reason(hit: Hit): string {
  if (hit.segment_kind === "ocr") return t("why.text");
  const meaning = hit.z !== null && hit.z >= 2;
  if (meaning && hit.words >= 0.5) return t("why.both");
  return meaning || hit.words < 0.5 ? t("why.meaning") : t("why.words");
}

/** The light on glass follows the pointer: `--mx` and `--my` on every `.glass` under it. */
export function followPointer(root: HTMLElement): void {
  let frame = 0;
  root.addEventListener("pointermove", (event) => {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      for (const element of root.querySelectorAll<HTMLElement>("[data-light]")) {
        const box = element.getBoundingClientRect();
        element.style.setProperty("--mx", `${event.clientX - box.left}px`);
        element.style.setProperty("--my", `${event.clientY - box.top}px`);
      }
    });
  });
}

/** Runs `run` after `wait` ms without another call. */
export function debounce<A extends unknown[]>(run: (...args: A) => void, wait: number): (...args: A) => void {
  let timer = 0;
  return (...args: A) => {
    clearTimeout(timer);
    timer = window.setTimeout(() => run(...args), wait);
  };
}

export function toast(message: string): void {
  document.querySelector(".toast")?.remove();
  const element = h("div.toast.glass", { role: "status" }, message);
  document.body.append(element);
  setTimeout(() => element.classList.add("leaving"), 1600);
  setTimeout(() => element.remove(), 2000);
}

export function applyAppearance(theme: string, accent: string): void {
  const dark = theme === "dark" || (theme === "system" && matchMedia("(prefers-color-scheme: dark)").matches);
  document.documentElement.dataset.theme = dark ? "dark" : "light";
  document.documentElement.dataset.accent = accent;
}
