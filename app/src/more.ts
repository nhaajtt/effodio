// More ways to use the index: the tidy-up, the albums that make themselves, the thread from one file through the
// others, the months a search falls in, the folder a file belongs in, and a pad to draw what you're looking for.

import * as api from "./api";
import { Hit, Kind, SearchResult } from "./api";
import { locale, t } from "./i18n";
import { report } from "./results";
import { bytes, fileName, folderOf, h, icon, kindIcon } from "./ui";

export interface Member {
  file: number;
  path: string;
  kind: Kind;
  size: number;
  modified: number;
  nearness?: number;
}

/** A file as a small card: its picture, its name and a line under it. Clicking shows it in its folder. */
export function memberCard(member: Member, sub?: string, badge?: string): HTMLElement {
  const thumb = h("div.thumb", kindIcon(member.kind));
  api.thumbnail(member.path, 256).then(
    (url) => thumb.replaceChildren(h("img.ready", { alt: "", src: url, style: { width: "100%", height: "100%", objectFit: "cover" } }), badge ? h("span.keep", badge) : ""),
    () => {},
  );
  return h(
    "button.member",
    { title: member.path, onclick: () => void api.reveal(member.path).catch(report) },
    thumb,
    h("div.name", fileName(member.path)),
    h("div.sub", sub ?? `${bytes(member.size)} · ${folderOf(member.path)}`),
  );
}

/** A card of files under a heading; past `first` of them, the rest wait behind a "+n" button. */
function groupCard(title: string, pills: string[], cards: HTMLElement[], first = 12): HTMLElement {
  const members = h("div.members", ...cards.slice(0, first));
  if (cards.length > first) {
    const more = h("button.member.more", `+${cards.length - first}`);
    more.addEventListener("click", () => more.replaceWith(...cards.slice(first)));
    members.append(more);
  }
  return h("div.group-card.glass", h("header", title, ...pills.map((pill) => h("span.pill", pill))), members);
}

function waiting(container: HTMLElement, title: string, body: string): HTMLElement {
  const groups = h("div.groups", h("div.empty", h("p", "…")));
  container.replaceChildren(h("div.view", h("div.heading", h("h2", title), h("p", body)), groups));
  return groups;
}

const nothing = (text: string) => h("div.empty", h("div.orb.glass", icon("check", 30)), h("p", text));
const total = (files: Member[]) => bytes(files.reduce((sum, file) => sum + file.size, 0));

export async function mountTidy(container: HTMLElement): Promise<void> {
  const groups = waiting(container, t("tidy.title"), t("tidy.body"));
  let found: { heaps: { name: string; files: Member[] }[]; bursts: { keep: number; files: Member[] }[] } = { heaps: [], bursts: [] };
  try {
    found = await api.ask({ op: "tidy" });
  } catch (error) {
    report(error);
  }
  if (!found.heaps.length && !found.bursts.length) {
    groups.replaceChildren(nothing(t("tidy.none")));
    return;
  }
  groups.replaceChildren(
    ...found.heaps.map((heap) =>
      groupCard(t(`tidy.${heap.name}` as "tidy.blurry"), [t("tidy.files", { n: heap.files.length }), total(heap.files)], heap.files.map((file) => memberCard(file))),
    ),
    ...found.bursts.map((burst) =>
      groupCard(
        t("tidy.burst"),
        [t("tidy.files", { n: burst.files.length }), total(burst.files)],
        burst.files.map((file) => memberCard(file, undefined, file.file === burst.keep ? t("tidy.keep") : undefined)),
      ),
    ),
  );
}

/** The folder's own name, from the trail `folderOf` writes. */
const lastFolder = (trail: string) => trail.split(/[›\/]/).pop()!.trim() || trail;

const monthYear = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(locale, { month: "short", year: "numeric" });

