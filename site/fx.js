(() => {
  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const fine = matchMedia("(hover: hover) and (pointer: fine)").matches;
  const root = document.documentElement;
  const pointer = { x: -9999, y: -9999 };

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
  }

  // The field: every point a file, gathered by what it's about. A search lights one gathering and draws it to
  // the panel; the pointer lights whatever it passes.
  const canvas = document.getElementById("stars");
  const ctx = canvas.getContext("2d");
  const colours = ["217,166,58", "63,107,216", "138,107,240", "47,179,160"];
  let width = 0, height = 0, points = [], lit = -1, litAt = 0;
  function size() {
    const ratio = Math.min(devicePixelRatio || 1, 2);
    width = innerWidth;
    height = innerHeight;
    canvas.width = width * ratio;
    canvas.height = height * ratio;
    ctx.setTransform(ratio, 0, 0, ratio, 0, 0);
    const count = Math.round(Math.min(150, (width * height) / 9500));
    const centres = [[0.16, 0.24], [0.84, 0.3], [0.24, 0.8], [0.8, 0.78]];
    points = Array.from({ length: count }, (_, index) => {
      const group = index % 4;
      const angle = Math.random() * 6.283;
      const reach = Math.sqrt(Math.random()) * Math.min(width, height) * 0.3;
      return {
        group,
        hx: centres[group][0] * width + Math.cos(angle) * reach * 1.25,
        hy: centres[group][1] * height + Math.sin(angle) * reach,
        x: 0, y: 0,
        size: 0.8 + Math.random() * 1.7,
        phase: Math.random() * 6.283,
        pace: 0.15 + Math.random() * 0.3,
        sway: 8 + Math.random() * 22,
      };
    });
  }
  function draw(now) {
    const time = now / 1000;
    const light = root.dataset.theme === "light";
    const base = light ? 0.55 : 0.5;
    ctx.clearRect(0, 0, width, height);
    const glow = lit < 0 ? 0 : Math.max(0, 1 - (now - litAt) / 2200);
    const panel = document.getElementById("demo").getBoundingClientRect();
    const goalX = panel.left + panel.width / 2, goalY = panel.top + 30;
    for (const point of points) {
      point.x = point.hx + Math.cos(time * point.pace + point.phase) * point.sway;
      point.y = point.hy + Math.sin(time * point.pace * 1.3 + point.phase) * point.sway;
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
        ctx.strokeStyle = `rgba(${colours[one.group]},${(1 - far / 8100) * 0.16 * boost})`;
        ctx.beginPath();
        ctx.moveTo(one.x, one.y);
        ctx.lineTo(other.x, other.y);
        ctx.stroke();
      }
    }
    for (const point of points) {
      const dx = point.x - pointer.x, dy = point.y - pointer.y;
      const near = Math.max(0, 1 - Math.hypot(dx, dy) / 170);
      const flare = point.group === lit ? glow : 0;
      if (near > 0) {
        ctx.strokeStyle = `rgba(217,166,58,${near * 0.55})`;
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
      ctx.fillStyle = `rgba(${colours[point.group]},${Math.min(1, base + near * 0.5 + flare * 0.5)})`;
      ctx.beginPath();
      ctx.arc(point.x, point.y, point.size * (1 + near * 1.6 + flare * 1.4), 0, 6.283);
      ctx.fill();
    }
  }
  size();
  addEventListener("resize", size);
  document.addEventListener("glint:search", () => {
    lit = (lit + 1) % 4;
    litAt = performance.now();
  });

  // The lens.
  const lens = document.getElementById("lens");
  const at = { x: innerWidth / 2, y: innerHeight / 2, scale: 1, goal: 1 };
  if (fine) {
    addEventListener("pointermove", (event) => {
      pointer.x = event.clientX;
      pointer.y = event.clientY;
      at.goal = event.target.closest?.("a, button") ? 0.42 : event.target.closest?.("p, code, li, small") ? 0.7 : 1;
      lens.classList.add("on");
    });
    addEventListener("pointerdown", () => (at.scale *= 0.7));
    document.addEventListener("pointerleave", () => {
      lens.classList.remove("on");
      pointer.x = pointer.y = -9999;
    });
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
      at.x += (pointer.x - at.x) * 0.17;
      at.y += (pointer.y - at.y) * 0.17;
      at.scale += (at.goal - at.scale) * 0.14;
      lens.style.transform = `translate3d(${at.x}px,${at.y}px,0) translate(-50%,-50%) scale(${at.scale})`;
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

  // Glass tilts toward the pointer.
  if (fine && !still) {
    document.querySelectorAll(".card, .panel, .facts").forEach((glass) => {
      const most = glass.classList.contains("panel") ? 5 : 9;
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

  // A burst of glints where something is pressed.
  function burst(x, y) {
    for (let index = 0; index < 14; index++) {
      const spark = document.createElement("i");
      spark.className = "spark";
      const angle = Math.random() * 6.283, far = 36 + Math.random() * 90, wide = 7 + Math.random() * 15;
      spark.style.cssText = `left:${x}px;top:${y}px;width:${wide}px;height:${wide}px`;
      document.body.append(spark);
      spark.animate(
        [
          { transform: "translate(-50%,-50%) scale(0) rotate(0deg)", opacity: 1 },
          { transform: `translate(calc(-50% + ${Math.cos(angle) * far}px),calc(-50% + ${Math.sin(angle) * far}px)) scale(1) rotate(${Math.random() * 240 - 120}deg)`, opacity: 0 },
        ],
        { duration: 650 + Math.random() * 550, easing: "cubic-bezier(0.2,0.8,0.3,1)" },
      ).onfinish = () => spark.remove();
    }
  }
  if (!still) document.addEventListener("click", (event) => event.target.closest("button, a.link") && burst(event.clientX, event.clientY));

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
