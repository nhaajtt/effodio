(() => {
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const fine = matchMedia("(hover: hover) and (pointer: fine)").matches;
  const root = document.documentElement;
  const pointer = { x: -9999, y: -9999 };
  const TURN = 6.283;

  // The headline, a letter at a time (again when the language changes).
  function letters() {
    const line = document.querySelector('[data-t="h1a"]');
    const text = line.textContent;
    line.textContent = "";
    [...text].forEach((letter, index) => {
      const span = document.createElement("span");
      span.className = "ch";
      span.textContent = letter === " " ? " " : letter;
      span.style.animationDelay = index * 45 + "ms";
      line.append(span);
    });
  }
  if (!still) {
    letters();
    document.addEventListener("glint:lang", letters);
    document.querySelectorAll(".cmd, .panel").forEach((pane) => {
      const rim = document.createElement("i");
      rim.className = "rim";
      pane.append(rim);
    });
  }

  // One glint: a four-pointed star that flies, turns and goes out.
  let alive = 0;
  function glint(x, y, { far = 0, fall = 0, wide = 12, time = 800, cool = false } = {}) {
    if (alive > 90) return;
    alive++;
    const spark = document.createElement("i");
    spark.className = cool ? "spark cool" : "spark";
    const angle = Math.random() * TURN;
    const dx = Math.cos(angle) * far, dy = Math.sin(angle) * far + fall;
    spark.style.cssText = `left:${x}px;top:${y}px;width:${wide}px;height:${wide}px`;
    document.body.append(spark);
    const turn = Math.random() * 260 - 130;
    spark.animate(
      [
        { transform: "translate(-50%,-50%) scale(0) rotate(0deg)", opacity: 1 },
        { transform: `translate(calc(-50% + ${dx * 0.6}px),calc(-50% + ${dy * 0.6}px)) scale(1) rotate(${turn * 0.6}deg)`, opacity: 1, offset: 0.35 },
        { transform: `translate(calc(-50% + ${dx}px),calc(-50% + ${dy}px)) scale(0) rotate(${turn}deg)`, opacity: 0 },
      ],
      { duration: time, easing: "cubic-bezier(0.2,0.8,0.3,1)" },
    ).onfinish = () => {
      spark.remove();
      alive--;
    };
  }
  const burst = (x, y, count) => {
    for (let index = 0; index < count; index++) glint(x, y, { far: 40 + Math.random() * 100, wide: 7 + Math.random() * 16, time: 650 + Math.random() * 600, cool: index % 3 === 0 });
  };

  // The field: every point a file, gathered by what it's about. Points shy from the pointer and spring back,
  // twinkle on their own, and ride the ring a click sends out. A search lights one gathering.
  const canvas = document.getElementById("stars");
  const ctx = canvas.getContext("2d");
  const colours = ["217,166,58", "63,107,216", "138,107,240", "47,179,160"];
  let width = 0, height = 0, points = [], lit = -1, litAt = 0, rings = [], comet = null, nextComet = 2500;
  function size() {
    const ratio = Math.min(devicePixelRatio || 1, 2);
    width = innerWidth;
    height = innerHeight;
    canvas.width = width * ratio;
    canvas.height = height * ratio;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    const count = Math.round(Math.min(170, (width * height) / 8500));
    const centres = [[0.16, 0.24], [0.84, 0.3], [0.24, 0.8], [0.8, 0.78]];
    points = Array.from({ length: count }, (_, index) => {
      const group = index % 4;
      const angle = Math.random() * TURN;
      const reach = Math.sqrt(Math.random()) * Math.min(width, height) * 0.32;
      return {
        group,
        hx: centres[group][0] * width + Math.cos(angle) * reach * 1.25,
        hy: centres[group][1] * height + Math.sin(angle) * reach,
        x: 0, y: 0, ox: 0, oy: 0, vx: 0, vy: 0,
        size: 0.8 + Math.random() * 1.8,
        phase: Math.random() * TURN,
        pace: 0.15 + Math.random() * 0.3,
        sway: 8 + Math.random() * 22,
        blink: 0.5 + Math.random() * 1.1,
      };
    });
  }
  function star(x, y, reach, thin) {
    ctx.beginPath();
    for (let corner = 0; corner < 8; corner++) {
      const far = corner % 2 ? reach * thin : reach;
      const angle = (corner / 8) * TURN - TURN / 4;
      ctx.lineTo(x + Math.cos(angle) * far, y + Math.sin(angle) * far);
    }
    ctx.closePath();
    ctx.fill();
  }
  function draw(now) {
    const time = now / 1000;
    const light = root.dataset.theme === "light";
    const white = light ? "120,90,20" : "255,255,255";
    ctx.clearRect(0, 0, width, height);
    const glow = lit < 0 ? 0 : Math.max(0, 1 - (now - litAt) / 2200);
    const panel = document.getElementById("demo").getBoundingClientRect();
    const goalX = panel.left + panel.width / 2, goalY = panel.top + 30;

    rings = rings.filter((ring) => now - ring.at < 1500);
    for (const ring of rings) {
      const age = (now - ring.at) / 1500;
      ring.reach = age * Math.max(width, height) * 0.75;
      for (const [offset, alpha] of [[0, 0.5], [-26, 0.22]]) {
        if (ring.reach + offset <= 0) continue;
        ctx.strokeStyle = `rgba(244,213,138,${(1 - age) * alpha})`;
        ctx.lineWidth = 1.5;
        ctx.beginPath();
        ctx.arc(ring.x, ring.y, ring.reach + offset, 0, TURN);
        ctx.stroke();
      }
    }

    for (const point of points) {
      const bx = point.hx + Math.cos(time * point.pace + point.phase) * point.sway;
      const by = point.hy + Math.sin(time * point.pace * 1.3 + point.phase) * point.sway;
      let dx = bx + point.ox - pointer.x, dy = by + point.oy - pointer.y;
      let far = Math.hypot(dx, dy) || 1;
      if (far < 150) {
        const push = (1 - far / 150) * 1.5;
        point.vx += (dx / far) * push;
        point.vy += (dy / far) * push;
      }
      for (const ring of rings) {
        dx = bx + point.ox - ring.x;
        dy = by + point.oy - ring.y;
        far = Math.hypot(dx, dy) || 1;
        if (Math.abs(far - ring.reach) < 34) {
          const push = (1 - (now - ring.at) / 1500) * 1.6;
          point.vx += (dx / far) * push;
          point.vy += (dy / far) * push;
        }
      }
      point.vx = (point.vx - point.ox * 0.028) * 0.9;
      point.vy = (point.vy - point.oy * 0.028) * 0.9;
      point.ox += point.vx;
      point.oy += point.vy;
      point.x = bx + point.ox;
      point.y = by + point.oy;
    }

    ctx.lineWidth = 1;
    for (let a = 0; a < points.length; a++) {
      const one = points[a];
      for (let b = a + 1; b < points.length; b++) {
        const other = points[b];
        if (other.group !== one.group) continue;
        const dx = one.x - other.x, dy = one.y - other.y;
        const far = dx * dx + dy * dy;
        if (far > 8100) continue;
        const boost = one.group === lit ? 1 + glow * 3 : 1;
        ctx.strokeStyle = `rgba(${colours[one.group]},${(1 - far / 8100) * 0.18 * boost})`;
        ctx.beginPath();
        ctx.moveTo(one.x, one.y);
        ctx.lineTo(other.x, other.y);
        ctx.stroke();
      }
    }
    for (const point of points) {
      const near = Math.max(0, 1 - Math.hypot(point.x - pointer.x, point.y - pointer.y) / 190);
      const flare = point.group === lit ? glow : 0;
      if (near > 0) {
        ctx.strokeStyle = `rgba(217,166,58,${near * 0.5})`;
        ctx.beginPath();
        ctx.moveTo(point.x, point.y);
        ctx.lineTo(pointer.x, pointer.y);
        ctx.stroke();
      }
      if (flare > 0 && panel.bottom > 0) {
        ctx.strokeStyle = `rgba(${colours[point.group]},${Math.sin(flare * Math.PI) * 0.3})`;
        ctx.beginPath();
        ctx.moveTo(point.x, point.y);
        ctx.lineTo(point.x + (goalX - point.x) * (1 - flare), point.y + (goalY - point.y) * (1 - flare));
        ctx.stroke();
      }
      ctx.fillStyle = `rgba(${colours[point.group]},${Math.min(1, 0.55 + near * 0.45 + flare * 0.45)})`;
      ctx.beginPath();
      ctx.arc(point.x, point.y, point.size * (1 + near * 1.6 + flare * 1.4), 0, TURN);
      ctx.fill();
      // Now and then a point flashes into a star.
      const wink = Math.sin(time * point.blink + point.phase * 3);
      const shine = Math.max(wink > 0.9 ? (wink - 0.9) * 10 : 0, near * 0.8, flare * 0.8);
      if (shine > 0.02) {
        ctx.fillStyle = `rgba(${white},${shine * 0.9})`;
        star(point.x, point.y, point.size * (3 + shine * 6), 0.14);
      }
    }

    // A streak of light crosses the sky from time to time.
    if (!comet && now > nextComet) {
      const fromLeft = Math.random() < 0.5;
      comet = { x: fromLeft ? -40 : width + 40, y: Math.random() * height * 0.5, vx: (fromLeft ? 1 : -1) * (9 + Math.random() * 5), vy: 3 + Math.random() * 3 };
    }
    if (comet) {
      comet.x += comet.vx;
      comet.y += comet.vy;
      const tail = ctx.createLinearGradient(comet.x, comet.y, comet.x - comet.vx * 16, comet.y - comet.vy * 16);
      tail.addColorStop(0, `rgba(${white},0.9)`);
      tail.addColorStop(0.3, "rgba(244,213,138,0.5)");
      tail.addColorStop(1, "rgba(155,123,245,0)");
      ctx.strokeStyle = tail;
      ctx.lineWidth = 2;
      ctx.beginPath();
      ctx.moveTo(comet.x, comet.y);
      ctx.lineTo(comet.x - comet.vx * 16, comet.y - comet.vy * 16);
      ctx.stroke();
      ctx.fillStyle = `rgba(${white},0.95)`;
      star(comet.x, comet.y, 9, 0.16);
      if (comet.x < -200 || comet.x > width + 200 || comet.y > height + 200) {
        comet = null;
        nextComet = now + 3500 + Math.random() * 4500;
      }
    }
  }
  size();
  addEventListener("resize", size);
  document.addEventListener("glint:search", () => {
    lit = (lit + 1) % 4;
    litAt = performance.now();
    if (still || document.hidden) return;
    const icon = document.querySelector(".ask svg").getBoundingClientRect();
    if (icon.bottom > 0 && icon.top < innerHeight) for (let index = 0; index < 5; index++) glint(icon.left + 10, icon.top + 10, { far: 18 + Math.random() * 26, wide: 6 + Math.random() * 8, time: 700 });
  });

  // The pointer sheds glints as it goes.
  let lastX = 0, lastY = 0;
  if (fine) {
    addEventListener("pointermove", (event) => {
      pointer.x = event.clientX;
      pointer.y = event.clientY;
      if (still || Math.hypot(pointer.x - lastX, pointer.y - lastY) < 26) return;
      lastX = pointer.x;
      lastY = pointer.y;
      glint(pointer.x + Math.random() * 16 - 8, pointer.y + Math.random() * 16 - 8, { far: 10, fall: 26 + Math.random() * 30, wide: 5 + Math.random() * 9, time: 600 + Math.random() * 500, cool: Math.random() < 0.3 });
    });
    document.addEventListener("pointerleave", () => (pointer.x = pointer.y = -9999));
  }

  // A press sends a ring through the field, and a burst from buttons.
  if (!still) {
    document.addEventListener("pointerdown", (event) => {
      rings.push({ x: event.clientX, y: event.clientY, at: performance.now(), reach: 0 });
      burst(event.clientX, event.clientY, event.target.closest("button, a") ? 16 : 6);
    });
    // The headline glitters.
    setInterval(() => {
      if (document.hidden) return;
      const box = document.querySelector("h1").getBoundingClientRect();
      if (box.bottom < 0 || box.top > innerHeight) return;
      glint(box.left + Math.random() * box.width, box.top + Math.random() * box.height, { wide: 8 + Math.random() * 18, time: 900 + Math.random() * 500, cool: Math.random() < 0.35 });
    }, 300);
  }

  // The tape of searches.
  const phrases = ["a dog on the beach", "the clause about pets", "hoa don tien dien", "PAY-7731-DX", "zebra in a video", "sunset over the bay", "invoice from March", "the slide about pricing"];
  const runs = [...document.querySelectorAll(".run")].map((run, row) => {
    const order = row ? [...phrases].reverse() : phrases;
    const text = order.map((phrase) => `<span>${phrase}</span><b>✦</b>`).join("");
    run.innerHTML = text + text;
    return { run, at: 0, way: row ? 1 : -1 };
  });
  let lastScroll = scrollY, speed = 0;

  function frame(now) {
    if (!document.hidden) {
      draw(now);
      speed += (scrollY - lastScroll - speed) * 0.2;
      lastScroll = scrollY;
      for (const row of runs) {
        const half = row.run.scrollWidth / 2;
        row.at += row.way * (0.5 + Math.abs(speed) * 0.6);
        if (row.at <= -half) row.at += half;
        if (row.at > 0) row.at -= half;
        row.run.style.transform = `translate3d(${row.at}px,0,0) skewX(${Math.max(-14, Math.min(14, -speed * row.way * 0.5))}deg)`;
      }
    }
    requestAnimationFrame(frame);
  }
  if (still) draw(0);
  else requestAnimationFrame(frame);

  if (fine && !still) {
    // Glass tilts toward the pointer.
    document.querySelectorAll(".card, .panel, .facts").forEach((glass) => {
      const most = glass.classList.contains("panel") ? 5 : 10;
      glass.addEventListener("pointermove", (event) => {
        const box = glass.getBoundingClientRect();
        const across = (event.clientX - box.left) / box.width - 0.5;
        const down = (event.clientY - box.top) / box.height - 0.5;
        glass.style.transform = `perspective(900px) rotateX(${-down * most}deg) rotateY(${across * most}deg) translateY(-4px)`;
      });
      glass.addEventListener("pointerleave", () => (glass.style.transform = ""));
    });
    // Buttons lean toward the pointer.
    document.querySelectorAll(".btn, .chip").forEach((button) => {
      button.addEventListener("pointermove", (event) => {
        const box = button.getBoundingClientRect();
        button.style.translate = `${(event.clientX - box.left - box.width / 2) * 0.3}px ${(event.clientY - box.top - box.height / 2) * 0.4}px`;
      });
      button.addEventListener("pointerleave", () => (button.style.translate = ""));
    });
  }

  // Light and dark through an iris.
  const theme = document.getElementById("theme");
  const flip = theme.onclick;
  theme.onclick = (event) => {
    if (still || !document.startViewTransition) return flip();
    root.style.setProperty("--vx", event.clientX + "px");
    root.style.setProperty("--vy", event.clientY + "px");
    document.startViewTransition(flip);
  };
})();
