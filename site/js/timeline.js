import { pauseOffscreen } from "./frame.js";
import { TEXT } from "./site-data.js";
const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;
const coarse = matchMedia("(pointer: coarse)").matches;

const START_YEAR = 2024;
const END_YEAR = 2029;
const YEAR_W = 150;
const PAD = 24;
const BAR_H = 38;

const now = (() => {
  const d = new Date();
  const y = d.getFullYear();
  return y + (d - new Date(y, 0, 1)) / (new Date(y + 1, 0, 1) - new Date(y, 0, 1));
})();

const x = (year) => PAD + (year - START_YEAR) * YEAR_W;

const GLYPH = {
  cap: '<path d="M2 5.500 8 3l6 2.500-6 2.500z M4.500 7.500v3c0 .8 1.600 1.700 3.500 1.700s3.500-.9 3.500-1.700v-3" />',
  rack: '<rect x="2.500" y="2.500" width="11" height="4" rx=".8"/><rect x="2.500" y="9.500" width="11" height="4" rx=".8"/><path d="M5 4.500h.01M5 11.500h.01"/>',
  flag: '<path d="M4 14V2.500M4 3h8l-2 2.500L12 8H4"/>',
  bug: '<path d="M8 5.500c1.700 0 2.800 1.200 2.800 3S9.700 12.500 8 12.500 5.200 10.300 5.200 8.500 6.300 5.500 8 5.500zM6.500 4l1.500 1.500L9.500 4M3 8h2.200M10.800 8H13M3.500 11l1.800-1M12.500 11l-1.800-1"/>',
  cloud: '<path d="M4.500 12a2.800 2.800 0 0 1-.4-5.500A3.800 3.800 0 0 1 11.400 6 2.600 2.600 0 0 1 11.500 12z"/>',
  cert: '<circle cx="8" cy="6.500" r="3.500"/><path d="M6 9.500 5 14l3-1.500 3 1.500-1-4.500"/>',
};
const glyph = (k) =>
  `<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.300" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">${GLYPH[k]}</svg>`;

const T = TEXT.timeline;
const ITEMS = [
  {
    id: "glint", row: 0, logo: "flag", label: T.now[0], sub: T.now[1], start: 2026.6, end: now, range: "(2026)", current: true,
    next: { logo: "cloud", label: T.now[2], sub: T.now[3] },
    preview: { title: "Glint 0.1", lines: ["$ glint --version", "0.1.0, Windows 10 and 11", "$ glint status", "runs offline, on your PC"] },
  },
  { id: "signed", row: 1, logo: "cert", label: T.signed[0], sub: T.signed[1], start: 2027, end: 2028, range: "", future: true },
  { id: "gpu", row: 2, logo: "rack", label: T.gpu[0], sub: T.gpu[1], start: 2026.9, end: 2027.9, range: "", future: true },
  {
    id: "model", row: 3, logo: "cloud", label: T.model[0], sub: T.model[1], start: 2025.9, end: 2026.9, range: "", muted: true,
    href: "https://huggingface.co/google/embeddinggemma-2", cursor: "Open the model",
  },
  {
    id: "digup", row: 4, logo: "bug", label: T.digup[0], sub: T.digup[1], start: 2025.2, end: 2026.2, range: "", muted: true, faint: true,
    href: "https://github.com/ARahim3/DigUp", cursor: "View on GitHub",
  },
];
const ROWS = Math.max(...ITEMS.map((i) => i.row)) + 1;

const CHEV = [
  [0, 0], [1, 1], [2, 2], [3, 3], [2, 4], [1, 5], [0, 6],
];
const chevSvg = () =>
  '<svg class="tl-chev" viewBox="0 0 9 7" aria-hidden="true">' +
  [0, 5]
    .flatMap((o) => CHEV.map(([a, b]) => [a + o, b]))
    .map(([a, b]) => `<rect class="tl-chev-px" x="${a}" y="${b}" width="1" height="1" style="--col:${a}"/>`)
    .join("") +
  "</svg>";

const LOCK =
  '<svg viewBox="0 0 12 12" stroke="currentColor" aria-hidden="true"><rect x="2.4" y="5.2" width="7.2" height="5" rx="1.3" fill="none" stroke-width="1.3"/><path d="M4 5.2 V3.9 a2 2 0 0 1 4 0 V5.2" fill="none" stroke-width="1.3"/></svg>';

const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