export async function mountAlbums(container: HTMLElement): Promise<void> {
  const groups = waiting(container, t("albums.title"), t("albums.body"));
  let albums: { name: { en: string; vi: string } | null; files: Member[] }[] = [];
  try {
    albums = (await api.ask<{ albums: typeof albums }>({ op: "albums" })).albums;
  } catch (error) {
    report(error);
  }
  if (!albums.length) {
    groups.replaceChildren(nothing(t("albums.none")));
    return;
  }
  groups.replaceChildren(
    ...albums.map((album) => {
      const from = monthYear(album.files[0].modified);
      const to = monthYear(album.files[album.files.length - 1].modified);
      // Most of an album usually sits in one folder: that says more than the subject alone.
      const folders = new Map<string, number>();
      for (const file of album.files) folders.set(folderOf(file.path), (folders.get(folderOf(file.path)) ?? 0) + 1);
      const [folder, inFolder] = [...folders].sort((a, b) => b[1] - a[1])[0];
      const subject = album.name ? (locale === "vi" ? album.name.vi : album.name.en) : t("albums.unnamed");
      const title = inFolder * 2 > album.files.length ? `${subject} · ${lastFolder(folder)}` : subject;
      return groupCard(title, [from === to ? from : `${from} – ${to}`, t("tidy.files", { n: album.files.length })], album.files.map((file) => memberCard(file)), 8);
    }),
  );
}

/** The thread from a file: the files about the same thing, joined by a line, each with how near it is. */
export async function mountThread(container: HTMLElement, hit: Hit, back: () => void): Promise<void> {
  const groups = waiting(container, t("thread.title", { name: fileName(hit.path) }), t("thread.body"));
  groups.before(h("button.button", { style: { justifySelf: "start", margin: "0 0 10px" }, onclick: back }, icon("chevron", 14), t("thread.back")));
  let thread: Member[] = [];
  try {
    thread = (await api.ask<{ thread: Member[] }>({ op: "thread", file: hit.file })).thread;
  } catch (error) {
    report(error);
  }
  if (thread.length < 2) {
    groups.replaceChildren(nothing(t("thread.none")));
    return;
  }
  const beads = thread.map((member, index) => {
    const card = memberCard(member, index === 0 ? folderOf(member.path) : `${Math.round((member.nearness ?? 0) * 100)}% · ${folderOf(member.path)}`);
    card.style.animationDelay = `${index * 70}ms`;
    return card;
  });
  groups.replaceChildren(h("div.thread.glass", ...beads));
}

/** The months (or years) a search's results fall in, as bars. Clicking one narrows the search to it. */
export function timeline(result: SearchResult | null, pick: (after: string, before: string) => void): HTMLElement | null {
  const hits = result?.hits.filter((hit) => hit.strong && hit.modified > 0) ?? [];
  if (hits.length < 6) return null;
  const month = (seconds: number) => {
    const date = new Date(seconds * 1000);
    return date.getFullYear() * 12 + date.getMonth();
  };
  const all = hits.map((hit) => month(hit.modified));
  const [first, last] = [Math.min(...all), Math.max(...all)];
  if (first === last) return null;
  // Past three years of months there are too many bars to read: by year then.
  const step = last - first >= 36 ? 12 : 1;
  const start = Math.floor(first / step) * step;
  const counts = new Map<number, number>();
  for (const one of all) counts.set(Math.floor(one / step) * step, (counts.get(Math.floor(one / step) * step) ?? 0) + 1);
  const most = Math.max(...counts.values());
  const two = (value: number) => String(value).padStart(2, "0");
  const bars: HTMLElement[] = [];
  for (let at = start; at <= last; at += step) {
    const count = counts.get(at) ?? 0;
    const year = Math.floor(at / 12);
    const label = step === 12 ? String(year) : new Date(year, at % 12, 1).toLocaleDateString(locale, { month: "short", year: "2-digit" });
    const bar = h("button.bar", { title: `${label} · ${count}`, disabled: count === 0 }, h("i", { style: { height: `${count ? 18 + (count / most) * 82 : 4}%` } }), h("span", label));
    bar.addEventListener("click", () => pick(`${year}-${two((at % 12) + 1)}`, `${year}-${two(step === 12 ? 12 : (at % 12) + 1)}`));
    bars.push(bar);
  }
  return h("div.timeline", ...bars);
}

