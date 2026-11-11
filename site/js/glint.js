// The parts of the page that are Glint's own: the screen, the private band, the install section.
import { TEXT, COMMAND } from "./site-data.js";

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

$("copy").addEventListener("click", async () => {
  try {
    await navigator.clipboard.writeText(COMMAND);
  } catch {
    const range = document.createRange();
    range.selectNodeContents($("cmdtext"));
    getSelection().removeAllRanges();
    getSelection().addRange(range);
    return;
  }
  $("copy").textContent = TEXT.copied;
  setTimeout(() => ($("copy").textContent = TEXT.copy), 1600);
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
