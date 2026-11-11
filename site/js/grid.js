import { PROJECTS, TEXT } from './site-data.js';

const SVG_NS = 'http://www.w3.org/2000/svg';

function el(tag, cls, text) {
  const n = document.createElement(tag);
  if (cls) n.className = cls;
  if (text != null) n.textContent = text;
  return n;
}

function wheelIcon() {
  const s = document.createElementNS(SVG_NS, 'svg');
  const attrs = {
    class: 'garage-title-mark', width: '32', height: '32', viewBox: '0 0 32 32',
    fill: 'none', stroke: 'currentColor', 'stroke-width': '2', 'stroke-linecap': 'round', 'aria-hidden': 'true',
  };
  for (const k in attrs) s.setAttribute(k, attrs[k]);
  const c = document.createElementNS(SVG_NS, 'circle');
  c.setAttribute('cx', '16');
  c.setAttribute('cy', '16');
  c.setAttribute('r', '5');
  const p = document.createElementNS(SVG_NS, 'path');
  p.setAttribute('d', 'M16 3v5M16 24v5M3 16h5M24 16h5M6.8 6.8l3.5 3.5M21.7 21.7l3.5 3.5M25.2 6.8l-3.5 3.5M10.3 21.7l-3.5 3.5');
  s.append(c, p);
  return s;
}

function activeCell(p) {
  const a = el('article', 'garage-cell garage-cell--active');
  a.dataset.slug = p.slug;
  a.appendChild(el('span', 'garage-kicker', `${p.num} ${p.kind}`));
  const row = el('div', 'garage-title-row');
  if (p.icon) row.appendChild(wheelIcon());
  const h2 = el('h2', 'garage-title');
  const link = el('a', 'garage-cell-link', p.title);
  link.href = p.href;
  h2.appendChild(link);
  row.appendChild(h2);
  if (p.wip) {
    const chip = el('span', 'wip-chip');
    const dot = el('span', 'wip-dot');
    dot.setAttribute('aria-hidden', 'true');
    chip.append(dot, 'WIP');
    row.appendChild(chip);
  }
  a.appendChild(row);
  a.appendChild(el('p', 'garage-desc', p.desc));
  const foot = el('div', 'garage-cell-foot');
  const tags = el('div', 'garage-tags');
  p.tags.forEach((t) => tags.appendChild(el('span', 'garage-tag', t)));
  const arrow = el('span', 'garage-arrow', '→');
  arrow.setAttribute('aria-hidden', 'true');
  foot.append(tags, arrow);
  a.appendChild(foot);
  return a;
}

const SOON_MODES = { lg: { cols: 44, rows: 21 }, sm: { cols: 25, rows: 12 } };
const SOON_HUE = 28;
const SOON_TICK = 86400000 / 2592000;
const SOON_MIN_L = 7;
const SOON_LEVELS = 94;

