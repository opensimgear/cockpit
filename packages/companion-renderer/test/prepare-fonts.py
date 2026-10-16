"""Prepare immutable OFL Rajdhani glyph coverage for offline PoC recipes; never used on ESP32."""
import hashlib, json, math
from pathlib import Path
from PIL import Image, ImageDraw, ImageFont, __version__, features

ROOT = Path(__file__).resolve().parents[1]
FONT = ROOT / "assets" / "Rajdhani-SemiBold.ttf"
EXPECTED = "94bbd25a18ca665999feb05a537de9fd2b860dcfb78bbe9ca00270825bf235da"
assert hashlib.sha256(FONT.read_bytes()).hexdigest() == EXPECTED
assert __version__ == "12.3.0", "Use the pinned Pillow converter or intentionally republish exact resulting bytes"
CHARACTERS = " ?0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZ./%:+-"
fonts = []
for size in (14, 18, 20, 24, 36):
    face = ImageFont.truetype(str(FONT), size * 4)
    ascent, descent = face.getmetrics()
    glyphs = []
    for char in sorted(set(CHARACTERS), key=ord):
        x0, y0, x1, y1 = face.getbbox(char, anchor="ls")
        left, top, right, bottom = math.floor(x0 / 4), math.floor(y0 / 4), math.ceil(x1 / 4), math.ceil(y1 / 4)
        width, height = right - left, bottom - top
        if width and height:
            image = Image.new("L", (width * 4, height * 4))
            ImageDraw.Draw(image).text((-left * 4, -top * 4), char, font=face, fill=255, anchor="ls")
            image = image.resize((width, height), Image.Resampling.LANCZOS)
            alpha = list(image.get_flattened_data())
        else:
            alpha = []
        stride = (width + 1) // 2
        coverage = [0] * (stride * height)
        for y in range(height):
            for x in range(width):
                coverage[y * stride + x // 2] |= ((alpha[y * width + x] * 15 + 127) // 255) << (0 if x % 2 else 4)
        x_offset = max(0, left)
        glyphs.append(dict(codepoint=ord(char), width=width, height=height, xOffset=x_offset, yOffset=top,
                           advance=max(math.ceil(face.getlength(char) / 4), x_offset + width), coverage=coverage))
    fonts.append(dict(id="rajdhani-" + str(size), lineHeight=math.ceil((ascent + descent) / 4),
                      ascent=math.ceil(ascent / 4), glyphs=glyphs))
result = dict(sourceUrl="https://github.com/google/fonts/tree/main/ofl/rajdhani",
              sourceSha256=EXPECTED, license="SIL Open Font License 1.1; assets/OFL.txt",
              converter=dict(id="pillow-freetype-a4", pillow=__version__, freetype=features.version("freetype2"),
                             supersampling=4, filter="Lanczos", alphaRounding="nearest", characterSubset=CHARACTERS),
              fonts=fonts)
(ROOT / "assets" / "prepared-fonts.json").write_text(json.dumps(result, separators=(",", ":")) + "\n", encoding="utf-8")
print("Prepared", len(fonts), "font subsets from", EXPECTED)