function barMain(it) {
  const logo = (k, bare) =>
    `<span class="tl-bar-logo tl-bar-logo--glyph${bare ? " tl-bar-logo--bare" : ""}" aria-hidden="true">${glyph(k)}</span>`;
  const label = (l, s) => `<span class="tl-bar-label">${esc(l)}${s ? `<span class="tl-bar-sub">${esc(s)}</span>` : ""}</span>`;
  const range = it.range ? `<span class="tl-bar-range${it.current ? " is-current" : ""}">${it.range}</span>` : "";
  if (it.next) {
    return (
      `<span class="tl-bar-main"><span class="tl-bar-grp">${logo(it.logo)}${label(it.label, it.sub)}${range}</span>` +
      `<span class="tl-bar-gap" aria-hidden="true">${chevSvg()}</span>` +
      `<span class="tl-bar-grp">${logo(it.next.logo, true)}${label(it.next.label, it.next.sub)}</span></span>`
    );
  }
  return `<span class="tl-bar-main">${logo(it.logo)}${label(it.label, it.sub)}${range}</span>`;
}

function mount() {
  const host = document.getElementById("timeline-mount");
  if (!host) return;

  const section = document.createElement("section");
  section.className = "tl";
  section.setAttribute("aria-label", "Timeline");
  const scroll = document.createElement("div");
  scroll.className = "tl-scroll";
  scroll.setAttribute("role", "group");
  scroll.setAttribute("aria-label", "Scroll horizontally to move through the years");
  const track = document.createElement("div");
  track.className = "tl-track";
  track.style.width = x(END_YEAR) + PAD + "px";

  const years = Array.from({ length: END_YEAR - START_YEAR + 1 }, (_, i) => START_YEAR + i);
  const thisYear = Math.floor(now);
  let html = '<div class="tl-axis" aria-hidden="true">';
  years.forEach((y) => (html += `<span class="tl-tick${y > thisYear ? " is-future" : ""}" style="left:${x(y)}px">${y}</span>`));
  html += "</div>";
  years.forEach((y) => (html += `<span class="tl-grid" style="left:${x(y)}px" aria-hidden="true"></span>`));
  years.forEach((y) =>
    [0.25, 0.5, 0.75].forEach((q) => {
      if (y + q <= END_YEAR) html += `<span class="tl-grid tl-grid--q" style="left:${x(y + q)}px" aria-hidden="true"></span>`;
    })
  );
  html += `<span class="tl-future" style="left:${x(now)}px" aria-hidden="true"></span>`;
  html += `<span class="tl-now" style="left:${x(now)}px" aria-hidden="true"><span class="tl-now-dot"></span></span>`;
  track.innerHTML = html;

  let rowH = 72;
  const bars = ITEMS.map((it) => {
    const n = document.createElement(it.href ? "a" : "div");
    n.className =
      "tl-bar" +
      (it.current ? " is-current" : "") +
      (it.next ? " tl-bar--umbrella" : "") +
      (it.muted ? " is-muted" : "") +
      (it.faint ? " is-faint" : "") +
      (it.future ? " is-future" : "") +
      (it.href ? " tl-bar--link" : "");
    if (it.href) {
      n.href = it.href;
      n.target = "_blank";
      n.rel = "noopener noreferrer";
    }
    n.innerHTML = barMain(it);
    n.style.left = x(it.start) + "px";
    n.style.width = x(it.end) - x(it.start) + (it.current ? YEAR_W : 0) + "px";
    n.style.height = BAR_H + "px";
    track.append(n);
    return { it, n };
  });

  function layout() {
    const h = scroll.clientHeight;
    const roomy = h > 120;
    rowH = roomy ? (h - 30) / ROWS : 72;
    track.style.height = roomy ? h + "px" : 30 + 72 * ROWS + 20 + "px";
    /* phones: 44px bars so every bar is a comfortable tap target */
    const bh = matchMedia("(max-width: 759px)").matches ? 44 : BAR_H;
    bars.forEach(({ it, n }) => {
      n.style.height = bh + "px";
      n.style.top = 30 + it.row * rowH + (rowH - bh) / 2 + "px";
    });
  }

  scroll.append(track);
  section.append(scroll);
  section.style.setProperty("--tl-rows", String(ROWS));
  const hint = document.createElement("p");
  hint.className = "tl-hint";
  hint.setAttribute("aria-hidden", "true");
  hint.innerHTML = `<span class="tl-hint-arrows">← →</span> ${TEXT.hint}`;
  section.append(hint);
  host.append(section);
  pauseOffscreen(host);

  /* match the intro's height like the original (side by side layouts only; phones size it in CSS) */
  const intro = document.getElementById("intro-mount");
  const tlWrap = host;
  if (intro) {
    const wide = matchMedia("(min-width: 760px)");
    const sync = () => {
      tlWrap.style.height = wide.matches ? intro.getBoundingClientRect().height + "px" : "";
    };
    sync();
    new ResizeObserver(sync).observe(intro);
    wide.addEventListener("change", sync);
  }
  scroll.addEventListener("scroll", () => section.classList.add("is-swiped"), { passive: true, once: true });
  new ResizeObserver(layout).observe(scroll);
  layout();

  scroll.scrollLeft = scroll.clientWidth < 700 ? x(2026) - PAD : x(now) - 0.62 * scroll.clientWidth;

  /* worn stretches: the room's wear grid lives on the page, not on the years, so every bar is read at the spot of
     the document it sits on right now. Wherever along the strip people stood takes a polish, and a bar scrolled
     under that spot picks it up. The strip itself is stuck to the scroller's window (css position: sticky). */
  const wear = document.createElement("div");
  wear.className = "tl-wear";
  wear.setAttribute("aria-hidden", "true");
  const sheen = wear.appendChild(document.createElement("i"));
  track.prepend(wear);
  let scale = 0, box = null, polishing = 0;
  function survey() {
    const w = window.__world;
    if (!w || !w.wearAt) return;
    const r = scroll.getBoundingClientRect();
    const docW = document.documentElement.clientWidth || innerWidth;
    const docH = Math.max(document.documentElement.scrollHeight, innerHeight);
    const top = r.top + scrollY;
    box = { left: r.left, top, w: r.width, h: r.height, at: (cx, dy) => (cx < 0 || cx > docW ? 0 : w.wearAt(cx / docW, (top + dy) / docH)) };
    const N = 48, cols = [];
    let peak = 0;
    for (let i = 0; i < N; i++) {
      const cx = r.left + (i / (N - 1)) * r.width;
      const v = Math.max(box.at(cx, r.height * 0.25), box.at(cx, r.height * 0.5), box.at(cx, r.height * 0.75));
      peak = Math.max(peak, v);
      cols.push(v);
    }
    /* measured against the strip's own most worn spot: the busiest corner of the page would flatten it to nothing.
       The floor under the divisor keeps a single stray footstep from reading as a path. */
    scale = peak > 0.004 ? 1 / Math.max(peak, 0.1) : 0;
    const stops = cols.map((v, i) => `rgb(255 220 180 / ${(Math.pow(Math.min(1, v * scale), 1.4) * 0.14).toFixed(3)}) ${((i / (N - 1)) * 100).toFixed(1)}%`);
    sheen.style.width = r.width + "px";
    sheen.style.height = Math.max(0, r.height - 36) + "px";
    sheen.style.background = `linear-gradient(90deg, ${stops.join(",")})`;
    sheen.style.opacity = scale ? 1 : 0;
    polish();
  }
  function polish() {
    polishing = 0;
    if (!box) return;
    const sl = scroll.scrollLeft;
    for (const b of bars) {
      const n = b.n;
      b.l ??= n.offsetLeft; b.w ??= n.offsetWidth; b.t ??= n.offsetTop; b.h ??= n.offsetHeight;
      /* the middle of the part of the bar that is in view */
      const a = Math.max(b.l - sl, 0), z = Math.min(b.l + b.w - sl, box.w);
      const v = z <= a ? 0 : Math.min(1, box.at(box.left + (a + z) / 2, b.t + b.h / 2) * scale);
      const s = (Math.round(v * 20) / 20).toFixed(2);
      if (s !== b.wear) { b.wear = s; n.style.setProperty("--wear", s); }
    }
  }
  const resurvey = () => { for (const b of bars) b.l = b.w = b.t = b.h = undefined; survey(); };
  scroll.addEventListener("scroll", () => { if (box && !polishing) polishing = requestAnimationFrame(polish); }, { passive: true });
  new ResizeObserver(resurvey).observe(scroll);
  addEventListener("lab:world", (e) => { if (e.detail.what === "wear") survey(); });
  survey();

  /* drag to pan */
  scroll.addEventListener("pointerdown", (e) => {
    if (e.button !== 0 || e.pointerType !== "mouse") return;
    const x0 = e.clientX;
    const l0 = scroll.scrollLeft;
    let moved = false;
    scroll.classList.add("is-dragging");
    const move = (ev) => {
      const dx = ev.clientX - x0;
      if (Math.abs(dx) > 3) moved = true;
      scroll.scrollLeft = l0 - dx;
    };
    const up = () => {
      scroll.classList.remove("is-dragging");
      removeEventListener("pointermove", move);
      removeEventListener("pointerup", up);
      removeEventListener("pointercancel", up);
      setTimeout(() => scroll.removeEventListener("click", block, true), 0);
    };
    const block = (ev) => {
      if (moved) {
        ev.preventDefault();
        ev.stopPropagation();
      }
      scroll.removeEventListener("click", block, true);
    };
    scroll.addEventListener("click", block, true);
    addEventListener("pointermove", move);
    addEventListener("pointerup", up);
    addEventListener("pointercancel", up);
  });
  scroll.addEventListener("dragstart", (e) => e.preventDefault());

  /* hover: orbiting light, preview card, cursor label */
  const cursor = coarse ? null : makeCursor(section);
  let orbitEl = null;
  let orbitRaf = 0;
  let preview = null;

  const stopOrbit = () => {
    cancelAnimationFrame(orbitRaf);
    orbitEl?.remove();
    orbitEl = null;
  };
  const startOrbit = (n, current) => {
    stopOrbit();
    if (reduced) return;
    const o = document.createElement("span");
    o.className = "tl-orbit" + (current ? " is-current" : "");
    Object.assign(o.style, { left: n.offsetLeft + "px", top: n.offsetTop + "px", width: n.offsetWidth + "px", height: n.offsetHeight + "px" });
    o.setAttribute("aria-hidden", "true");
    track.append(o);
    orbitEl = o;
    const w = n.offsetWidth;
    const h = n.offsetHeight;
    const perim = 2 * (w + h);
    const k = Math.min(1, perim / 1200);
    const vmax = 900 * k;
    const vmin = Math.max(80, 160 * k);
    let dist = 0;
    let last = performance.now();
    const trail = [];
    const pt = (d) =>
      d < w ? [d, 0] : d < w + h ? [w, d - w] : d < 2 * w + h ? [w - (d - w - h), h] : [0, h - (d - 2 * w - h)];
    const ang = ([px, py]) => (180 * Math.atan2(px - w / 2, h / 2 - py)) / Math.PI;
    const frame = (t) => {
      const d = dist % perim;
      const p = pt(d);
      const a = ang(p);
      trail.push({ t, dist });
      while (trail.length > 2 && trail[0].t < t - 700) trail.shift();
      const back = (((d - Math.min(dist - trail[0].dist, 0.45 * perim)) % perim) + perim) % perim;
      const a2 = ang(pt(back));
      o.style.setProperty("--tl-orbit-a", a + "deg");
      o.style.setProperty("--tl-orbit-t", ((((a - a2) % 360) + 360) % 360) + "deg");
      const v = Math.min(1, Math.min(p[0], w - p[0]) / 30);
      dist += ((vmin + (vmax - vmin) * v * v * (3 - 2 * v)) * (t - last)) / 1000;
      last = t;
      orbitRaf = requestAnimationFrame(frame);
    };
    orbitRaf = requestAnimationFrame(frame);
  };

  const showPreview = (it, rect) => {
    hidePreview();
    const p = it.preview;
    if (!p || coarse) return;
    const w = 300;
    const h = 169;
    const left = Math.min(Math.max(rect.left + 20, 12), innerWidth - w - 12);
    const top0 = rect.top - h - 14;
    const top = top0 >= 72 ? top0 : rect.bottom + 14;
    const card = document.createElement("div");
    card.className = "tl-preview";
    card.setAttribute("aria-hidden", "true");
    Object.assign(card.style, { left: left + "px", top: top + "px", width: w + "px", height: h + "px" });
    card.innerHTML =
      `<div class="tl-preview-card"><div class="tl-preview-bar"><i></i><i></i><i></i><span>${esc(p.title)}</span></div>` +
      `<div class="tl-preview-body">${p.lines.map((l) => `<div>${esc(l)}</div>`).join("")}<div class="tl-preview-caret">_</div></div></div>`;
    document.body.append(card);
    preview = card;
  };
  const hidePreview = () => {
    preview?.remove();
    preview = null;
  };

  bars.forEach(({ it, n }) => {
    n.addEventListener("mouseenter", () => {
      startOrbit(n, !!it.current);
      if (it.preview) showPreview(it, n.getBoundingClientRect());
      if (it.href && cursor) cursor.show(`${it.cursor ?? "View"} ${it.href.startsWith("/") ? "→" : "↗"}`);
    });
    n.addEventListener("mouseleave", () => {
      stopOrbit();
      hidePreview();
      cursor?.hide();
    });
  });
  scroll.addEventListener("scroll", () => {
    if (preview) hidePreview();
  }, { passive: true });
}

