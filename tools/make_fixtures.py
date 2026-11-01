"""Generates tests/fixtures: words on varied backgrounds and words.tsv with where each word is.

    python tools/make_fixtures.py tests/fixtures [CJK_FONT]

Needs Pillow and the DejaVu and Liberation fonts. CJK_FONT (e.g. NotoSansSC-Regular.otf)
adds the Chinese image. Output is deterministic (fixed seed).
"""
import random
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFilter, ImageFont

DEJAVU = "/usr/share/fonts/truetype/dejavu/"
LIBERATION = "/usr/share/fonts/truetype/liberation/"
FONTS = {
    "dejavu-sans": DEJAVU + "DejaVuSans.ttf",
    "dejavu-sans-bold": DEJAVU + "DejaVuSans-Bold.ttf",
    "dejavu-serif": DEJAVU + "DejaVuSerif.ttf",
    "dejavu-mono": DEJAVU + "DejaVuSansMono.ttf",
    "arial-like": LIBERATION + "LiberationSans-Regular.ttf",
    "arial-like-bold": LIBERATION + "LiberationSans-Bold.ttf",
    "times-like": LIBERATION + "LiberationSerif-Regular.ttf",
    "narrow": LIBERATION + "LiberationSansNarrow-Regular.ttf",
}
SIZES = [11, 13, 16, 20, 28]
WIDTH = 760

VOCAB = [
    "falcon", "river", "pixel", "nova", "shadow", "maple", "echo", "delta", "orbit", "cobalt",
    "lunar", "tiger", "quartz", "willow", "ember", "atlas", "frost", "comet", "raven", "sable",
    "download", "settings", "report", "window", "market", "garden", "planet", "rocket",
]
CJK_WORDS = ["你好世界", "测试用户", "学习中文", "天气很好", "北京欢迎你", "数据分析", "小明123", "快乐每一天"]
CYRILLIC_WORDS = ["привет", "Москва", "игрок", "окно", "Сергей77", "погода", "текст", "буфер"]


def token(rng):
    """A word or a username-like token. Avoids I/l and O/0 pairs that no OCR can tell apart."""
    a, b = rng.choice(VOCAB), rng.choice(VOCAB)
    style = rng.randrange(8)
    if style == 0:
        return a
    if style == 1:
        return a.capitalize() + str(rng.randrange(2, 99))
    if style == 2:
        return f"{a}_{b}"
    if style == 3:
        return a.capitalize() + b.capitalize()
    if style == 4:
        return f"{a}.{b}"
    if style == 5:
        return f"{a}-{rng.randrange(2, 9)}"
    if style == 6:
        return f"{rng.randrange(2, 9)},{rng.randrange(100, 999)}"
    return "".join(rng.choice("abcdefghjkmnpqrstuvwxyz23456789") for _ in range(rng.randrange(5, 9)))


def solid(color):
    return lambda w, h, rng: Image.new("RGB", (w, h), color)


def gradient(c0, c1, vertical):
    def make(w, h, rng):
        img = Image.new("RGB", (w, h))
        n = h if vertical else w
        for i in range(n):
            t = i / max(n - 1, 1)
            c = tuple(round(a + (b - a) * t) for a, b in zip(c0, c1))
            box = [0, i, w, i + 1] if vertical else [i, 0, i + 1, h]
            ImageDraw.Draw(img).rectangle(box, fill=c)
        return img
    return make


