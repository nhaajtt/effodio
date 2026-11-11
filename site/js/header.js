const SITE_URL = "https://effodio.nhaajt.com";
import { TEXT, LANG, INSTALLER, COMMAND } from "./site-data.js";
import { onFrame, kick, pauseOffscreen } from "./frame.js";

const reduced = matchMedia("(prefers-reduced-motion: reduce)").matches;

const cmd = (name, ...args) =>
  window.dispatchEvent(new CustomEvent("lab:cmd", { detail: { cmd: name, args } }));

/* ---------- terminal state shared by header button and intro row ---------- */
const term = {
  open: window.innerWidth >= 760,
  subs: new Set(),
  set(v) {
    if (this.open === v) return;
    this.open = v;
    this.subs.forEach((f) => f(v));
  },
};
window.addEventListener("lab:terminal", (e) => {
  if (e.detail && typeof e.detail.open === "boolean") term.set(e.detail.open);
});

function toggleTerminal() {
  document.getElementById("install")?.scrollIntoView({ behavior: "smooth", block: "start" });
}

function el(tag, attrs = {}, html) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) n.setAttribute(k, v);
  if (html != null) n.innerHTML = html;
  return n;
}

// The install button: the same shape as a terminal prompt.
function terminalButton(showLabel) {
  const b = el("button", { type: "button", class: "hdr-terminal", "aria-label": TEXT.install, title: TEXT.install });
  b.innerHTML =
    (showLabel ? `<span class="hdr-terminal-label" aria-hidden="true">${TEXT.install}</span>` : "") +
    '<span aria-hidden="true">&gt;</span><span class="hdr-terminal-cursor" aria-hidden="true">_</span>';
  b.addEventListener("click", toggleTerminal);
  return b;
}

function languageButton() {
  const b = el("button", { type: "button", class: "hdr-cmd", title: LANG === "vi" ? "English" : "Tiếng Việt" },
    `<span class="hdr-cmd-label">${LANG === "vi" ? "EN" : "VI"}</span>`);
  b.addEventListener("click", () => {
    try {
      localStorage.setItem("lang", LANG === "vi" ? "en" : "vi");
    } catch {}
    location.href = location.pathname;
  });
  return b;
}

/* ---------- header ---------- */
function mountHeader() {
  const hdr = document.getElementById("hdr-mount");
  if (!hdr) return;
  const right = el("div", { class: "hdr-right" });
  const jump = el(
    "button",
    { type: "button", class: "hdr-cmd", title: "Jump to… (⌘K)" },
    `<span class="hdr-cmd-label">${TEXT.jump}</span><span class="hdr-cmd-keys" aria-hidden="true"><kbd>⌘</kbd><kbd>K</kbd></span>`
  );
  jump.addEventListener("click", () => palette.toggle());
  if (!/Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent)) {
    const keys = jump.querySelector("kbd");
    if (keys) keys.textContent = "Ctrl";
    jump.title = "Jump to… (Ctrl+K)";
  }
  right.append(jump, languageButton(), terminalButton(false));
  hdr.append(right);
  if (!reduced) orbit(jump);
}

/* light that travels around the pill border while hovered */
function orbit(node) {
  let raf = 0;
  let running = false;
  const start = () => {
    if (running) return;
    running = true;
    const { width: w, height: h } = node.getBoundingClientRect();
    const perim = 2 * (w + h);
    const k = Math.min(1, perim / 1200);
    const vmax = 900 * k;
    const vmin = Math.max(80, 160 * k);
    let dist = 0;
    let last = performance.now();
    const trail = [];
    const pt = (d) =>
      d < w ? [d, 0] : d < w + h ? [w, d - w] : d < 2 * w + h ? [w - (d - w - h), h] : [0, h - (d - 2 * w - h)];
    const ang = ([x, y]) => (180 * Math.atan2(x - w / 2, h / 2 - y)) / Math.PI;
    const frame = (now) => {
      const d = dist % perim;
      const p = pt(d);
      const a = ang(p);
      trail.push({ t: now, dist });
      while (trail.length > 2 && trail[0].t < now - 700) trail.shift();
      const back = (((d - Math.min(dist - trail[0].dist, 0.45 * perim)) % perim) + perim) % perim;
      const a2 = ang(pt(back));
      node.style.setProperty("--hdr-orbit-a", a + "deg");
      node.style.setProperty("--hdr-orbit-t", ((((a - a2) % 360) + 360) % 360) + "deg");
      const v = Math.min(1, Math.min(p[0], w - p[0]) / 30);
      dist += ((vmin + (vmax - vmin) * v * v * (3 - 2 * v)) * (now - last)) / 1000;
      last = now;
      raf = requestAnimationFrame(frame);
    };
    last = performance.now();
    raf = requestAnimationFrame(frame);
  };
  const stop = () => {
    running = false;
    cancelAnimationFrame(raf);
  };
  node.addEventListener("pointerenter", start);
  node.addEventListener("pointerleave", stop);
}

