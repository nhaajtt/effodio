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

interface Node extends Point {
  /** Where it sits once the map has settled, and where it's drawn now (it flies in from the middle). */
  hx: number;
  hy: number;
  ties: Node[];
  group: number;
  picture?: HTMLImageElement;
  asked?: boolean;
}

interface Tie {
  a: Node;
  b: Node;
  strength: number;
}

interface Group {
  members: Node[];
  name: string;
  colour: string;
}

const PICTURED: Kind[] = ["image", "screenshot", "video"];

export async function mountMap(container: HTMLElement, onPick: (point: Point) => void): Promise<void> {
  const canvas = h("canvas");
  const tip = h("div.tip.glass", { hidden: true });
  const hidden = new Set<Kind>();
  const legend = h(
    "div.legend",
    ...(Object.keys(COLOURS) as Kind[]).map((kind) => {
      const pill = h("button.pill", { type: "button" }, h("i.swatch", { style: { background: COLOURS[kind] } }), t(`kind.${kind}`));
      pill.addEventListener("click", () => {
        if (hidden.has(kind)) hidden.delete(kind);
        else hidden.add(kind);
        pill.classList.toggle("off", hidden.has(kind));
        wake();
      });
      return pill;
    }),
  );
  const hint = h("div.map-hint", t("map.hint"));
  const side = h("aside.map-side.glass", { hidden: true });
  const map = h("div.map", canvas, tip, legend, hint, side);
  container.replaceChildren(h("div.view", h("div.heading", h("h2", t("map.title")), h("p", t("map.body"))), map));

  let nodes: Node[] = [];
  let ties: Tie[] = [];
  try {
    const answer = await api.ask<{ points: Point[]; links?: [number, number, number][] }>({ op: "map", limit: 6000 });
    nodes = answer.points.map((point) => ({ ...point, hx: 0, hy: 0, ties: [], group: -1 }));
    const byFile = new Map(nodes.map((node) => [node.file, node]));
    for (const [first, second, strength] of answer.links ?? []) {
      const a = byFile.get(first);
      const b = byFile.get(second);
      if (!a || !b) continue;
      ties.push({ a, b, strength });
      a.ties.push(b);
      b.ties.push(a);
    }
  } catch (error) {
    report(error);
  }
  if (nodes.length < 3) {
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
  const sx = spread(nodes.map((node) => node.x));
  const sy = spread(nodes.map((node) => node.y));
  for (const node of nodes) {
    node.hx = (node.x - sx.middle) / sx.half;
    node.hy = (node.y - sy.middle) / sy.half;
  }

  // Let it settle: tied files draw together, files on top of each other step apart, nothing strays far from
  // where its meaning put it. (Skipped for very large libraries, where the dots are a cloud anyway.)
  if (nodes.length <= 1400) {
    const start = nodes.map((node) => [node.hx, node.hy]);
    const apart = Math.min(0.2, 1.7 / Math.sqrt(nodes.length));
    for (let round = 0; round < 90; round++) {
      const heat = 1 - round / 90;
      for (const { a, b, strength } of ties) {
        const dx = b.hx - a.hx, dy = b.hy - a.hy;
        const far = Math.hypot(dx, dy) || 1e-6;
        const pull = ((far - apart * 1.15) / far) * 0.06 * strength * heat;
        a.hx += dx * pull;
        a.hy += dy * pull;
        b.hx -= dx * pull;
        b.hy -= dy * pull;
      }
      for (let i = 0; i < nodes.length; i++) {
        for (let j = i + 1; j < nodes.length; j++) {
          const a = nodes[i], b = nodes[j];
          let dx = b.hx - a.hx, dy = b.hy - a.hy;
          const far = Math.hypot(dx, dy);
          if (far >= apart) continue;
          if (far < 1e-6) {
            dx = Math.cos(i * 2.4);
            dy = Math.sin(i * 2.4);
          }
          const push = ((apart - far) / (far || 1)) * 0.5;
          a.hx -= dx * push;
          a.hy -= dy * push;
          b.hx += dx * push;
          b.hy += dy * push;
        }
      }
      nodes.forEach((node, index) => {
        node.hx += (start[index][0] - node.hx) * 0.02;
        node.hy += (start[index][1] - node.hy) * 0.02;
      });
    }
  }

  // Files tied together, directly or through others, make a group; a group of three or more gets a glow and a name.
  const groups: Group[] = [];
  for (const node of nodes) {
    if (node.group >= 0) continue;
    const members: Node[] = [];
    const queue = [node];
    node.group = groups.length;
    while (queue.length) {
      const next = queue.pop()!;
      members.push(next);
      for (const other of next.ties) {
        if (other.group < 0) {
          other.group = groups.length;
          queue.push(other);
        }
      }
    }
    const most = <T,>(values: T[]): T => {
      const counts = new Map<T, number>();
      values.forEach((value) => counts.set(value, (counts.get(value) ?? 0) + 1));
      return [...counts].sort((a, b) => b[1] - a[1])[0][0];
    };
    const folder = most(members.map((member) => folderOf(member.path).split(/[\\/›]/).map((part) => part.trim()).filter(Boolean).pop() ?? ""));
    groups.push({ members, name: folder, colour: COLOURS[most(members.map((member) => member.kind))] });
  }

  const context = canvas.getContext("2d")!;
  const style = getComputedStyle(document.documentElement);
  const ink = style.getPropertyValue("--ink").trim() || "#fff";
  const small = `10px ${style.getPropertyValue("--pixel").trim() || "monospace"}`;
  const names = `12px ${style.getPropertyValue("--font").trim() || "sans-serif"}`;
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const showPictures = () => nodes.length <= 90 || zoom >= 2.4;
  let zoom = 1;
  let offsetX = 0;
  let offsetY = 0;
  let hovered: Node | null = null;
  let selected: Node | null = null;
  let frame = 0;
  const born = performance.now();

  const scaleNow = () => (Math.min(canvas.clientWidth, canvas.clientHeight) / 2) * 0.8 * zoom;
  const place = (node: Node, arrive = 1) => {
    const scale = scaleNow();
    return { x: canvas.clientWidth / 2 + offsetX + node.hx * scale * arrive, y: canvas.clientHeight / 2 + offsetY + node.hy * scale * arrive };
  };
  const size = (node: Node) => {
    const base = Math.min(9, 3.4 + node.ties.length * 0.7) * Math.min(2.2, Math.sqrt(zoom)) * (nodes.length < 400 ? 1.25 : 1);
    return showPictures() && PICTURED.includes(node.kind) ? Math.max(base, Math.min(34, 15 * Math.sqrt(zoom))) : base;
  };
  const rgba = (hex: string, alpha: number) => {
    const value = parseInt(hex.slice(1), 16);
    return `rgba(${value >> 16},${(value >> 8) & 255},${value & 255},${alpha})`;
  };

  const draw = (now: number) => {
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
    const age = still ? 1 : Math.min(1, (now - born) / 1100);
    const arrive = 1 - Math.pow(1 - age, 3);
    const focus = hovered ?? selected;
    const lit = focus ? new Set<Node>([focus, ...focus.ties]) : null;
    const at = new Map<Node, { x: number; y: number }>();
    for (const node of nodes) at.set(node, place(node, arrive));

    // The glow under each group, and its name.
    for (const group of groups) {
      const shown = group.members.filter((member) => !hidden.has(member.kind));
      if (shown.length < 3) continue;
      const cx = shown.reduce((sum, member) => sum + at.get(member)!.x, 0) / shown.length;
      const cy = shown.reduce((sum, member) => sum + at.get(member)!.y, 0) / shown.length;
      const reach = Math.max(...shown.map((member) => Math.hypot(at.get(member)!.x - cx, at.get(member)!.y - cy))) + 46;
      const glow = context.createRadialGradient(cx, cy, 0, cx, cy, reach);
      glow.addColorStop(0, rgba(group.colour, 0.17 * arrive));
      glow.addColorStop(0.6, rgba(group.colour, 0.07 * arrive));
      glow.addColorStop(1, rgba(group.colour, 0));
      context.fillStyle = glow;
      context.beginPath();
      context.arc(cx, cy, reach, 0, Math.PI * 2);
      context.fill();
      if (group.name && !lit) {
        context.font = small;
        context.textAlign = "center";
        context.fillStyle = rgba(group.colour, 0.85 * arrive);
        const top = Math.min(...shown.map((member) => at.get(member)!.y - size(member)));
        context.fillText(`${group.name.toUpperCase()} · ${shown.length}`, cx, Math.max(14, top - 12));
      }
    }

    // The ties: a curve from each file to the files that mean most nearly the same.
    context.lineCap = "round";
    for (const tie of ties) {
      if (hidden.has(tie.a.kind) || hidden.has(tie.b.kind)) continue;
      const from = at.get(tie.a)!, to = at.get(tie.b)!;
      if (Math.max(from.x, to.x) < 0 || Math.min(from.x, to.x) > width || Math.max(from.y, to.y) < 0 || Math.min(from.y, to.y) > height) continue;
      const bright = !!lit && (tie.a === focus || tie.b === focus);
      const alpha = (lit ? (bright ? 0.95 : 0.06) : 0.3 + tie.strength * 0.5) * age;
      const line = context.createLinearGradient(from.x, from.y, to.x, to.y);
      line.addColorStop(0, rgba(COLOURS[tie.a.kind], alpha));
      line.addColorStop(1, rgba(COLOURS[tie.b.kind], alpha));
      context.strokeStyle = line;
      context.lineWidth = bright ? 2 : 1 + tie.strength * 1.1;
      // A slight bow, always to the same side, so two ties between neighbours don't lie on each other.
      const bow = 0.12;
      const mx = (from.x + to.x) / 2 - (to.y - from.y) * bow;
      const my = (from.y + to.y) / 2 + (to.x - from.x) * bow;
      context.setLineDash(bright && !still ? [5, 7] : []);
      context.lineDashOffset = bright ? -now / 32 : 0;
      context.beginPath();
      context.moveTo(from.x, from.y);
      context.quadraticCurveTo(mx, my, to.x, to.y);
      context.stroke();
    }
    context.setLineDash([]);

    // The files: glass beads, or the pictures themselves when there's room for them.
    const pictures = showPictures();
    for (const node of nodes) {
      if (hidden.has(node.kind)) continue;
      const spot = at.get(node)!;
      const radius = size(node);
      if (spot.x < -40 || spot.y < -40 || spot.x > width + 40 || spot.y > height + 40) continue;
      const dim = lit && !lit.has(node) ? 0.22 : 1;
      context.globalAlpha = dim * age;
      const colour = COLOURS[node.kind];
      context.shadowColor = rgba(colour, 0.9);
      context.shadowBlur = node === focus ? 26 : 12;
      if (pictures && PICTURED.includes(node.kind)) {
        if (!node.asked) {
          node.asked = true;
          api.thumbnail(node.path, 96).then(
            (url) => {
              const picture = new Image();
              picture.onload = () => {
                node.picture = picture;
                wake();
              };
              picture.src = url;
            },
            () => undefined,
          );
        }
        context.fillStyle = rgba(colour, 0.9);
        context.beginPath();
        context.arc(spot.x, spot.y, radius + 2, 0, Math.PI * 2);
        context.fill();
        context.shadowBlur = 0;
        if (node.picture) {
          context.save();
          context.beginPath();
          context.arc(spot.x, spot.y, radius, 0, Math.PI * 2);
          context.clip();
          const side = Math.min(node.picture.width, node.picture.height);
          context.drawImage(node.picture, (node.picture.width - side) / 2, (node.picture.height - side) / 2, side, side, spot.x - radius, spot.y - radius, radius * 2, radius * 2);
          context.restore();
        }
      } else {
        const bead = context.createRadialGradient(spot.x - radius * 0.35, spot.y - radius * 0.4, radius * 0.1, spot.x, spot.y, radius);
        bead.addColorStop(0, "rgba(255,255,255,0.95)");
        bead.addColorStop(0.28, colour);
        bead.addColorStop(1, rgba(colour, 0.72));
        context.fillStyle = bead;
        context.beginPath();
        context.arc(spot.x, spot.y, radius, 0, Math.PI * 2);
        context.fill();
        context.shadowBlur = 0;
      }
      if (lit?.has(node)) {
        context.strokeStyle = node === focus ? ink : rgba(colour, 0.95);
        context.lineWidth = node === focus ? 2 : 1.4;
        context.beginPath();
        context.arc(spot.x, spot.y, radius + 5, 0, Math.PI * 2);
        context.stroke();
        if (node !== focus) {
          context.font = names;
          context.textAlign = "center";
          context.fillStyle = ink;
          const name = fileName(node.path);
          context.fillText(name.length > 26 ? `${name.slice(0, 24)}…` : name, spot.x, spot.y + radius + 19);
        }
      }
    }
    context.globalAlpha = 1;
    if (age < 1 || (focus && focus.ties.length && !still)) wake();
  };
  function wake(): void {
    if (!frame) frame = requestAnimationFrame(draw);
  }
  new ResizeObserver(wake).observe(canvas);

  const nearest = (x: number, y: number): Node | null => {
    let best: Node | null = null;
    let distance = Infinity;
    for (const node of nodes) {
      if (hidden.has(node.kind)) continue;
      const spot = place(node);
      const reach = Math.max(14, size(node) + 4);
      const d = Math.hypot(spot.x - x, spot.y - y);
      if (d < reach && d < distance) {
        distance = d;
        best = node;
      }
    }
    return best;
  };

  // A picked file opens beside the map: what it is, and the files most like it. The search is one button away.
  const select = (node: Node | null) => {
    selected = node;
    side.hidden = !node;
    tip.hidden = true;
    if (node) {
      // Keep the picked dot clear of the panel.
      const over = place(node).x - (canvas.clientWidth - side.offsetWidth - 60);
      if (over > 0) offsetX -= over;
    }
    wake();
    if (!node) return;
    const shot = h("div.shot", kindIcon(node.kind));
    api.thumbnail(node.path, 512).then((url) => shot.replaceChildren(h("img", { alt: "", src: url })), () => undefined);
    const alike = h("div.alike", h("p", "…"));
    const facts = h("div.sub", folderOf(node.path));
    side.replaceChildren(
      h("button.close", { type: "button", title: t("map.close"), onclick: () => select(null) }, "×"),
      shot,
      h("div.name", fileName(node.path)),
      facts,
      h(
        "div.acts",
        h("button.button", { type: "button", onclick: () => void api.openResult(node.path).catch(report) }, t("map.open")),
        h("button.button", { type: "button", onclick: () => void api.reveal(node.path).catch(report) }, t("map.reveal")),
        h("button.button", { type: "button", onclick: () => onPick(node) }, t("map.search")),
      ),
      h("h4", t("map.alike")),
      alike,
    );
    side.scrollTop = 0;
    api.similar(node.file, "", 9).catch(() => ({ hits: [] as api.Hit[] })).then((result) => {
      if (selected !== node) return;
      // The files the map ties it to come first, then whatever else the search finds like it.
      const hits: { file: number; path: string; kind: Kind }[] = [...node.ties];
      for (const hit of result.hits) if (hit.file !== node.file && !hits.some((other) => other.file === hit.file)) hits.push(hit);
      hits.splice(8);
      alike.replaceChildren(
        ...(hits.length
          ? hits.map((hit) => {
              const thumb = h("div.thumb", kindIcon(hit.kind));
              api.thumbnail(hit.path, 96).then((url) => thumb.replaceChildren(h("img", { alt: "", src: url })), () => undefined);
              const there = nodes.find((other) => other.file === hit.file);
              return h(
                "button.like",
                { type: "button", title: hit.path, onclick: () => (there ? select(there) : void api.reveal(hit.path).catch(report)) },
                thumb,
                h("div", h("div.name", fileName(hit.path)), h("div.sub", folderOf(hit.path))),
              );
            })
          : [h("p", t("map.alone"))]),
      );
    }, report);
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
      wake();
      return;
    }
    const found = nearest(event.clientX - box.left, event.clientY - box.top);
    if (found !== hovered) {
      hovered = found;
      canvas.style.cursor = found ? "pointer" : "";
      wake();
      if (found) {
        const picture = h("img", { alt: "" });
        api.thumbnail(found.path, 96).then((url) => (picture.src = url), () => picture.remove());
        const related = found.ties.length ? h("div.related", t("map.related", { count: found.ties.length })) : null;
        tip.replaceChildren(
          picture,
          h("div", h("div", { style: { fontWeight: "600" } }, fileName(found.path)), h("div", { style: { color: "var(--ink-3)" } }, folderOf(found.path)), ...(related ? [related] : [])),
        );
      }
    }
    tip.hidden = !found || found === selected;
    if (found) {
      const spot = place(found);
      tip.style.left = `${Math.min(spot.x + size(found) + 12, box.width - 290)}px`;
      tip.style.top = `${Math.min(spot.y + 14, box.height - 84)}px`;
    }
  });
  canvas.addEventListener("pointerup", (event) => {
    const wasDrag = dragging?.moved;
    dragging = null;
    if (wasDrag) return;
    const box = canvas.getBoundingClientRect();
    const found = nearest(event.clientX - box.left, event.clientY - box.top);
    select(found);
  });
  canvas.addEventListener("pointerleave", () => {
    hovered = null;
    tip.hidden = true;
    wake();
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
      wake();
    },
    { passive: false },
  );
  wake();
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