/** For a search by example: the folder most of the nearest files live in. */
export function belongs(result: SearchResult | null): HTMLElement | null {
  const hits = result?.hits.filter((hit) => hit.strong).slice(0, 24) ?? [];
  if (hits.length < 3) return null;
  const votes = new Map<string, { weight: number; count: number; path: string }>();
  hits.forEach((hit, rank) => {
    const folder = folderOf(hit.path);
    const vote = votes.get(folder) ?? { weight: 0, count: 0, path: hit.path };
    vote.weight += 1 / (rank + 2);
    vote.count++;
    votes.set(folder, vote);
  });
  const [folder, vote] = [...votes].sort((a, b) => b[1].weight - a[1].weight)[0];
  if (vote.count < 2) return null;
  return h(
    "button.belongs.glass",
    { title: folder, onclick: () => void api.reveal(vote.path).catch(report) },
    icon("folder", 15),
    h("span", t("belongs.label")),
    h("b", lastFolder(folder)),
    h("span.pill", t("tidy.files", { n: vote.count })),
  );
}

/** A pad to draw on; `done` gets the drawing as a PNG file's path. */
export function sketchPad(done: (path: string) => void): void {
  const canvas = h("canvas", { width: 560, height: 400 });
  const pen = canvas.getContext("2d")!;
  const wipe = () => {
    pen.fillStyle = "#fff";
    pen.fillRect(0, 0, canvas.width, canvas.height);
  };
  wipe();
  pen.lineWidth = 5;
  pen.lineCap = "round";
  pen.lineJoin = "round";
  pen.strokeStyle = "#111";
  let drawing = false;
  let drawn = false;
  const at = (event: PointerEvent) => {
    const box = canvas.getBoundingClientRect();
    return [((event.clientX - box.left) / box.width) * canvas.width, ((event.clientY - box.top) / box.height) * canvas.height] as const;
  };
  canvas.addEventListener("pointerdown", (event) => {
    drawing = true;
    drawn = true;
    canvas.setPointerCapture(event.pointerId);
    pen.beginPath();
    pen.moveTo(...at(event));
  });
  canvas.addEventListener("pointermove", (event) => {
    if (!drawing) return;
    pen.lineTo(...at(event));
    pen.stroke();
  });
  canvas.addEventListener("pointerup", () => (drawing = false));
  const colours = ["#111111", "#d23b3b", "#e2a52b", "#2f9e5b", "#2f6fd8", "#8a5bd8"].map((colour) => {
    const dot = h("button.ink", { style: { background: colour }, "aria-label": colour });
    dot.addEventListener("click", () => (pen.strokeStyle = colour));
    return dot;
  });
  const close = () => veil.remove();
  const find = h("button.button.primary", icon("search", 16), t("sketch.find"));
  find.addEventListener("click", () => {
    if (!drawn) return;
    canvas.toBlob(async (blob) => {
      if (!blob) return;
      try {
        done(await api.saveExample(new Uint8Array(await blob.arrayBuffer()), "png"));
        close();
      } catch (error) {
        report(error);
      }
    }, "image/png");
  });
  const pad = h(
    "div.sketch.glass",
    h("header", h("b", t("sketch.title")), h("span.grow"), h("button.icon-button", { onclick: close, title: t("action.close") }, icon("close", 15))),
    canvas,
    h("footer", ...colours, h("span.grow"), h("button.button", { onclick: () => (wipe(), (drawn = false)) }, t("sketch.clear")), find),
  );
  const veil = h("div.veil", pad);
  veil.addEventListener("pointerdown", (event) => {
    if (event.target === veil) close();
  });
  document.body.append(veil);
}