/* ---------- command palette ---------- */
const ICONS = {
  layers: "M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5M3 17l9 5 9-5",
  pointer: "M4 3l14 6-6 2-2 6z",
  compass: "M12 3a9 9 0 100 18 9 9 0 000-18zM15 9l-1.8 4.2L9 15l1.8-4.2z",
  wave: "M3 12c2 0 2-5 4-5s2 10 4 10 2-5 4-5",
  terminal: "M5 6l5 5-5 5M12 16h6",
  send: "M20 4L3 11l6 2 2 6z",
  copy: "M8 4H6a2 2 0 00-2 2v12a2 2 0 002 2h10a2 2 0 002-2v-2M8 4a2 2 0 012-2h4a2 2 0 012 2v0a2 2 0 01-2 2h-4a2 2 0 01-2-2zM8 10h8M8 14h4",
  pen: "M4 20l1-4L16 5a2 2 0 013 3L8 19l-4 1zM14 7l3 3",
  doc: "M6 3h8l5 5v13H6zM14 3v5h5M9.5 13h5M9.5 16.5h5",
  chip: "M7 7h10v10H7zM10 3v4M14 3v4M10 17v4M14 17v4M3 10h4M3 14h4M17 10h4M17 14h4",
  github: "M9 19c-4 1-4-2-6-2m12 4v-3.5a3 3 0 00-.8-2.3c2.7-.3 5.5-1.3 5.5-6a4.6 4.6 0 00-1.3-3.2 4.3 4.3 0 00-.1-3.2s-1-.3-3.4 1.3a11.600 11.600 0 00-6 0C6.500 3.200 5.500 3.500 5.500 3.500a4.300 4.300 0 00-.1 3.200A4.600 4.600 0 004 9.900c0 4.600 2.800 5.700 5.500 6A3 3 0 008.700 18.200V21",
};
const icon = (d) =>
  `<svg class="gcmd-ico" width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${d}"/></svg>`;

const jumpTo = (sel) => () => {
  document.querySelector(sel)?.scrollIntoView({ behavior: reduced ? "auto" : "smooth", block: "start" });
};
const openUrl = (u) => () => window.open(u, "_blank", "noopener,noreferrer");
const runCmd = (c) => () => {
  if (!term.open) {
    term.set(true);
    cmd("terminal", "open");
  }
  cmd("run", c);
};

const copyText = (text, said) => async () => {
  try {
    await navigator.clipboard.writeText(text);
    toast(said);
  } catch {}
};
const GROUPS = [
  {
    heading: "Effodio",
    items: [
      { v: "Demo video recorded tour app screen", label: "The recorded tour", ico: "layers", hint: "↓", run: jumpTo("#desk-stage") },
      { v: "Timeline plan roadmap macos gpu signed", label: "The plan", ico: "wave", hint: "↓", run: jumpTo(".lab-timeline") },
      { v: "What it does features photos video ocr filters", label: "What it does", ico: "compass", hint: "↓", run: jumpTo("#main-content") },
      { v: "Private privacy offline local", label: "Private", ico: "chip", hint: "↓", run: jumpTo("#private") },
      { v: "Install powershell one line command", label: "Install", ico: "terminal", hint: "↓", run: jumpTo("#install") },
    ],
  },
  {
    heading: "Actions",
    items: [
      { v: "Copy install command powershell irm iex", label: "Copy the install command", ico: "copy", run: copyText(COMMAND, "Install command copied") },
      { v: "Download installer exe setup", label: "Download the installer", ico: "doc", run: () => (location.href = INSTALLER) },
      { v: "Read install script ps1 source", label: "Read the install script", ico: "pen", run: openUrl("install.ps1") },
      { v: "Language Vietnamese English tieng viet", label: LANG === "vi" ? "English" : "Tiếng Việt", ico: "send", run: () => { try { localStorage.setItem("lang", LANG === "vi" ? "en" : "vi"); } catch {} location.href = location.pathname; } },
      { v: "Copy site URL link", label: "Copy site URL", ico: "copy", run: () => copyUrl() },
    ],
  },
  {
    heading: "Elsewhere",
    items: [
      { v: "Nhaajt lab home site author", label: "nhaajt.com", ico: "layers", hint: "↗", arrow: true, run: openUrl("https://www.nhaajt.com") },
    ],
  },
];

