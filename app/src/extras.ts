// Two other ways into the index: the map (every file as a dot, placed by meaning) and the duplicates.

import * as api from "./api";
import { Kind } from "./api";
import { t } from "./i18n";
import { report } from "./results";
import { bytes, fileName, folderOf, h, icon, kindIcon } from "./ui";

interface Point {
  file: number;
  x: number;
  y: number;
  kind: Kind;
  path: string;
}

const COLOURS: Record<Kind, string> = {
  image: "#e2b34a",
  screenshot: "#5fa8f5",
  pdf: "#ef6f6c",
  doc: "#59c9a5",
  audio: "#b18cf2",
  video: "#f08fc0",
};

export async function mountMap(container: HTMLElement, onPick: (point: Point) => void): Promise<void> {
  const canvas = h("canvas");
  const tip = h("div.tip.glass", { hidden: true });
  const legend = h(
    "div.legend",
    ...(Object.keys(COLOURS) as Kind[]).map((kind) => h("span.pill", h("i.swatch", { style: { background: COLOURS[kind] } }), t(`kind.${kind}`))),
  );
  const map = h("div.map", canvas, tip, legend);
  container.replaceChildren(h("div.view", h("div.heading", h("h2", t("map.title")), h("p", t("map.body"))), map));

  let points: Point[] = [];
  try {
    points = (await api.ask<{ points: Point[] }>({ op: "map", limit: 6000 })).points;
  } catch (error) {
    report(error);
  }
  if (points.length < 3) {
    map.replaceChildren(h("div.empty", h("div.orb.glass", icon("map", 30)), h("p", t("map.empty"))));
    return;
  }
  // Spread to fill the view: the two directions rarely vary by the same amount.
  const spread = (values: number[]) => {
    const sorted = [...values].sort((a, b) => a - b);
    const low = sorted[Math.floor(sorted.length * 0.01)];
    const high = sorted[Math.ceil(sorted.length * 0.99) - 1];
    return { middle: (low + high) / 2, half: Math.max((high - low) / 2, 1e-6) };
  };
  const sx = spread(points.map((p) => p.x));
  const sy = spread(points.map((p) => p.y));
  for (const point of points) {
    point.x = (point.x - sx.middle) / sx.half;
    point.y = (point.y - sy.middle) / sy.half;
  }

  const context = canvas.getContext("2d")!;
  let zoom = 1;
  let offsetX = 0;
  let offsetY = 0;
  let hovered: Point | null = null;
  let frame = 0;

  const place = (point: Point) => {
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    const scale = (Math.min(width, height) / 2) * 0.84 * zoom;
    return { x: width / 2 + offsetX + point.x * scale, y: height / 2 + offsetY + point.y * scale };
  };

  const draw = () => {
    frame = 0;
    const ratio = window.devicePixelRatio || 1;
    const width = canvas.clientWidth;
    const height = canvas.clientHeight;
    if (canvas.width !== Math.round(width * ratio) || canvas.height !== Math.round(height * ratio)) {
      canvas.width = Math.round(width * ratio);
      canvas.height = Math.round(height * ratio);
    }
    context.setTransform(ratio, 0, 0, ratio, 0, 0);
    context.clearRect(0, 0, width, height);
    const radius = Math.max(2.2, Math.min(6, 2.2 * Math.sqrt(zoom) * (points.length < 400 ? 1.6 : 1)));
    context.globalAlpha = 0.82;
    for (const point of points) {
      const at = place(point);
      if (at.x < -10 || at.y < -10 || at.x > width + 10 || at.y > height + 10) continue;
      context.fillStyle = COLOURS[point.kind];
      context.beginPath();
      context.arc(at.x, at.y, radius, 0, Math.PI * 2);
      context.fill();
    }
    context.globalAlpha = 1;
    if (hovered) {
      const at = place(hovered);
      context.strokeStyle = getComputedStyle(document.documentElement).getPropertyValue("--ink");
      context.lineWidth = 2;
      context.beginPath();
      context.arc(at.x, at.y, radius + 4, 0, Math.PI * 2);
      context.stroke();
    }
  };
  const redraw = () => {
    if (!frame) frame = requestAnimationFrame(draw);
  };
  new ResizeObserver(redraw).observe(canvas);

  const nearest = (x: number, y: number): Point | null => {
    let best: Point | null = null;
    let distance = 14 * 14;
    for (const point of points) {
      const at = place(point);
      const d = (at.x - x) ** 2 + (at.y - y) ** 2;
      if (d < distance) {
        distance = d;
        best = point;
      }
    }
    return best;
  };

  let dragging: { x: number; y: number; moved: boolean } | null = null;
  canvas.addEventListener("pointerdown", (event) => {
    dragging = { x: event.clientX, y: event.clientY, moved: false };
    canvas.setPointerCapture(event.pointerId);
  });
  canvas.addEventListener("pointermove", (event) => {
    const box = canvas.getBoundingClientRect();
    if (dragging) {
      const dx = event.clientX - dragging.x;
      const dy = event.clientY - dragging.y;
      if (Math.abs(dx) + Math.abs(dy) > 3) dragging.moved = true;
      offsetX += dx;
      offsetY += dy;
      dragging.x = event.clientX;
      dragging.y = event.clientY;
      tip.hidden = true;
      redraw();
      return;
    }
    const found = nearest(event.clientX - box.left, event.clientY - box.top);
    if (found !== hovered) {
      hovered = found;
      redraw();
      if (found) {
        const picture = h("img", { alt: "" });
        api.thumbnail(found.path, 96).then((url) => (picture.src = url), () => picture.remove());
        tip.replaceChildren(picture, h("div", h("div", { style: { fontWeight: "600" } }, fileName(found.path)), h("div", { style: { color: "var(--ink-3)" } }, folderOf(found.path))));
      }
    }
    tip.hidden = !found;
    if (found) {
      const at = place(found);
      tip.style.left = `${Math.min(at.x + 14, box.width - 290)}px`;
      tip.style.top = `${Math.min(at.y + 14, box.height - 70)}px`;
    }
  });
  canvas.addEventListener("pointerup", (event) => {
    const wasDrag = dragging?.moved;
    dragging = null;
    if (wasDrag) return;
    const box = canvas.getBoundingClientRect();
    const found = nearest(event.clientX - box.left, event.clientY - box.top);
    if (found) onPick(found);
  });
  canvas.addEventListener("pointerleave", () => {
    hovered = null;
    tip.hidden = true;
    redraw();
  });
  canvas.addEventListener(
    "wheel",
    (event) => {
      event.preventDefault();
      const box = canvas.getBoundingClientRect();
      const factor = Math.exp(-event.deltaY * 0.0015);
      const next = Math.max(0.6, Math.min(40, zoom * factor));
      // Zoom about the pointer.
      const px = event.clientX - box.left - box.width / 2;
      const py = event.clientY - box.top - box.height / 2;
      offsetX = px - ((px - offsetX) * next) / zoom;
      offsetY = py - ((py - offsetY) * next) / zoom;
      zoom = next;
      redraw();
    },
    { passive: false },
  );
  redraw();
}

