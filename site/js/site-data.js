// Everything the page says, in both languages: read by the header, the intro, the timeline, the grid and the
// install section.

const asked = new URLSearchParams(location.search).get("lang");
let saved = null;
try {
  saved = localStorage.getItem("lang");
} catch {}
export const LANG = asked || saved || (navigator.language.startsWith("vi") ? "vi" : "en");
document.documentElement.lang = LANG;

export const VERSION = "__VERSION__";
export const INSTALLER = `download/Glint_${VERSION}_x64-setup.exe`;
export const COMMAND = 'powershell -c "irm https://glint.nhaajt.com/install.ps1 | iex"';

const en = {
  lede: "[[#GLINT]] by [[@Nhaajt|https://www.nhaajt.com]]. It finds the files on your computer by what's in them: [[describe it]] the way you remember it and get [[the photo]], [[the PDF page]], [[the second of the video]]. One small model, running on [[your own PC]], in 100+ languages.",
  note: "Free, for Windows 10 and 11. Nothing you index leaves your computer. It goes online once, to download the model.",
  place: "on your PC",
  install: "Install",
  download: "Download .exe",
  jump: "Jump to",
  soon: "macOS · coming",
  hint: "swipe through the plan",
  screen: "Recorded in the app, on a folder of sample files",
  installKicker: "05 INSTALL · TWO WAYS",
  installTitle: "One line, or one file",
  copy: "Copy",
  copied: "Copied",
  one: "<b>One line in PowerShell.</b> Downloads the installer from this site, checks its SHA-256 and installs it quietly, for your user only. Run it again to update.",
  read: "Read the script first",
  two: "<b>Or the installer.</b> It isn't signed yet, so Windows asks before running it: More info, then Run anyway.",
  ffmpeg: "Recordings and videos need FFmpeg (winget install Gyan.FFmpeg). Everything else works without it.",
  privateKicker: "PRIVATE · NOTHING LEAVES YOUR COMPUTER",
  private: "It [[only reads]] your files: never changes, moves or uploads them. It skips [[keys, certificates and password files]], hidden and system folders, and in a code project it reads only screenshots.",
  thanks: "Glint learned from DigUp by Abdur Rahim and is built on EmbeddingGemma 2, llama.cpp, Tauri and SQLite. It isn't affiliated with Google or Microsoft.",
  projects: [
    ["04", "FILTERS · IN THE SAME BOX", "Narrow it as you type", "Kind, folder, type and date go in with your words, and a minus steers away from something. Accents are optional: “hoa don tien dien” finds “Hóa đơn tiền điện tháng 10”.", ["kind:video", "in:Downloads", "after:30d", "-word"]],
    ["03", "EXACT WORDS · SCREENSHOTS", "Text inside pictures", "Windows reads the words in screenshots and scans, so an error code or a booking number is found exactly as written, wherever it's written.", ["OCR", "PAY-7731-DX", "Scans"]],
    ["02", "MOMENTS · VIDEO AND SOUND", "To the page, to the second", "A result opens where it matched: the PDF at page 3, the video at the moment the zebra walks in. A strip under a recording shows every place the search matches.", ["PDF", "MP4", "M4A", "DOCX"]],
    ["01", "MEANING · PHOTOS", "Search by what it shows", "Type “a dog on the beach” and get the photo, with no tags and no file names to remember. Drop in a picture, or press Tab on a result, to find more like it.", ["JPG", "PNG", "HEIC", "RAW"]],
  ],
  timeline: {
    now: ["Glint 0.1", "Windows", "macOS", "planned"],
    signed: ["Signed installer", "planned"],
    gpu: ["GPU build", "Vulkan · prepared"],
    model: ["EmbeddingGemma 2", "the model"],
    digup: ["DigUp", "where it learned"],
  },
};

