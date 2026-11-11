// Glints: a few stars fly from the search box when results arrive, and from the gold button when it's pressed.

const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
let alive = 0;

function glint(x: number, y: number, far: number, wide: number, cool: boolean): void {
  if (alive > 40) return;
  alive++;
  const star = document.createElement("i");
  star.className = cool ? "glint cool" : "glint";
  const angle = Math.random() * Math.PI * 2;
  const dx = Math.cos(angle) * far;
  const dy = Math.sin(angle) * far;
  const turn = Math.random() * 260 - 130;
  star.style.cssText = `left:${x}px;top:${y}px;width:${wide}px;height:${wide}px`;
  document.body.append(star);
  star.animate(
    [
      { transform: "translate(-50%,-50%) scale(0) rotate(0deg)", opacity: 1 },
      { transform: `translate(calc(-50% + ${dx * 0.6}px),calc(-50% + ${dy * 0.6}px)) scale(1) rotate(${turn * 0.6}deg)`, opacity: 1, offset: 0.35 },
      { transform: `translate(calc(-50% + ${dx}px),calc(-50% + ${dy}px)) scale(0) rotate(${turn}deg)`, opacity: 0 },
    ],
    { duration: 600 + Math.random() * 500, easing: "cubic-bezier(0.2,0.8,0.3,1)" },
  ).onfinish = () => {
    star.remove();
    alive--;
  };
}

function burst(x: number, y: number, count: number, reach: number): void {
  for (let index = 0; index < count; index++) glint(x, y, reach * (0.4 + Math.random() * 0.6), 6 + Math.random() * 10, index % 3 === 0);
}

export function mountGlints(): void {
  if (still) return;
  // A search that just finished: the search box stops being busy.
  new MutationObserver((changes) => {
    for (const change of changes) {
      const box = change.target as HTMLElement;
      if (!box.classList?.contains("searchbox")) continue;
      const wasBusy = (change.oldValue ?? "").split(" ").includes("busy");
      if (wasBusy && !box.classList.contains("busy")) {
        const icon = box.querySelector(".spark")?.getBoundingClientRect();
        if (icon) burst(icon.left + icon.width / 2, icon.top + icon.height / 2, 7, 46);
      }
    }
  }).observe(document.body, { subtree: true, attributes: true, attributeFilter: ["class"], attributeOldValue: true });

  document.addEventListener("click", (event) => {
    if ((event.target as HTMLElement).closest?.(".button.primary")) burst(event.clientX, event.clientY, 10, 70);
  });
}