/* orange arrow plus label that trails the pointer over linked bars */
function makeCursor(parent) {
  const wrap = document.createElement("div");
  wrap.className = "tl-cursor";
  wrap.setAttribute("aria-hidden", "true");
  const label = document.createElement("div");
  label.className = "tl-cursor-label";
  label.style.opacity = "0";
  const arrow = document.createElement("div");
  arrow.className = "tl-cursor-arrow";
  arrow.style.opacity = "0";
  arrow.innerHTML =
    '<svg width="31" height="31" viewBox="0 0 28 28" fill="none" style="display:block;overflow:visible"><path d="M5 3 L23 14 L14 16 L11 24 Z" fill="var(--ac)" stroke="rgba(0,0,0,0.3)" stroke-width="1" stroke-linejoin="round"/></svg>';
  wrap.append(label, arrow);
  parent.append(wrap);

  const OFF_X = 27.9;
  const OFF_Y = 12.2;
  const spring = (k, c, m) => ({ k, c, m, v: 0, p: -9999, t: -9999 });
  const aX = spring(380, 32, 0.6);
  const aY = spring(380, 32, 0.6);
  const lX = spring(220, 26, 0.7);
  const lY = spring(220, 26, 0.7);
  const rot = spring(200, 24, 0.6);
  const scl = spring(500, 28, 0.5);
  scl.p = scl.t = 1;
  let active = false;
  let raf = 0;
  let last = 0;
  let prevPt = null;
  let seeded = false;

  const step = (s, dt) => {
    const n = Math.max(1, Math.ceil(dt / 0.004));
    const h = dt / n;
    for (let i = 0; i < n; i++) {
      const a = (s.k * (s.t - s.p) - s.c * s.v) / s.m;
      s.v += a * h;
      s.p += s.v * h;
    }
  };
  const settled = (s) => Math.abs(s.t - s.p) < 0.05 && Math.abs(s.v) < 0.05;

  const frame = (t) => {
    const dt = Math.min(0.05, (t - last) / 1000);
    last = t;
    [aX, aY, lX, lY, rot, scl].forEach((s) => step(s, dt));
    label.style.transform = `translate(${lX.p + OFF_X}px,${lY.p + OFF_Y}px) rotate(${rot.p}deg) scale(${scl.p})`;
    arrow.style.transform = `translate(${aX.p}px,${aY.p}px) scale(${scl.p})`;
    if (active || ![aX, aY, lX, lY, rot, scl].every(settled)) raf = requestAnimationFrame(frame);
    else raf = 0;
  };
  const kick = () => {
    if (!raf) {
      last = performance.now();
      raf = requestAnimationFrame(frame);
    }
  };

  const onMove = (e) => {
    const now = performance.now();
    if (prevPt) {
      const dtp = Math.max(1, now - prevPt.t);
      const vx = ((e.clientX - prevPt.x) / dtp) * 1000;
      const vy = ((e.clientY - prevPt.y) / dtp) * 1000;
      rot.t = Math.sign(vx) * Math.min(1, Math.hypot(vx, vy) / 1500) * 25;
    }
    prevPt = { x: e.clientX, y: e.clientY, t: now };
    aX.t = lX.t = e.clientX;
    aY.t = lY.t = e.clientY;
    if (!seeded || !active) {
      aX.p = lX.p = e.clientX;
      aY.p = lY.p = e.clientY;
      seeded = true;
    }
    if (active) kick();
  };
  const onDown = () => {
    scl.t = 0.92;
    kick();
  };
  const onUp = () => {
    scl.t = 1;
    kick();
  };
  addEventListener("mousemove", onMove);
  addEventListener("mousedown", onDown);
  addEventListener("mouseup", onUp);

  return {
    show(text) {
      label.textContent = text;
      if (!active) {
        aX.p = aX.t;
        aY.p = aY.t;
        lX.p = lX.t;
        lY.p = lY.t;
      }
      active = true;
      label.style.opacity = "1";
      arrow.style.opacity = "1";
      kick();
    },
    hide() {
      active = false;
      rot.t = 0;
      prevPt = null;
      label.style.opacity = "0";
      arrow.style.opacity = "0";
    },
  };
}

mount();
