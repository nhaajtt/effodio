// The parts of the page that are Magpie's own: the screen, the private band, the install section.
import { TEXT, COMMAND, INSTALLER } from "./site-data.js";

const $ = (id) => document.getElementById(id);
const escape = (text) => text.replace(/&/g, "&amp;").replace(/</g, "&lt;");
// [[words]] are the lit ones, as in the introduction.
const lit = (text) => escape(text).replace(/\[\[([^\]]+)\]\]/g, '<span class="lab-intro-key">$1</span>');

$("screen-note").textContent = TEXT.screen;
$("private-kicker").textContent = TEXT.privateKicker;
$("private-text").innerHTML = lit(TEXT.private);
$("install-kicker").textContent = TEXT.installKicker;
$("install-title").textContent = TEXT.installTitle;
$("cmdtext").textContent = COMMAND;
$("copy").textContent = TEXT.copy;
$("one").innerHTML = TEXT.one;
$("read").textContent = TEXT.read;
$("two").innerHTML = TEXT.two;
$("ffmpeg").textContent = TEXT.ffmpeg;
$("thanks").textContent = TEXT.thanks;

// The install line also sits at the top of the page, under the introduction.
// It is in the page already; here it gets its words in the reader's language and moves in beside the introduction.
const top = document.getElementById("top-install");
if (top) {
  top.querySelector("code").textContent = COMMAND;
  top.querySelector("button").textContent = TEXT.copy;
  top.querySelector("[data-label]").textContent = TEXT.download;
  top.querySelector("a").href = INSTALLER;
  const intro = document.querySelector(".lab-intro-main");
  if (intro) {
    top.classList.remove("g-top-static");
    intro.append(top);
  }
}

document.querySelectorAll(".g-cmd").forEach((bar) => {
  const button = bar.querySelector("button");
  const code = bar.querySelector("code");
  button.addEventListener("click", async () => {
    try {
      await navigator.clipboard.writeText(COMMAND);
    } catch {
      const range = document.createRange();
      range.selectNodeContents(code);
      getSelection().removeAllRanges();
      getSelection().addRange(range);
      return;
    }
    button.textContent = TEXT.copied;
    setTimeout(() => (button.textContent = TEXT.copy), 1600);
  });
});

// The recording plays only while it's on screen; a click pauses and resumes it.
const video = document.querySelector(".g-device video");
new IntersectionObserver(([entry]) => (entry.isIntersecting ? video.play().catch(() => {}) : video.pause()), { threshold: 0.25 }).observe(video);
video.addEventListener("click", () => (video.paused ? video.play() : video.pause()));
if (matchMedia("(prefers-reduced-motion: reduce)").matches) {
  video.removeAttribute("autoplay");
  video.controls = true;
  video.pause();
}