function soonCanvas(host) {
  const canvas = el('canvas', 'garage-soon-canvas');
  canvas.setAttribute('aria-hidden', 'true');
  host.appendChild(canvas);
  const ctx = canvas.getContext('2d');
  const mq = window.matchMedia('(max-width: 860px)');
  const still = window.matchMedia('(prefers-reduced-motion: reduce)');
  const atlas = document.createElement('canvas');
  const actx = atlas.getContext('2d');

  let cols = 0, rows = 0, n = 0, live = 0;
  let cssW = 0, cssH = 0, dpr = 1, S = 1;
  let sw = 0, sh = 0, pad = 2, baseY = 0;
  let font = '';
  let dx = new Float32Array(0), dy = new Float32Array(0), rr = new Float32Array(0);
  let hueOf = new Float32Array(0);
  let order = new Int32Array(0);
  let slot = new Uint8Array(0), built = new Uint8Array(0);
  let running = false, raf = 0, timer = 0, last = 0, acc = 0, lastStep = -1;

  function geometry() {
    const cs = getComputedStyle(canvas);
    const size = parseFloat(cs.fontSize) || 10;
    const gap = parseFloat(cs.getPropertyValue('--soon-gap')) || 4;
    const scale = parseFloat(cs.getPropertyValue('--soon-scale')) || 1.5;
    font = `${cs.fontStyle} ${cs.fontWeight} ${size}px ${cs.fontFamily}`;
    const m = SOON_MODES[mq.matches ? 'sm' : 'lg'];
    cols = m.cols; rows = m.rows; n = cols * rows;
    dpr = window.devicePixelRatio || 1;
    S = dpr * scale;

    actx.font = font;
    const cw = actx.measureText('?').width;
    const asc = Math.round(actx.measureText('?').fontBoundingBoxAscent);
    const desc = Math.round(actx.measureText('?').fontBoundingBoxDescent);
    const baseline = Math.floor((size - (asc + desc)) / 2) + asc;

    sw = Math.ceil(cw * S) + pad * 2;
    sh = Math.ceil(size * S) + pad * 2;
    baseY = baseline;

    const gw = cols * cw + (cols - 1) * gap;
    const gh = rows * size;
    // the grid track never shrinks below its content, so an overflowing grid starts at the edge
    const gcx = Math.max(0, (cssW - gw) / 2) + gw / 2;
    const gcy = Math.max(0, (cssH - gh) / 2) + gh / 2;
    dx = new Float32Array(n); dy = new Float32Array(n); rr = new Float32Array(n);
    slot = new Uint8Array(n);
    order = new Int32Array(n);
    live = 0;
    const ids = new Int16Array(cols * 2 + 1).fill(-1);
    let used = 0;
    for (let r = 0; r < rows; r++) {
      for (let c = 0; c < cols; c++) {
        const i = r * cols + c;
        const ox = (c + 1) / cols - 0.5;
        const oy = (rows - r) / rows - 0.5;
        const k = Math.abs(2 * (c + 1) - cols);
        if (ids[k] < 0) ids[k] = used++;
        slot[i] = ids[k];
        rr[i] = Math.sqrt(ox * ox + oy * oy);
        dx[i] = Math.round((gcx + (c * (cw + gap) - gw / 2) * scale) * dpr) - pad;
        dy[i] = Math.round((gcy + (r * size - gh / 2) * scale) * dpr) - pad;
        // the radial mask is fully transparent past 78% of the farthest corner: skip those cells
        const mx = (dx[i] + pad + (cw * S) / 2) / dpr - cssW / 2;
        const my = (dy[i] + pad + (size * S) / 2) / dpr - cssH / 2;
        const md = Math.hypot(mx / (cssW / 2), my / (cssH / 2)) / Math.SQRT2;
        if (md < 0.78 + 0.04) order[live++] = i;
      }
    }
    atlas.width = used * sw;
    atlas.height = SOON_LEVELS * sh;
    built = new Uint8Array(used * SOON_LEVELS);
    // hue per slot: slot ids are assigned in column order, so store the k that owns each
    hueOf = new Float32Array(used);
    for (let k = 0; k < ids.length; k++) if (ids[k] >= 0) hueOf[ids[k]] = SOON_HUE + (k / (2 * cols)) * 42;
  }
  function sprite(s, li) {
    const idx = s * SOON_LEVELS + li;
    if (built[idx]) return;
    built[idx] = 1;
    actx.setTransform(1, 0, 0, 1, 0, 0);
    actx.clearRect(s * sw, li * sh, sw, sh);
    actx.save();
    actx.beginPath();
    actx.rect(s * sw, li * sh, sw, sh);
    actx.clip();
    actx.setTransform(S, 0, 0, S, s * sw + pad, li * sh + pad);
    actx.font = font;
    actx.textBaseline = 'alphabetic';
    actx.fillStyle = `hsl(${hueOf[s]}, 88%, ${li + SOON_MIN_L}%)`;
    actx.fillText('?', 0, baseY);
    actx.restore();
  }

  function draw(t) {
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    const ph = t / 120;
    for (let j = 0; j < live; j++) {
      const i = order[j];
      let l = Math.sin(rr[i] * 36 - ph) * 100;
      if (l < SOON_MIN_L) l = SOON_MIN_L;
      const li = Math.round(l) - SOON_MIN_L;
      const s = slot[i];
      sprite(s, li);
      ctx.drawImage(atlas, s * sw, li * sh, sw, sh, dx[i], dy[i], sw, sh);
    }
  }

  function resize() {
    const w = host.clientWidth, h = host.clientHeight;
    if (!w || !h) return;
    cssW = w; cssH = h;
    geometry();
    canvas.width = Math.round(w * dpr);
    canvas.height = Math.round(h * dpr);
    lastStep = -1;
    draw(still.matches ? 0 : Math.round(((acc / SOON_TICK) | 0) * SOON_TICK));
  }

  function frame(now) {
    raf = 0;
    if (!running) return;
    if (last >= 0) acc += now - last;
    last = now;
    const step = Math.floor(acc / SOON_TICK);
    if (step !== lastStep) {
      lastStep = step;
      draw(Math.round(step * SOON_TICK) % 86400000);
    }
    const wait = (step + 1) * SOON_TICK - acc - 9;
    timer = setTimeout(wake, wait > 0 ? wait : 0);
  }

  function wake() {
    timer = 0;
    if (running) raf = requestAnimationFrame(frame);
  }

  function setRunning(on) {
    on = on && !still.matches;
    if (on === running) return;
    running = on;
    if (on) {
      last = -1;
      raf = requestAnimationFrame(frame);
    } else {
      if (raf) cancelAnimationFrame(raf);
      if (timer) clearTimeout(timer);
      raf = timer = 0;
    }
  }

  if ('ResizeObserver' in window) new ResizeObserver(resize).observe(host);
  mq.addEventListener('change', resize);
  still.addEventListener('change', resize);
  if (document.fonts) document.fonts.ready.then(() => { built.fill(0); resize(); });
  resize();
  return { setRunning };
}

