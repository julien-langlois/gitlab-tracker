#!/usr/bin/env python3
"""Renders terminal captures (`tmux capture-pane -e -p` output, ANSI SGR colours)
into an animated GIF, for assets/demo.gif (scripts/record-demo-gifs.sh).

    ansi2gif.py OUT.gif FRAME_MS FRAME1.ansi [FRAME2.ansi …]

Symbols the monospace font lacks come from DejaVu Sans or Noto Sans Symbols.
A frame file may end in `@<ms>` in its name's stem (`07@2500.ansi`) to stay on
screen longer. Needs Pillow, DejaVu Sans Mono and, for emoji, Noto Color Emoji.
"""

import re
import sys
import unicodedata
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

FONT_DIR = Path("/usr/share/fonts/truetype")
SIZE = 15
PAD = 14
# Catppuccin Macchiato, as the old VHS tape used.
BG = (36, 39, 58)
FG = (202, 211, 245)
BASE16 = [
    (73, 77, 100), (237, 135, 150), (166, 218, 149), (238, 212, 159),
    (138, 173, 244), (245, 189, 230), (139, 213, 202), (184, 192, 224),
    (91, 96, 120), (237, 135, 150), (166, 218, 149), (238, 212, 159),
    (138, 173, 244), (245, 189, 230), (139, 213, 202), (165, 173, 203),
]


def xterm256(n):
    if n < 16:
        return BASE16[n]
    if n < 232:
        n -= 16
        steps = [0, 95, 135, 175, 215, 255]
        return steps[n // 36], steps[(n // 6) % 6], steps[n % 6]
    v = 8 + (n - 232) * 10
    return v, v, v


SGR = re.compile(r"\x1b\[([0-9;:]*)m")
OTHER_ESC = re.compile(r"\x1b(\[[0-9;?]*[A-Za-ln-z]|\][^\x07]*\x07|[()][0-9A-B])")


def parse(text):
    """Lines of (char, fg, bg, bold, reverse) cells."""
    text = OTHER_ESC.sub("", text)
    lines = []
    fg, bg, bold, rev = FG, BG, False, False
    for raw in text.split("\n"):
        cells = []
        pos = 0
        for m in SGR.finditer(raw):
            cells += [(c, fg, bg, bold, rev) for c in raw[pos:m.start()]]
            pos = m.end()
            codes = [int(c) if c else 0 for c in re.split("[;:]", m.group(1))] or [0]
            i = 0
            while i < len(codes):
                c = codes[i]
                if c == 0:
                    fg, bg, bold, rev = FG, BG, False, False
                elif c == 1:
                    bold = True
                elif c == 22:
                    bold = False
                elif c == 7:
                    rev = True
                elif c == 27:
                    rev = False
                elif 30 <= c <= 37:
                    fg = BASE16[c - 30]
                elif 90 <= c <= 97:
                    fg = BASE16[c - 90 + 8]
                elif c == 39:
                    fg = FG
                elif 40 <= c <= 47:
                    bg = BASE16[c - 40]
                elif 100 <= c <= 107:
                    bg = BASE16[c - 100 + 8]
                elif c == 49:
                    bg = BG
                elif c in (38, 48) and i + 1 < len(codes):
                    if codes[i + 1] == 5 and i + 2 < len(codes):
                        col = xterm256(codes[i + 2])
                        i += 2
                    elif codes[i + 1] == 2 and i + 4 < len(codes):
                        col = tuple(codes[i + 2:i + 5])
                        i += 4
                    else:
                        col = None
                    if col:
                        if c == 38:
                            fg = col
                        else:
                            bg = col
                i += 1
        cells += [(c, fg, bg, bold, rev) for c in raw[pos:]]
        lines.append(cells)
    while lines and not lines[-1]:
        lines.pop()
    return lines


def wide(ch):
    """Two cells, as tmux counts them (East Asian Wide / Fullwidth)."""
    return unicodedata.east_asian_width(ch) in ("W", "F")


_TOFU = {}


def missing(font, ch):
    """Whether the font draws `ch` as its "missing glyph" box."""
    def drawn(c):
        img = Image.new("L", (2 * SIZE, 2 * SIZE))
        ImageDraw.Draw(img).text((0, 0), c, fill=255, font=font)
        return img.tobytes()

    if font not in _TOFU:
        _TOFU[font] = drawn("\U0010FFFD")
    return drawn(ch) == _TOFU[font]


def render(lines, cols, rows, fonts):
    regular, bold, emoji, fallbacks = fonts
    cw = int(regular.getlength("M"))
    ch = SIZE + 5
    img = Image.new("RGB", (cols * cw + 2 * PAD, rows * ch + 2 * PAD), BG)
    draw = ImageDraw.Draw(img)
    for y, cells in enumerate(lines[:rows]):
        x = 0
        for c, fg, bg, b, rev in cells:
            if rev:
                fg, bg = bg, fg
            px, py = PAD + x * cw, PAD + y * ch
            w = 2 if wide(c) else 1
            if bg != BG:
                draw.rectangle([px, py, px + w * cw - 1, py + ch - 1], fill=bg)
            if c.strip():
                font = bold if b else regular
                if emoji and (w == 2 or ord(c) >= 0x1F000):
                    # Emoji: the colour font, as tall as a line, centred on the
                    # cells tmux gave them (a one-cell emoji overflows a little,
                    # as in a terminal).
                    glyph = Image.new("RGBA", (136, 128))
                    ImageDraw.Draw(glyph).text((0, 0), c, font=emoji, embedded_color=True)
                    side = ch - 2
                    glyph = glyph.resize((side, int(side * 128 / 136)), Image.LANCZOS)
                    img.paste(glyph, (px + (w * cw - side) // 2, py + (ch - glyph.height) // 2), glyph)
                else:
                    if missing(font, c):
                        # A symbol the monospace font lacks: the first fallback
                        # that has it.
                        font = next((f for f in fallbacks if not missing(f, c)), font)
                    draw.text((px, py + 2), c, fill=fg, font=font)
            x += w
    return img


def main():
    out, frame_ms, *frames = sys.argv[1:]
    noto = FONT_DIR / "noto/NotoColorEmoji.ttf"
    fallbacks = [
        ImageFont.truetype(str(FONT_DIR / name), SIZE)
        for name in ("dejavu/DejaVuSans.ttf", "noto/NotoSansSymbols2-Regular.ttf",
                     "noto/NotoSansSymbols-Regular.ttf", "noto/NotoSansMath-Regular.ttf")
        if (FONT_DIR / name).exists()
    ]
    fonts = (
        ImageFont.truetype(str(FONT_DIR / "dejavu/DejaVuSansMono.ttf"), SIZE),
        ImageFont.truetype(str(FONT_DIR / "dejavu/DejaVuSansMono-Bold.ttf"), SIZE),
        ImageFont.truetype(str(noto), 109) if noto.exists() else None,
        fallbacks,
    )
    parsed = [parse(Path(f).read_text(errors="replace")) for f in frames]
    cols = max(sum(2 if wide(c[0]) else 1 for c in line) for p in parsed for line in p)
    rows = max(len(p) for p in parsed)
    images, durations = [], []
    for f, lines in zip(frames, parsed):
        images.append(render(lines, cols, rows, fonts).convert("P", palette=Image.ADAPTIVE))
        stem = Path(f).stem
        durations.append(int(stem.split("@")[1]) if "@" in stem else int(frame_ms))
    images[0].save(out, save_all=True, append_images=images[1:], duration=durations,
                   loop=0, optimize=True)


if __name__ == "__main__":
    main()