interface Member {
  file: number;
  path: string;
  kind: Kind;
  size: number;
  modified: number;
}

export async function mountDuplicates(container: HTMLElement): Promise<void> {
  const groups = h("div.groups", h("div.empty", h("p", "…")));
  container.replaceChildren(h("div.view", h("div.heading", h("h2", t("dupes.title")), h("p", t("dupes.body"))), groups));
  let found: Member[][] = [];
  try {
    found = (await api.ask<{ groups: Member[][] }>({ op: "duplicates" })).groups;
  } catch (error) {
    report(error);
  }
  if (!found.length) {
    groups.replaceChildren(h("div.empty", h("div.orb.glass", icon("check", 30)), h("p", t("dupes.none"))));
    return;
  }
  groups.replaceChildren(
    ...found.map((group) => {
      const exact = group.every((member) => member.size === group[0].size);
      return h(
        "div.group-card.glass",
        h("header", t("dupes.group", { n: group.length }), exact ? h("span.pill", t("dupes.exact")) : null, h("span.pill", bytes(group.reduce((sum, member) => sum + member.size, 0)))),
        h(
          "div.members",
          ...group.map((member) => {
            const thumb = h("div.thumb", kindIcon(member.kind));
            api.thumbnail(member.path, 256).then((url) => {
              const image = h("img.ready", { alt: "", src: url, style: { width: "100%", height: "100%", objectFit: "cover" } });
              thumb.replaceChildren(image);
            }, () => {});
            return h(
              "button.member",
              { title: member.path, onclick: () => void api.reveal(member.path).catch(report) },
              thumb,
              h("div.name", fileName(member.path)),
              h("div.sub", `${bytes(member.size)} · ${folderOf(member.path)}`),
            );
          }),
        ),
      );
    }),
  );
}