function soonCell() {
  const cell = el('div', 'garage-cell garage-cell--soon');
  const shader = el('div', 'garage-soon-shader');
  shader.setAttribute('aria-hidden', 'true');
  const view = soonCanvas(shader);
  cell.appendChild(shader);
  cell.appendChild(el('span', 'garage-slot-num', '05'));
  const rule = el('span', 'garage-slot-rule');
  rule.setAttribute('aria-hidden', 'true');
  cell.appendChild(rule);
  cell.appendChild(el('p', 'garage-slot-text', TEXT.soon));

  let inView = true;
  let idle = false;
  const apply = () => {
    const paused = idle || !inView || document.hidden;
    shader.classList.toggle('is-shader-paused', paused);
    view.setRunning(!paused);
  };
  setTimeout(() => { idle = true; apply(); }, 60000);
  if ('IntersectionObserver' in window) {
    new IntersectionObserver(([e]) => { inView = e.isIntersecting; apply(); }, { rootMargin: '120px' }).observe(shader);
  }
  document.addEventListener('visibilitychange', apply);
  apply();
  return cell;
}

/* the grid wears in: projects people walk to most move forward and set heavier, the rest thin out */
function wearIn(mount) {
  const links = window.__world?.state.links || {};
  const cells = [...mount.querySelectorAll('.garage-cell--active')];
  const max = Math.max(0, ...cells.map((c) => links[c.dataset.slug] || 0));
  if (max <= 0) return;
  /* layout position plus whatever glide is still running, so a reorder in mid-air carries on from where the card is */
  const pos = (c) => {
    const m = new DOMMatrix(getComputedStyle(c).transform);
    return [c.offsetLeft + m.m41, c.offsetTop + m.m42];
  };
  const before = new Map(cells.map((c) => [c, pos(c)]));
  mount.dataset.worn = '1';
  cells.forEach((c) => c.style.setProperty('--wear', ((links[c.dataset.slug] || 0) / max).toFixed(2)));
  const sorted = [...cells].sort((x, y) => (links[y.dataset.slug] || 0) - (links[x.dataset.slug] || 0));
  if (sorted.every((c, i) => c === cells[i])) return;
  const anchor = mount.querySelector('.garage-cell:not(.garage-cell--active)');
  sorted.forEach((c) => mount.insertBefore(c, anchor));
  if (matchMedia('(prefers-reduced-motion: reduce)').matches) return;
  for (const c of sorted) {
    const a = before.get(c), b = [c.offsetLeft, c.offsetTop];
    const dx = a[0] - b[0], dy = a[1] - b[1];
    c.getAnimations().forEach((x) => x.cancel());
    if (Math.abs(dx) + Math.abs(dy) > 0.5) c.animate([{ transform: `translate(${dx}px,${dy}px)` }, { transform: 'none' }], { duration: 700, easing: 'cubic-bezier(.2,.7,.2,1)' });
  }
}

function init() {
  const mount = document.getElementById('main-content');
  if (!mount) return;
  mount.textContent = '';
  PROJECTS.forEach((p) => mount.appendChild(activeCell(p)));
  mount.appendChild(soonCell());
  const v = el('div', 'garage-cell garage-cell--void');
  v.setAttribute('aria-hidden', 'true');
  mount.appendChild(v);
  addEventListener('lab:world', (e) => { if (e.detail.what === 'links') wearIn(mount); });
  wearIn(mount);
}

init();
