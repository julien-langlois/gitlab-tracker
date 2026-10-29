#!/usr/bin/env python3
"""Draws the application icon and writes it in every format the platforms use.

    python3 -I assets/icon/make_icons.py        (needs Pillow)

Writes, next to this script:
  gitlab-tracker.svg          scalable icon (Linux hicolor/scalable)
  png/gitlab-tracker-<n>.png  16 … 1024 px (Linux hicolor, window icon)
  gitlab-tracker.ico          Windows (16 … 256 px)
  gitlab-tracker.icns         macOS (16 … 1024 px)

The motif is a generic merge graph (a trunk, a branch merging into it) inside a
tracking ring, in the window's blue accent: no tanuki, no GitLab orange.
"""

import math
from pathlib import Path

from PIL import Image, ImageDraw

HERE = Path(__file__).resolve().parent
SIZE = 1024
SCALE = 4  # supersampling: drawn at 4096 px, then downscaled (antialiasing)

BG_TOP = (0x16, 0x2B, 0x47)
BG_BOTTOM = (0x0B, 0x16, 0x28)
TRUNK = (0xE8, 0xEE, 0xF6)
ACCENT = (0x3B, 0xA2, 0xFF)
RING = (0x2A, 0x6B, 0xAE)  # the accent over the background, opaque

MARGIN, RADIUS = 72, 210  # background tile
STROKE = 60
TOP = (430, 252)  # trunk start
MERGE = (430, 672)  # where the branch lands; the ring is centred on it
BRANCH = (698, 356)  # branch start
CTRL1, CTRL2 = (698, 544), (430, 504)  # branch curve (cubic Bézier)
NODE_R, HOLE_R = 74, 34
RING_R, RING_W = 176, 30
# Degrees left open (clockwise from 3 o'clock, y down), where the trunk and the
# branch come in: the ring reads as a sweep rather than a target.
RING_GAP = (225, 345)


def bezier(p0, p1, p2, p3, steps=400):
    for i in range(steps + 1):
        t = i / steps
        u = 1 - t
        yield (
            u**3 * p0[0] + 3 * u * u * t * p1[0] + 3 * u * t * t * p2[0] + t**3 * p3[0],
            u**3 * p0[1] + 3 * u * u * t * p1[1] + 3 * u * t * t * p2[1] + t**3 * p3[1],
        )


def draw(size=SIZE):
    s = SCALE
    big = SIZE * s
    img = Image.new("RGBA", (big, big), (0, 0, 0, 0))

    # Background: vertical gradient clipped to a rounded tile.
    gradient = Image.new("RGBA", (big, big))
    g = ImageDraw.Draw(gradient)
    for y in range(big):
        t = y / (big - 1)
        g.line(
            [(0, y), (big, y)],
            fill=tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM)) + (255,),
        )
    mask = Image.new("L", (big, big), 0)
    ImageDraw.Draw(mask).rounded_rectangle(
        [MARGIN * s, MARGIN * s, (SIZE - MARGIN) * s, (SIZE - MARGIN) * s],
        radius=RADIUS * s,
        fill=255,
    )
    img.paste(gradient, (0, 0), mask)

    d = ImageDraw.Draw(img)

    def circle(c, r, fill):
        d.ellipse([(c[0] - r) * s, (c[1] - r) * s, (c[0] + r) * s, (c[1] + r) * s], fill=fill)

    def stroke(points, color):
        # Discs stamped along the path: smooth joins and round caps, which
        # `ImageDraw.line` does not give on a curve.
        points = list(points)
        r = STROKE / 2
        for (x0, y0), (x1, y1) in zip(points, points[1:]):
            n = max(1, round(max(abs(x1 - x0), abs(y1 - y0)) / 2))
            for i in range(n + 1):
                t = i / n
                circle((x0 + (x1 - x0) * t, y0 + (y1 - y0) * t), r, color)

    # Tracking ring around the merge point, open on one side.
    c = MERGE
    box = [(c[0] - RING_R) * s, (c[1] - RING_R) * s, (c[0] + RING_R) * s, (c[1] + RING_R) * s]
    d.arc(box, start=RING_GAP[1], end=RING_GAP[0], fill=RING, width=RING_W * s)

    stroke([TOP, MERGE], TRUNK)
    stroke(bezier(BRANCH, CTRL1, CTRL2, MERGE), ACCENT)

    bg_mid = tuple(round((a + b) / 2) for a, b in zip(BG_TOP, BG_BOTTOM))
    for node, color in ((TOP, TRUNK), (BRANCH, ACCENT), (MERGE, ACCENT)):
        circle(node, NODE_R, color)
        circle(node, HOLE_R, bg_mid)

    return img.resize((size, size), Image.LANCZOS)


def svg():
    hex_ = "#{:02x}{:02x}{:02x}".format
    (tx, ty), (mx, my), (bx, by) = TOP, MERGE, BRANCH
    bg_mid = tuple(round((a + b) / 2) for a, b in zip(BG_TOP, BG_BOTTOM))
    a0, a1 = math.radians(RING_GAP[1]), math.radians(RING_GAP[0])
    rx0, ry0 = mx + RING_R * math.cos(a0), my + RING_R * math.sin(a0)
    rx1, ry1 = mx + RING_R * math.cos(a1), my + RING_R * math.sin(a1)
    large = 1 if (RING_GAP[0] - RING_GAP[1]) % 360 > 180 else 0
    nodes = "".join(
        f'<circle cx="{x}" cy="{y}" r="{NODE_R}" fill="{col}"/>'
        f'<circle cx="{x}" cy="{y}" r="{HOLE_R}" fill="{hex_(*bg_mid)}"/>'
        for (x, y), col in ((TOP, hex_(*TRUNK)), (BRANCH, hex_(*ACCENT)), (MERGE, hex_(*ACCENT)))
    )
    side = SIZE - 2 * MARGIN
    return f"""<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {SIZE} {SIZE}" width="{SIZE}" height="{SIZE}">
  <defs>
    <linearGradient id="bg" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="{hex_(*BG_TOP)}"/>
      <stop offset="1" stop-color="{hex_(*BG_BOTTOM)}"/>
    </linearGradient>
  </defs>
  <rect x="{MARGIN}" y="{MARGIN}" width="{side}" height="{side}" rx="{RADIUS}" fill="url(#bg)"/>
  <path d="M{rx0:.1f} {ry0:.1f} A{RING_R} {RING_R} 0 {large} 1 {rx1:.1f} {ry1:.1f}" fill="none" stroke="{hex_(*RING)}" stroke-width="{RING_W}"/>
  <g fill="none" stroke-width="{STROKE}" stroke-linecap="round">
    <path d="M{tx} {ty} L{mx} {my}" stroke="{hex_(*TRUNK)}"/>
    <path d="M{bx} {by} C{CTRL1[0]} {CTRL1[1]} {CTRL2[0]} {CTRL2[1]} {mx} {my}" stroke="{hex_(*ACCENT)}"/>
  </g>
  {nodes}
</svg>
"""


def main():
    master = draw()
    (HERE / "png").mkdir(exist_ok=True)
    for n in (16, 24, 32, 48, 64, 128, 256, 512, 1024):
        img = master if n == SIZE else master.resize((n, n), Image.LANCZOS)
        img.save(HERE / "png" / f"gitlab-tracker-{n}.png")
    master.save(HERE / "gitlab-tracker.ico", sizes=[(n, n) for n in (16, 24, 32, 48, 64, 128, 256)])
    master.save(HERE / "gitlab-tracker.icns")
    (HERE / "gitlab-tracker.svg").write_text(svg())


if __name__ == "__main__":
    main()
