(() => {
  const root = document.documentElement;
  const $ = (id) => document.getElementById(id);

  const vi = {
    nav_does: "Tính năng", nav_private: "Riêng tư", nav_install: "Cài đặt",
    eyebrow: "Cho Windows 10 và 11 · miễn phí",
    h1a: "Tả nó ra.", h1b: "Tìm thấy ngay.",
    lede: "Glint tìm file trên máy bạn theo nội dung: ảnh theo thứ nó chụp, PDF theo điều một trang viết, ghi âm hay video theo đúng khoảnh khắc. Mọi thứ chạy ngay trên máy bạn.",
    copy: "Sao chép", copied: "Đã chép", or: "hoặc", dl: "tải bộ cài (8,8 MB)",
    demo_files: "12.480 file", demo_open: "Mở", demo_like: "Tương tự", demo_preview: "Xem trước",
    hotkey: "mở nó từ bất kỳ ứng dụng nào",
    does_h: "Một ô tìm kiếm cho mọi loại file",
    f1h: "Theo ý nghĩa", f1p: "Gõ \"chó trên bãi biển\" là ra tấm ảnh, không cần nhãn hay nhớ tên file. Dùng được với hơn 100 ngôn ngữ.",
    f2h: "Đến đúng trang, đúng giây", f2p: "Kết quả mở tại chỗ khớp: PDF ở trang 3, video ở lúc con ngựa vằn bước vào. Một dải thời gian chỉ ra mọi đoạn khớp.",
    f3h: "Chữ trong ảnh", f3p: "Windows đọc chữ trong ảnh chụp màn hình và bản scan, nên mã lỗi hay mã đặt chỗ được tìm đúng từng ký tự.",
    f4h: "Tìm theo ví dụ", f4p: "Thả hoặc dán một tấm ảnh, hoặc nhấn Tab trên một kết quả, Glint sẽ tìm những thứ giống nó.",
    f5h: "Bộ lọc gõ thẳng",
    f6h: "Không cần gõ dấu", f6p: "\"hoa don tien dien\" vẫn ra \"Hóa đơn tiền điện tháng 10\". Kèm theo là bản đồ mọi file theo ý nghĩa và danh sách file gần trùng.",
    priv_h: "Không có gì rời khỏi máy bạn",
    priv_p: "Model hiểu file của bạn là một model nhỏ chạy ngay trên máy. Glint lên mạng đúng một lần để tải nó (865 MB), sau đó chạy offline. Không tài khoản, không thu thập dữ liệu.",
    p1: "Chỉ đọc file. Không bao giờ sửa, chuyển hay tải chúng lên.",
    p2: "Bỏ qua khóa, chứng chỉ, file mật khẩu, thư mục ẩn và thư mục hệ thống.",
    p3: "Trong dự án mã nguồn, chỉ đọc ảnh chụp màn hình.",
    p4: "Cài cho riêng tài khoản của bạn. Không cần quyền quản trị.",
    inst_h: "Hai cách cài",
    i1h: "Một dòng trong PowerShell", i1p: "Tải bộ cài từ trang này, kiểm SHA-256 rồi cài im lặng. Chạy lại đúng dòng đó để cập nhật.",
    i1read: "Đọc script trước khi chạy",
    i2h: "Bộ cài", i2p: "Bộ cài chưa được ký số nên Windows sẽ hỏi trước khi chạy: chọn More info rồi Run anyway.",
    sha: "SHA-256 của bộ cài:",
    ffmpeg: "Ghi âm và video cần FFmpeg: winget install Gyan.FFmpeg. Những thứ khác chạy được mà không cần nó.",
    thanks: "Glint học hỏi từ DigUp của Abdur Rahim và dựng trên EmbeddingGemma 2, llama.cpp, Tauri và SQLite. Glint không liên kết với Google hay Microsoft.",
  };
  const en = {};
  document.querySelectorAll("[data-t]").forEach((el) => (en[el.dataset.t] = el.textContent));
  en.copied = "Copied";

  let lang = localStorage.getItem("lang") || (navigator.language.startsWith("vi") ? "vi" : "en");
  const words = () => (lang === "vi" ? vi : en);
  function speak() {
    root.lang = lang;
    document.querySelectorAll("[data-t]").forEach((el) => (el.textContent = words()[el.dataset.t] ?? en[el.dataset.t]));
    $("lang").textContent = lang === "vi" ? "EN" : "VI";
    document.dispatchEvent(new CustomEvent("glint:lang"));
  }
  $("lang").onclick = () => {
    lang = lang === "vi" ? "en" : "vi";
    localStorage.setItem("lang", lang);
    speak();
  };
  speak();

  const saved = localStorage.getItem("theme");
  root.dataset.theme = saved || (matchMedia("(prefers-color-scheme: light)").matches ? "light" : "dark");
  $("theme").onclick = () => {
    root.dataset.theme = root.dataset.theme === "dark" ? "light" : "dark";
    localStorage.setItem("theme", root.dataset.theme);
  };

  $("copy").onclick = async () => {
    const text = $("cmdtext").textContent;
    try {
      await navigator.clipboard.writeText(text);
    } catch {
      const range = document.createRange();
      range.selectNodeContents($("cmdtext"));
      getSelection().removeAllRanges();
      getSelection().addRange(range);
      return;
    }
    $("copy").textContent = words().copied;
    setTimeout(() => ($("copy").textContent = words().copy), 1600);
  };

  document.addEventListener("pointermove", (event) => {
    const glass = event.target.closest?.(".glass");
    if (!glass) return;
    const box = glass.getBoundingClientRect();
    glass.style.setProperty("--mx", event.clientX - box.left + "px");
    glass.style.setProperty("--my", event.clientY - box.top + "px");
  });

  const still = matchMedia("(prefers-reduced-motion: reduce)").matches;
  const seen = new IntersectionObserver(
    (entries) => entries.forEach((entry) => entry.isIntersecting && (entry.target.classList.add("in"), seen.unobserve(entry.target))),
    { threshold: 0.15 },
  );
  document.querySelectorAll(".card, .facts, h2, .split p").forEach((el, index) => {
    if (still) return;
    el.classList.add("reveal");
    el.style.transitionDelay = (index % 3) * 70 + "ms";
    el.addEventListener("transitionend", () => (el.style.transitionDelay = ""), { once: true });
    seen.observe(el);
  });

  // The search panel, playing a few searches.
  const icons = {
    photo: '<rect x="3" y="5" width="18" height="14" rx="2"/><circle cx="9" cy="10" r="1.6"/><path d="M4 17l5-4 4 3 3-2 4 3"/>',
    pdf: '<path d="M7 3h7l4 4v14H7z"/><path d="M14 3v4h4M10 12h5M10 16h5"/>',
    video: '<rect x="3" y="6" width="13" height="12" rx="2"/><path d="M16 10l5-3v10l-5-3z"/>',
    audio: '<path d="M4 10v4M8 7v10M12 4v16M16 8v8M20 11v2"/>',
    shot: '<rect x="3" y="4" width="18" height="13" rx="2"/><path d="M9 21h6M12 17v4"/>',
    doc: '<path d="M7 3h10v18H7z"/><path d="M10 8h4M10 12h4M10 16h3"/>',
  };
  const plays = [
    { q: "a dog on the beach", rows: [["photo", "IMG_2291.jpg", "Pictures › Summer", "Photo"], ["photo", "IMG_2304.jpg", "Pictures › Summer", "Photo"], ["video", "Day at the coast.mp4", "Videos › 2026", "0:42"]] },
    { q: "the clause about pets in the lease", rows: [["pdf", "Lease - 14 Elm Street.pdf", "<mark>Pets</mark>. The tenant may keep one cat or one small dog", "Page 3 of 4"], ["doc", "Moving checklist.docx", "Documents › Home", "Document"], ["shot", "Screenshot 2026-09-02.png", "Pictures › Screenshots", "Screenshot"]] },
    { q: "hoa don tien dien", rows: [["doc", "Hóa đơn tiền điện tháng 10.txt", "Kỳ thanh toán tháng 10, số <mark>điện</mark> tiêu thụ 312 kWh", "Document"], ["pdf", "EVN-2026-10.pdf", "Documents › Bills", "Page 1"], ["photo", "scan-0007.jpg", "Documents › Scans", "Photo"]] },
    { q: "PAY-7731-DX", rows: [["shot", "Screenshot 2026-10-18.png", "Payment declined. Reference <mark>PAY-7731-DX</mark>", "Screenshot"], ["pdf", "Receipt October.pdf", "Order <mark>PAY-7731-DX</mark> was refunded on", "Page 2"], ["audio", "Support call.m4a", "Recordings", "3:15"]] },
    { q: "zebra kind:video", rows: [["video", "Masai Mara - day 2.mp4", "Videos › Kenya", "0:06"], ["video", "Safari drive.mp4", "Videos › Kenya", "1:48"], ["video", "Zoo with the kids.mov", "Videos › 2025", "0:31"]] },
  ];
  const typed = $("typed");
  const rows = $("rows");
  const wait = (ms) => new Promise((done) => setTimeout(done, ms));
  function show(play) {
    rows.innerHTML = play.rows
      .map(
        ([kind, name, sub, badge], index) =>
          `<div class="row" style="animation-delay:${index * 90}ms"><span class="thumb k-${kind}"><svg viewBox="0 0 24 24">${icons[kind]}</svg></span><div><strong>${name}</strong><small>${sub}</small></div><span class="badge">${badge}</span></div>`,
      )
      .join("");
    document.dispatchEvent(new CustomEvent("glint:search"));
  }
  async function run() {
    if (still) {
      typed.textContent = plays[0].q;
      show(plays[0]);
      return;
    }
    for (let turn = 0; ; turn = (turn + 1) % plays.length) {
      const play = plays[turn];
      typed.textContent = "";
      for (const letter of play.q) {
        typed.textContent += letter;
        await wait(38 + Math.random() * 50);
      }
      await wait(220);
      show(play);
      await wait(3600);
      while (document.hidden) await wait(500);
    }
  }
  run();
})();