function editDistance(a, b) {
  const m = a.length;
  const n = b.length;
  let prev = Array.from({ length: n + 1 }, (_, i) => i);
  for (let i = 1; i <= m; i++) {
    const cur = [i];
    for (let j = 1; j <= n; j++) {
      cur[j] = Math.min(prev[j] + 1, cur[j - 1] + 1, prev[j - 1] + (a[i - 1] === b[j - 1] ? 0 : 1));
    }
    prev = cur;
  }
  return prev[n];
}
const typoAllowance = (w) => (w.length < 5 ? 0 : w.length < 8 ? 1 : 2);

function score(value, search) {
  const v = value.toLowerCase();
  const terms = search.toLowerCase().split(/\s+/).filter(Boolean);
  if (!terms.length) return 1;
  if (terms.every((t) => v.includes(t))) return v.startsWith(terms[0]) ? 1 : 0.5;
  const words = v.split(/[^a-z0-9]+/).filter(Boolean);
  return terms.every((t) => {
    const a = typoAllowance(t);
    return a > 0 && words.some((w) => editDistance(t, w) <= a);
  })
    ? 0.2
    : 0;
}

let toastTimer = 0;
function toast(msg) {
  document.querySelector(".gcmd-toast")?.remove();
  const t = el(
    "div",
    { class: "gcmd-toast", role: "status" },
    '<svg width="14" height="14" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="M4 12.5l5 5L20 6.5"/></svg>'
  );
  t.append(document.createTextNode(msg));
  document.body.append(t);
  clearTimeout(toastTimer);
  toastTimer = setTimeout(() => t.remove(), 2200);
}
async function copyUrl() {
  try {
    await navigator.clipboard.writeText(SITE_URL);
    toast(SITE_URL.replace(/^https?:\/\//, "") + " copied to clipboard");
  } catch {
    toast("Couldn't copy — clipboard is blocked");
  }
}

const palette = (() => {
  let overlay = null;
  let content = null;
  let opener = null;
  let items = [];
  let groups = [];
  let sel = -1;

  const visible = () => items.filter((i) => !i.node.hidden);
  const select = (i, scroll = true) => {
    const vis = visible();
    if (!vis.length) {
      sel = -1;
      return;
    }
    sel = (i + vis.length) % vis.length;
    items.forEach((it) => {
      it.node.setAttribute("data-selected", "false");
      it.node.setAttribute("aria-selected", "false");
    });
    const cur = vis[sel];
    cur.node.setAttribute("data-selected", "true");
    cur.node.setAttribute("aria-selected", "true");
    content.querySelector("input").setAttribute("aria-activedescendant", cur.node.id);
    if (scroll) cur.node.scrollIntoView({ block: "nearest" });
  };

  const filter = (q) => {
    const query = q.trim();
    groups.forEach((g) => {
      let best = 0;
      g.items.forEach((it) => {
        it.s = score(it.v, query);
        it.node.hidden = it.s === 0;
        best = Math.max(best, it.s);
      });
      g.best = best;
      g.node.hidden = best === 0;
      if (query) {
        [...g.items].sort((a, b) => b.s - a.s).forEach((it) => g.list.append(it.node));
      } else {
        g.items.forEach((it) => g.list.append(it.node));
      }
    });
    const host = content.querySelector(".gcmd-list");
    const ordered = query ? [...groups].sort((a, b) => b.best - a.best) : groups;
    ordered.forEach((g) => host.append(g.node));
    const none = !items.some((i) => !i.node.hidden);
    host.querySelector(".gcmd-empty").hidden = !none;
    host.append(host.querySelector(".gcmd-empty"));
    select(0, false);
    host.scrollTop = 0;
  };

  const run = (it) => {
    close(false);
    setTimeout(it.run, 0);
  };

  function build() {
    overlay = el("div", { class: "gcmd-overlay" });
    content = el("div", { class: "gcmd-content", role: "dialog", "aria-modal": "true", "aria-label": "Command menu" });
    const root = el("div", { class: "gcmd-root", "cmdk-root": "" });
    const search = el(
      "div",
      { class: "gcmd-search" },
      '<svg class="gcmd-search-ico" width="17" height="17" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" aria-hidden="true"><circle cx="11" cy="11" r="7"/><path d="M20 20l-3.5-3.5"/></svg>'
    );
    const input = el("input", {
      type: "text",
      placeholder: "Jump to a project, run a command, or ask a question…",
      autocomplete: "off",
      autocorrect: "off",
      spellcheck: "false",
      role: "combobox",
      "aria-expanded": "true",
      "aria-controls": "gcmd-list",
    });
    search.append(input);
    const list = el("div", { class: "gcmd-list", "cmdk-list": "", role: "listbox", id: "gcmd-list" });
    items = [];
    groups = GROUPS.map((g, gi) => {
      const gn = el("div", { class: "gcmd-group", role: "presentation" });
      gn.append(el("div", { class: "gcmd-group-heading", "cmdk-group-heading": "", "aria-hidden": "true" }, g.heading));
      const gl = el("div", { role: "group" });
      gn.append(gl);
      const gItems = g.items.map((d, ii) => {
        const n = el("div", { class: "gcmd-item", role: "option", id: `gcmd-${gi}-${ii}`, "data-selected": "false" });
        n.innerHTML =
          icon(ICONS[d.ico]) +
          `<span class="gcmd-label">${d.label}</span>` +
          (d.hint ? `<span class="gcmd-hint${d.arrow ? " gcmd-hint--arrow" : ""}">${d.hint}</span>` : "");
        const it = { node: n, v: d.v, run: d.run, s: 1 };
        n.addEventListener("pointermove", () => {
          const idx = visible().indexOf(it);
          if (idx !== sel) select(idx, false);
        });
        n.addEventListener("click", () => run(it));
        gl.append(n);
        items.push(it);
        return it;
      });
      list.append(gn);
      return { node: gn, list: gl, items: gItems, best: 1 };
    });
    list.append(el("div", { class: "gcmd-empty", hidden: "" }, "No results."));
    const footer = el(
      "div",
      { class: "gcmd-footer" },
      "<span><kbd>↑</kbd><kbd>↓</kbd> navigate</span><span><kbd>↵</kbd> select</span><span><kbd>esc</kbd> close</span>"
    );
    root.append(search, list, footer);
    content.append(root);

    input.addEventListener("input", () => filter(input.value));
    content.addEventListener("focusout", (e) => {
      if (!e.relatedTarget || !content.contains(e.relatedTarget)) {
        setTimeout(() => {
          if (overlay && content && !content.contains(document.activeElement)) input.focus();
        }, 0);
      }
    });
    content.addEventListener("pointerdown", (e) => {
      if (e.target !== input) e.preventDefault();
    });
    content.addEventListener("keydown", (e) => {
      const vis = visible();
      if (e.key === "ArrowDown" || (e.ctrlKey && (e.key === "n" || e.key === "j"))) {
        e.preventDefault();
        select(sel + 1);
      } else if (e.key === "ArrowUp" || (e.ctrlKey && e.key === "p")) {
        e.preventDefault();
        select(sel - 1);
      } else if (e.key === "Home") {
        e.preventDefault();
        select(0);
      } else if (e.key === "End") {
        e.preventDefault();
        select(vis.length - 1);
      } else if (e.key === "Enter" && vis[sel]) {
        e.preventDefault();
        run(vis[sel]);
      } else if (e.key === "Tab") {
        e.preventDefault();
      }
    });
    overlay.addEventListener("pointerdown", () => close());
  }

  function open() {
    if (overlay) return;
    opener = document.activeElement;
    build();
    document.body.append(overlay, content);
    filter("");
    content.querySelector("input").focus();
  }
  function close(restore = true) {
    if (!overlay) return;
    const o = overlay;
    const c = content;
    overlay = content = null;
    const done = () => {
      o.remove();
      c.remove();
    };
    if (reduced) done();
    else {
      o.classList.add("is-closing");
      c.classList.add("is-closing");
      setTimeout(done, 130);
    }
    if (restore && opener && opener.focus) opener.focus();
  }
  return {
    toggle: () => (overlay ? close() : open()),
    isOpen: () => !!overlay,
    close,
  };
})();

document.addEventListener("keydown", (e) => {
  if (!e.key) return;
  if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "k") {
    e.preventDefault();
    if (!e.repeat) palette.toggle();
  } else if (e.key === "Escape" && palette.isOpen()) {
    palette.close();
  } else if (e.key === "`" && !e.metaKey && !e.ctrlKey && !e.altKey && !e.repeat && !palette.isOpen()) {
    const t = e.target;
    if (t && (t.isContentEditable || /^(INPUT|TEXTAREA|SELECT)$/.test(t.tagName))) return;
    e.preventDefault();
    toggleTerminal();
  }
});

/* ---------- intro ---------- */
const LEDE = TEXT.lede;
const NOTE = TEXT.note;

function wordHtml(tok) {
  let out = "";
  const re = /\[\[(#|@)?([^\]|]+)(?:\|([^\]]+))?\]\]/g;
  let last = 0;
  let m;
  while ((m = re.exec(tok))) {
    out += esc(tok.slice(last, m.index));
    if (m[1] === "#") out += `<span class="lab-intro-key lab-intro-name">${esc(m[2])}</span>`;
    else if (m[1] === "@") out += `<a href="${m[3]}" target="_blank" rel="noopener noreferrer"><span class="lab-intro-key">${esc(m[2])}</span></a>`;
    else out += `<span class="lab-intro-key">${esc(m[2])}</span>`;
    last = re.lastIndex;
  }
  return out + esc(tok.slice(last));
}
const esc = (s) => s.replace(/&/g, "&amp;").replace(/</g, "&lt;");

function revealWords(p, text, delay) {
  const toks = text.split(/\s+(?![^\[]*\]\])/).filter(Boolean);
  toks.forEach((t, i) => {
    const w = el("span", { class: "lab-intro-w" }, wordHtml(t));
    w.style.setProperty("--d", delay + i * 45 + "ms");
    p.append(w);
    if (i < toks.length - 1) p.append(" ");
  });
  return delay + toks.length * 45 + 750;
}

const SIG_PATHS = [
  "M3 33 C5 25 7 14 9.500 7 C12 14 18 27 23 33 C25 26 27 16 29 8",
  "M37 32 C39 24 41 14 43 7 M55 8 C53 17 51 27 50 33 M38 21 C44 18 51 19 57 20",
  "M2 38 C14 40 34 40 61 35",
];

/* index.html ships the intro markup (the word reveal is a CSS animation, so it plays at first
   paint); this only adds the live clock and the terminal button behaviour. */
function hydrateIntro(root) {
  const meta = root.querySelector(".lab-intro-meta");
  const btn = root.querySelector("[data-intro-terminal]");
  if (btn) terminalButton(true, btn);
  if (meta) startClock(meta);
  pauseOffscreen(root);
}

function startClock(meta) {
  const tick = (() => {
    let lastMin = -1;
    let n = 0;
    return () => {
      const d = new Date();
      const t = d.toLocaleTimeString("en-US", {
        hour: "2-digit",
        minute: "2-digit",
        second: "2-digit",
        hour12: true,
      });
      const min = d.getMinutes();
      if (min !== lastMin) {
        lastMin = min;
        n++;
      }
      let time = meta.querySelector("time");
      if (!time) {
        meta.innerHTML =
          `<time class="intro-stamp"><span class="intro-stamp-time"></span><span class="intro-stamp-sep" aria-hidden="true">·</span><span class="intro-stamp-place">${TEXT.place}</span></time>`;
        time = meta.querySelector("time");
      }
      time.dateTime = d.toISOString();
      const ts = time.querySelector(".intro-stamp-time");
      if (ts.textContent !== t) {
        ts.textContent = t;
        if (ts.dataset.n !== String(n)) {
          ts.dataset.n = String(n);
          ts.style.animation = "none";
          void ts.offsetWidth;
          ts.style.animation = "";
        }
      }
    };
  })();
  tick();
  setInterval(tick, 1000);
}

function mountIntro() {
  const root = document.getElementById("intro-mount");
  if (!root) return;
  if (root.querySelector(".lab-intro-lede")) return hydrateIntro(root);
  const main = el("div", { class: "lab-intro-main" });
  main.append(el("h1", { class: "lab-intro-sr", id: "lab-intro-heading" }, "Effodio: describe it, find it"));
  root.setAttribute("aria-labelledby", "lab-intro-heading");
  const lede = el("p", { class: "lab-intro-lede" });
  const note = el("p", { class: "lab-intro-note" });
  const end = revealWords(lede, LEDE, 120);
  revealWords(note, NOTE, Math.max(300, end * 0.45));
  main.append(lede, note);

  const aside = el("aside", { class: "lab-intro-readout", "aria-label": "Effodio" });
  const meta = el("p", { class: "lab-intro-meta" });
  meta.innerHTML = '<span class="intro-stamp-placeholder" aria-hidden="true">···</span>';
  startClock(meta);

  const controls = el("div", { class: "lab-intro-controls" });
  const hi = el(
    "a",
    { class: "lab-intro-hi", href: INSTALLER },
    `<span class="lab-intro-hi-label">${TEXT.download}</span><span class="lab-intro-hi-arrow" aria-hidden="true"><svg viewBox="0 0 24 24" width="13" height="13" fill="none"><path d="M22.5 4.3 1.5 11.5l5.3 1.5z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/><path d="M22.5 4.3 6.8 13l2.1 6.8 1.4-5.5 8.5 3.4z" stroke="currentColor" stroke-width="1.8" stroke-linejoin="round"/></svg></span>`
  );
  controls.append(terminalButton(true), hi);

  const sig = el("div", { class: "lab-intro-signoff" });
  sig.innerHTML =
    '<svg width="67" height="42" viewBox="-2 2 67 42" fill="none" role="img" aria-label="Nhaajt">' +
    SIG_PATHS.map(
      (d, i) =>
        `<path d="${d}" pathLength="1" style="--i:${i}" stroke="currentColor" stroke-width="${i === 2 ? 1 : 1.6}" stroke-linecap="round" stroke-linejoin="round"/>`
    ).join("") +
    "</svg>";
  sig.style.setProperty("--sig-delay", Math.max(900, end * 0.6) + "ms");
  sig.classList.add("is-drawn");

  aside.append(meta, controls, sig);
  root.append(main, aside);
  pauseOffscreen(root);
}

/* ---------- spin cursor ---------- */
const MAC_ARROW = [
  "W.........",
  "WW........",
  "WKW.......",
  "WKKW......",
  "WKKKW.....",
  "WKKKKW....",
  "WKKKKKW...",
  "WKKKKKKW..",
  "WKKKKKKKW.",
  "WKKKKKWWW.",
  "WKKWKKW...",
  "WKWWKKW...",
  "WW..WKKW..",
  "....WKKW..",
  ".....WKKW.",
  ".....WKKW.",
  "......WW..",
];
const TIP = Math.atan2(-15, -12);
const angDiff = (a, b) => {
  let d = (b - a) % (2 * Math.PI);
  if (d > Math.PI) d -= 2 * Math.PI;
  if (d < -Math.PI) d += 2 * Math.PI;
  return d;
};
const TEXT_SEL =
  'input:not([type]), input[type="text"], input[type="search"], input[type="email"], input[type="url"], input[type="password"], input[type="number"], textarea, [contenteditable]:not([contenteditable="false"])';

function mountCursor() {
  if (!matchMedia("(pointer: fine)").matches || reduced) return;
  const html = document.documentElement;
  const node = el("div", { id: "spin-cursor", "aria-hidden": "true" });
  let rects = "";
  MAC_ARROW.forEach((row, y) =>
    [...row].forEach((c, x) => {
      if (c !== ".") rects += `<rect x="${x}" y="${y}" width="1" height="1" fill="${c === "K" ? "#000" : "#fff"}"/>`;
    })
  );
  const cols = MAC_ARROW[0].length;
  const rows = MAC_ARROW.length;
  node.innerHTML =
    '<svg class="sc-spin" width="18" height="21" viewBox="0 0 18 21"><path d="M0,0 L18,11 L9,13 L6,21 Z"/></svg>' +
    `<svg class="sc-mac" width="${2 * cols}" height="${2 * rows}" viewBox="0 0 ${cols} ${rows}" shape-rendering="crispEdges">${rects}</svg>`;
  (document.getElementById("cursor-mount") || document.body).append(node);

  let tx = -9999, ty = -9999; // pointer target
  let cx = -9999, cy = -9999; // eased position
  let vx = 0, vy = 0; // smoothed velocity
  let rot = 0;
  let down = 0, downT = false;
  let linkW = 0;
  let inside = false;
  let link = null;
  let hidden = false;
  let mac = false;
  let alpha = 0;
  let lastMove = performance.now();

  const onMove = (e) => {
    if (e.pointerType !== "mouse") return;
    tx = e.clientX;
    ty = e.clientY;
    lastMove = e.timeStamp;
    if (!inside) {
      cx = tx;
      cy = ty;
      vx = vy = 0;
    }
    inside = true;
    html.classList.add("spin-cursor-on");
    const t = e.target instanceof Element ? e.target : null;
    hidden = !!t?.closest(TEXT_SEL) || !!t?.closest(".tl-bar--link");
    mac = !!t?.closest('.gt-window[data-screen="mac"], .gt-radio, .gt-duo-fold');
    link = hidden || mac ? null : t?.closest("a, button, [role=button], input, textarea, select, summary, label") ?? null;
    node.classList.toggle("is-link", !!link);
    node.classList.toggle("is-mac", mac);
    kick();
  };
  const leave = () => {
    inside = false;
    html.classList.remove("spin-cursor-on");
    kick();
  };
  const onOver = (e) => {
    if (e.target instanceof HTMLIFrameElement) leave();
  };
  const onDown = (e) => {
    if (e.pointerType === "mouse") {
      downT = true;
      lastMove = e.timeStamp;
      kick();
    }
  };
  const onUp = (e) => {
    if (e.pointerType === "mouse") {
      downT = false;
      kick();
    }
  };

  /* runs on the shared scheduler; sleeps once the cursor has faded out (or is hidden) */
  const frame = (now, dtMs) => {
    const dt = Math.min(0.05, Math.max(0, dtMs / 1000));
    const px = cx;
    const py = cy;
    const k = 1 - Math.exp(-dt / 0.02);
    cx += (tx - cx) * k;
    cy += (ty - cy) * k;
    if (dt > 0) {
      const kv = 1 - Math.exp(-dt / 0.05);
      vx += ((cx - px) / dt - vx) * kv;
      vy += ((cy - py) / dt - vy) * kv;
    }
    const speed = Math.hypot(vx, vy);
    if (speed > 40) rot += angDiff(rot, Math.atan2(vy, vx) - TIP) * (1 - Math.exp(-dt / 0.06));
    else if (link) {
      const r = link.getBoundingClientRect();
      if (r.width <= 320 && r.height <= 120) {
        const dx = r.left + r.width / 2 - cx;
        const dy = r.top + r.height / 2 - cy;
        const a = Math.hypot(dx, dy) > 6 ? Math.atan2(dy, dx) - TIP : 0;
        rot += angDiff(rot, a) * (1 - Math.exp(-dt / 0.12));
      }
    }
    const stretch = 1 + Math.min(speed / 3000, 0.35);
    const squash = 1 / Math.sqrt(stretch);
    down += ((downT ? 1 : 0) - down) * (1 - Math.exp(-dt / 0.05));
    linkW += ((link ? 1 : 0) - linkW) * (1 - Math.exp(-dt / 0.05));
    alpha =
      !inside || hidden
        ? 0
        : now - lastMove > 2000
          ? alpha < 0.01
            ? 0
            : alpha * Math.exp(-dt / 0.13)
          : 1;
    node.style.opacity = String(alpha);
    if (mac) node.style.transform = `translate(${Math.round(tx)}px,${Math.round(ty)}px)`;
    else {
      const s = (31 / 21) * (1 - 0.2 * down) * (1 - 0.2 * linkW);
      node.style.transform = `translate(${cx}px,${cy}px) rotate(${rot}rad) scale(${s * squash},${s * stretch})`;
    }
  };
  const active = () => alpha > 0 || (inside && !hidden && performance.now() - lastMove <= 2000);

  addEventListener("pointermove", onMove, { passive: true });
  addEventListener("pointerover", onOver, { passive: true });
  addEventListener("pointerdown", onDown, { passive: true });
  addEventListener("pointerup", onUp, { passive: true });
  html.addEventListener("pointerleave", leave);
  onFrame(frame, { active });
}

mountHeader();
mountIntro();
mountCursor();