def block_noise(w, h, rng):
    small = Image.new("RGB", (w // 4 + 1, h // 4 + 1))
    small.putdata([(v, v, v) for v in (rng.randrange(60, 150) for _ in range(small.width * small.height))])
    return small.resize((small.width * 4, small.height * 4), Image.NEAREST).crop((0, 0, w, h))


def photo(w, h, rng):
    img = Image.new("RGB", (w, h), (90, 110, 130))
    d = ImageDraw.Draw(img)
    for _ in range(60):
        x, y, r = rng.randrange(w), rng.randrange(h), rng.randrange(20, 120)
        d.ellipse([x - r, y - r, x + r, y + r], fill=tuple(rng.randrange(30, 230) for _ in range(3)))
    return img.filter(ImageFilter.GaussianBlur(10))


# name, background, text colour, outline colour (or None), fonts, words
CASES = [
    ("light", solid((255, 255, 255)), (17, 17, 17), None, ["arial-like", "dejavu-sans"], None),
    ("dark", solid((30, 30, 30)), (230, 230, 230), None, ["dejavu-sans", "arial-like-bold"], None),
    ("dark-muted", solid((43, 45, 49)), (181, 186, 193), None, ["arial-like", "dejavu-mono"], None),
    ("blue", solid((29, 78, 216)), (255, 255, 255), None, ["dejavu-sans-bold", "times-like"], None),
    ("yellow", solid((255, 243, 196)), (138, 28, 28), None, ["dejavu-serif", "narrow"], None),
    ("gradient-h", gradient((109, 40, 217), (249, 115, 22), False), (255, 255, 255), None, ["arial-like-bold", "dejavu-sans"], None),
    ("gradient-v", gradient((241, 245, 249), (100, 116, 139), True), (15, 23, 42), None, ["times-like", "arial-like"], None),
    ("noise", block_noise, (255, 255, 255), None, ["dejavu-sans-bold", "arial-like-bold"], None),
    ("photo", photo, (255, 255, 255), (0, 0, 0), ["arial-like-bold", "dejavu-sans"], None),
    ("low-contrast", solid((58, 58, 58)), (112, 112, 112), None, ["arial-like", "dejavu-sans"], None),
    ("cyrillic", solid((255, 255, 255)), (17, 17, 17), None, ["arial-like", "dejavu-sans"], CYRILLIC_WORDS),
]


def render(out, name, background, color, outline, fonts, words, rng, rows):
    line_gap = [round(s * 2.4) + 8 for s in SIZES for _ in fonts]
    height = 20 + sum(line_gap)
    img = background(WIDTH, height, rng)
    d = ImageDraw.Draw(img)
    stroke = {"stroke_width": 1, "stroke_fill": outline} if outline else {}
    y = 16
    for size in SIZES:
        for font_name in fonts:
            font = ImageFont.truetype(FONTS.get(font_name, font_name), size)
            x = 16 + rng.randrange(0, 40)
            for _ in range(rng.randrange(2, 4)):
                word = rng.choice(words) if words else token(rng)
                tail = rng.choice(["", "", "", ":", ","])
                if x + d.textlength(word + tail + " ", font=font) > WIDTH - 16:
                    break
                d.text((x, y), word + tail, font=font, fill=color, **stroke)
                x0, y0, x1, y1 = d.textbbox((x, y), word, font=font, stroke_width=1 if outline else 0)
                rows.append(f"{name}.png\t{x0}\t{y0}\t{x1}\t{y1}\t{word}\t{name}\t{Path(font_name).stem}\t{size}")
                x += d.textlength(word + tail + " ", font=font)
            y += round(size * 2.4) + 8
    img.save(out / f"{name}.png", optimize=True)


def main():
    out = Path(sys.argv[1])
    out.mkdir(parents=True, exist_ok=True)
    cases = list(CASES)
    if len(sys.argv) > 2:
        cases.append(("cjk", solid((255, 255, 255)), (17, 17, 17), None, [sys.argv[2]], CJK_WORDS))
    rng = random.Random(20261101)
    rows = ["image\tx0\ty0\tx1\ty1\tword\tcategory\tfont\tsize"]
    for case in cases:
        render(out, *case, rng, rows)
    (out / "words.tsv").write_text("\n".join(rows) + "\n", encoding="utf-8")
    print(f"{len(rows) - 1} words in {len(cases)} images -> {out}")


if __name__ == "__main__":
    main()
