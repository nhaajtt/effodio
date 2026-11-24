# Draws the app icon: a periodic-table element tile.
#   python scripts/make-icon.py              rewrites every icon file in the repo (app/src-tauri/icons, site/icon.png)
#   python scripts/make-icon.py <out_dir> [--preview]   only writes the element renders
# Writes element-<size>.png for every size below (each size drawn on its own,
# so small sizes drop the fine print instead of shrinking it into noise).
import io
import struct
import sys
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont

# ---- the element (edit per app) ----
SYMBOL = "Ef"
NAME = "Effodio"
NUMBER = "42"
MASS = "768.00"
TOP = (0x4a, 0x6a, 0xe0)
BOTTOM = (0x17, 0x21, 0x60)
# ------------------------------------

FONTS = Path("C:/Windows/Fonts")
BOLD = str(FONTS / "segoeuib.ttf")
SEMI = str(FONTS / "seguisb.ttf") if (FONTS / "seguisb.ttf").exists() else BOLD
SIZES = [16, 20, 24, 30, 32, 40, 44, 48, 50, 64, 71, 89, 107, 128, 142, 150, 256, 284, 310, 512, 1024]
SS = 4  # supersampling


def font(path, px):
    return ImageFont.truetype(path, max(1, round(px)))


def text_center(d, cx, cy, s, f, fill):
    l, t, r, b = d.textbbox((0, 0), s, font=f)
    d.text((cx - (l + r) / 2, cy - (t + b) / 2), s, font=f, fill=fill)


def render(size):
    n = size * SS
    small = size < 64
    full = size >= 256
    m = n * (0.03 if small else 0.06)          # outer margin
    w = n - 2 * m
    rad = w * (0.20 if small else 0.22)

    # gradient fill, diagonal
    g = 64
    grad = Image.new("RGBA", (g, g))
    gp = grad.load()
    for y in range(g):
        for x in range(g):
            t = (x + y) / (2 * (g - 1))
            gp[x, y] = tuple(round(TOP[i] + (BOTTOM[i] - TOP[i]) * t) for i in range(3)) + (255,)
    grad = grad.resize((n, n), Image.BICUBIC)
    mask = Image.new("L", (n, n), 0)
    ImageDraw.Draw(mask).rounded_rectangle([m, m, n - m, n - m], rad, fill=255)
    img = Image.new("RGBA", (n, n), (0, 0, 0, 0))
    img.paste(grad, (0, 0), mask)

    d = ImageDraw.Draw(img)
    white = (255, 255, 255, 255)
    soft = (255, 255, 255, 200)
    cx = n / 2

    if not small:
        # inner tile outline
        i = w * 0.075
        d.rounded_rectangle([m + i, m + i, n - m - i, n - m - i], rad * 0.62,
                            outline=(255, 255, 255, 110), width=max(SS, round(w * 0.010)))

    if small:
        text_center(d, cx, m + w * 0.50, SYMBOL, font(BOLD, w * 0.60), white)
    elif not full:
        fn = font(SEMI, w * 0.15)
        d.text((m + w * 0.15, m + w * 0.13), NUMBER, font=fn, fill=soft)
        text_center(d, cx, m + w * 0.53, SYMBOL, font(BOLD, w * 0.46), white)
        text_center(d, cx, m + w * 0.79, NAME, font(SEMI, w * 0.12), soft)
    else:
        fn = font(SEMI, w * 0.11)
        d.text((m + w * 0.15, m + w * 0.13), NUMBER, font=fn, fill=soft)
        text_center(d, cx, m + w * 0.49, SYMBOL, font(BOLD, w * 0.42), white)
        text_center(d, cx, m + w * 0.725, NAME, font(SEMI, w * 0.095), white)
        text_center(d, cx, m + w * 0.815, MASS, font(SEMI, w * 0.068), (255, 255, 255, 165))

    return img.resize((size, size), Image.LANCZOS)


PNGS = {
    "32x32.png": 32, "64x64.png": 64, "128x128.png": 128, "128x128@2x.png": 256, "icon.png": 512,
    "StoreLogo.png": 50, "Square30x30Logo.png": 30, "Square44x44Logo.png": 44, "Square71x71Logo.png": 71,
    "Square89x89Logo.png": 89, "Square107x107Logo.png": 107, "Square142x142Logo.png": 142,
    "Square150x150Logo.png": 150, "Square284x284Logo.png": 284, "Square310x310Logo.png": 310,
}
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 256]
ICNS_TYPES = [(b"ic11", 32), (b"ic12", 64), (b"ic07", 128), (b"ic13", 256), (b"ic08", 256), (b"ic14", 512),
              (b"ic09", 512), (b"ic10", 1024)]


def png_bytes(size):
    buf = io.BytesIO()
    render(size).save(buf, "PNG")
    return buf.getvalue()


def write_repo():
    root = Path(__file__).resolve().parent.parent
    icons = root / "app" / "src-tauri" / "icons"
    cache = {}

    def get(size):
        if size not in cache:
            cache[size] = png_bytes(size)
        return cache[size]

    for name, size in PNGS.items():
        (icons / name).write_bytes(get(size))
    (root / "site" / "icon.png").write_bytes(get(256))
    frames = [(s, get(s)) for s in ICO_SIZES]
    head = struct.pack("<HHH", 0, 1, len(frames))
    offset = 6 + 16 * len(frames)
    entries, body = b"", b""
    for s, data in frames:
        side = 0 if s >= 256 else s
        entries += struct.pack("<BBBBHHII", side, side, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
        body += data
    (icons / "icon.ico").write_bytes(head + entries + body)
    chunks = b"".join(t + struct.pack(">I", len(get(s)) + 8) + get(s) for t, s in ICNS_TYPES)
    (icons / "icon.icns").write_bytes(b"icns" + struct.pack(">I", len(chunks) + 8) + chunks)
    print("icons written")


def main():
    if len(sys.argv) < 2:
        write_repo()
        return
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    for s in SIZES:
        render(s).save(out / f"element-{s}.png")
    if "--preview" in sys.argv:
        show = [512, 128, 48, 32, 16]
        sheet = Image.new("RGBA", (sum(show) + 20 * len(show), 532), (240, 240, 240, 255))
        x = 10
        for s in show:
            im = Image.open(out / f"element-{s}.png")
            sheet.paste(im, (x, 10), im)
            x += s + 20
        sheet.save(out / "preview.png")


if __name__ == "__main__":
    main()