const vi = {
  ...en,
  lede: "[[#GLINT]] của [[@Nhaajt|https://www.nhaajt.com]]. Nó tìm file trên máy bạn theo nội dung bên trong: [[tả nó ra]] theo cách bạn nhớ và nhận về [[tấm ảnh]], [[trang PDF]], [[đúng giây của video]]. Một model nhỏ, chạy trên [[chính máy bạn]], hiểu hơn 100 ngôn ngữ.",
  note: "Miễn phí, cho Windows 10 và 11. Không có gì bạn lập chỉ mục rời khỏi máy. Nó chỉ lên mạng một lần để tải model.",
  place: "trên máy bạn",
  install: "Cài đặt",
  download: "Tải file .exe",
  jump: "Đi tới",
  soon: "macOS · sắp có",
  hint: "vuốt để xem kế hoạch",
  screen: "Quay ngay trong app, trên một thư mục file mẫu",
  installKicker: "05 CÀI ĐẶT · HAI CÁCH",
  installTitle: "Một dòng lệnh, hoặc một file",
  copy: "Sao chép",
  copied: "Đã chép",
  one: "<b>Một dòng trong PowerShell.</b> Tải bộ cài từ trang này, kiểm SHA-256 rồi cài im lặng, chỉ cho tài khoản của bạn. Chạy lại để cập nhật.",
  read: "Đọc script trước",
  two: "<b>Hoặc bộ cài.</b> Nó chưa được ký số nên Windows sẽ hỏi trước khi chạy: More info rồi Run anyway.",
  ffmpeg: "Ghi âm và video cần FFmpeg (winget install Gyan.FFmpeg). Những thứ khác chạy được mà không cần nó.",
  privateKicker: "RIÊNG TƯ · KHÔNG GÌ RỜI KHỎI MÁY BẠN",
  private: "Nó [[chỉ đọc]] file của bạn: không bao giờ sửa, chuyển hay tải lên. Nó bỏ qua [[khóa, chứng chỉ và file mật khẩu]], thư mục ẩn và thư mục hệ thống, và trong dự án mã nguồn nó chỉ đọc ảnh chụp màn hình.",
  thanks: "Glint học hỏi từ DigUp của Abdur Rahim và dựng trên EmbeddingGemma 2, llama.cpp, Tauri và SQLite. Glint không liên kết với Google hay Microsoft.",
  projects: [
    ["04", "BỘ LỌC · NGAY TRONG Ô TÌM", "Thu hẹp ngay khi gõ", "Loại, thư mục, đuôi file và ngày tháng gõ chung với câu tìm, dấu trừ để tránh một thứ. Không cần gõ dấu: “hoa don tien dien” vẫn ra “Hóa đơn tiền điện tháng 10”.", ["kind:video", "in:Downloads", "after:30d", "-word"]],
    ["03", "ĐÚNG TỪNG CHỮ · ẢNH CHỤP MÀN HÌNH", "Chữ nằm trong ảnh", "Windows đọc chữ trong ảnh chụp màn hình và bản scan, nên mã lỗi hay mã đặt chỗ được tìm đúng từng ký tự, dù nó nằm ở đâu.", ["OCR", "PAY-7731-DX", "Scan"]],
    ["02", "KHOẢNH KHẮC · VIDEO VÀ ÂM THANH", "Đến đúng trang, đúng giây", "Kết quả mở tại chỗ khớp: PDF ở trang 3, video ở lúc con ngựa vằn bước vào. Dải thời gian dưới bản ghi chỉ ra mọi đoạn khớp.", ["PDF", "MP4", "M4A", "DOCX"]],
    ["01", "Ý NGHĨA · ẢNH", "Tìm theo thứ nó chụp", "Gõ “chó trên bãi biển” là ra tấm ảnh, không cần nhãn, không cần nhớ tên file. Thả một tấm ảnh vào, hoặc nhấn Tab trên một kết quả, để tìm thứ giống nó.", ["JPG", "PNG", "HEIC", "RAW"]],
  ],
  timeline: {
    now: ["Glint 0.1", "Windows", "macOS", "dự kiến"],
    signed: ["Bộ cài ký số", "dự kiến"],
    gpu: ["Bản GPU", "Vulkan · đã chuẩn bị"],
    model: ["EmbeddingGemma 2", "model"],
    digup: ["DigUp", "nơi nó học hỏi"],
  },
};

export const TEXT = LANG === "vi" ? vi : en;

export const PROJECTS = TEXT.projects.map(([num, kind, title, desc, tags], index) => ({
  slug: ["filters", "ocr", "moments", "meaning"][index],
  num,
  kind,
  title,
  desc,
  tags,
  href: "#install",
}));
