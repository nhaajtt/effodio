/* Shared animation-frame scheduler.
   One requestAnimationFrame drives every subscriber. A subscriber runs only while its
   `active()` test passes, its element (if given) is on screen and the tab is visible;
   when nothing is active the loop stops completely until kick() is called. */

const subs = new Set();
let raf = 0;

const isLive = (s) => s.visible && s.active();

function loop(now) {
  raf = 0;
  if (document.hidden) return;
  let any = false;
  for (const s of subs) {
    if (!isLive(s)) {
      s.last = 0;
      continue;
    }
    const dt = s.last ? Math.min(250, now - s.last) : 1000 / 60;
    s.last = now;
    try {
      s.fn(now, dt);
    } catch (e) {
      console.error(e);
    }
  }
  for (const s of subs) if (isLive(s)) { any = true; break; }
  if (any) raf = requestAnimationFrame(loop);
}

/** Wake the loop (call after input or any state change that may make a subscriber active). */
export function kick() {
  if (!raf && !document.hidden) raf = requestAnimationFrame(loop);
}

/* one shared IntersectionObserver for visibility gating */
const watchers = new Map();
let io = null;
function observer() {
  if (!io && 'IntersectionObserver' in window) {
    io = new IntersectionObserver((entries) => {
      for (const e of entries) {
        const set = watchers.get(e.target);
        if (set) for (const cb of set) cb(e.isIntersecting);
      }
    }, { rootMargin: '100px 0px' });
  }
  return io;
}

/** Call cb(onScreen) whenever `el` enters or leaves the viewport (100px margin). Returns an unwatch function. */
export function watchVisible(el, cb) {
  const o = observer();
  if (!o) { cb(true); return () => {}; }
  let set = watchers.get(el);
  if (!set) {
    set = new Set();
    watchers.set(el, set);
    o.observe(el);
  }
  set.add(cb);
  return () => {
    set.delete(cb);
    if (!set.size) {
      watchers.delete(el);
      o.unobserve(el);
    }
  };
}

/** Toggle the `is-offscreen` class on `el` so its CSS animations pause while it is off screen. */
export function pauseOffscreen(el) {
  return watchVisible(el, (on) => el.classList.toggle('is-offscreen', !on));
}

/**
 * Run fn(now, dtMs) every frame while active() is true and `el` (optional) is on screen.
 * Returns { stop, kick }.
 */
export function onFrame(fn, { active = () => true, el = null } = {}) {
  const s = { fn, active, visible: true, last: 0 };
  subs.add(s);
  let unwatch = null;
  if (el) {
    s.visible = false;
    unwatch = watchVisible(el, (on) => {
      s.visible = on;
      if (on) kick();
    });
  }
  kick();
  return {
    stop() {
      subs.delete(s);
      if (unwatch) unwatch();
    },
    kick,
  };
}

document.addEventListener('visibilitychange', () => {
  if (!document.hidden) kick();
});
