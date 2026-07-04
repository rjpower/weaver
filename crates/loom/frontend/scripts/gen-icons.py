#!/usr/bin/env python3
"""Regenerate the PNG app-icon fallbacks from the same geometry as favicon.svg.

`favicon.svg` (a spool of thread on the brand blue) is the human-editable source
of the weaver app icon; modern browsers use it directly. This script rasterizes
the *same* shape into the two PNG fallbacks the HTML head also links:

    favicon-32.png       32x32   <link rel="alternate icon">  (older browsers)
    apple-touch-icon.png 180x180 <link rel="apple-touch-icon"> (iOS home screen)

Pure stdlib (no Pillow / no system rasterizer), so it runs anywhere Python does.
Edit favicon.svg, mirror the change in `spool()` below, then re-run:

    python3 crates/loom/frontend/scripts/gen-icons.py

Outputs land next to favicon.svg in ../src. rspack copies all three into the
served bundle (see rspack.config.js CopyRspackPlugin).
"""

import math
import os
import struct
import zlib

SS = 3  # supersample factor for antialiasing

BLUE = (0x2F, 0x6F, 0xED, 255)
WHITE = (0xFF, 0xFF, 0xFF, 255)
AMBER = (0xFF, 0xB0, 0x20, 255)


def seg_dist(px, py, ax, ay, bx, by):
    vx, vy = bx - ax, by - ay
    wx, wy = px - ax, py - ay
    length2 = vx * vx + vy * vy
    t = 0.0 if length2 == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / length2))
    dx, dy = px - (ax + t * vx), py - (ay + t * vy)
    return math.hypot(dx, dy)


def in_rrect(x, y, rx, ry, w, h, r):
    if not (rx <= x <= rx + w and ry <= y <= ry + h):
        return False
    cx = min(max(x, rx + r), rx + w - r)
    cy = min(max(y, ry + r), ry + h - r)
    return (x - cx) ** 2 + (y - cy) ** 2 <= r * r


def rrect(x, y, w, h, r, fill):
    return {"t": "rrect", "x": x, "y": y, "w": w, "h": h, "r": r, "fill": fill}


def seg(x1, y1, x2, y2, w, color):
    return {"t": "seg", "x1": x1, "y1": y1, "x2": x2, "y2": y2, "w": w, "color": color}


def cov(p, x, y):
    if p["t"] == "rrect":
        return 1.0 if in_rrect(x, y, p["x"], p["y"], p["w"], p["h"], p["r"]) else 0.0
    return 1.0 if seg_dist(x, y, p["x1"], p["y1"], p["x2"], p["y2"]) <= p["w"] / 2 else 0.0


def spool():
    """The icon scene in a 64-unit viewBox, matching favicon.svg exactly."""
    bg = rrect(2, 2, 60, 60, 14, BLUE)
    fg = [rrect(24, 16, 16, 32, 3, WHITE)]  # core (behind the wound thread)
    y = 19.0
    while y <= 45:  # wound thread
        fg.append(seg(24, y, 40, y, 3.2, AMBER))
        y += 4.5
    fg.append(rrect(14, 12, 36, 7, 3, WHITE))  # top flange
    fg.append(rrect(14, 45, 36, 7, 3, WHITE))  # bottom flange
    tail = [(40, 48), (46, 50), (48, 45), (44, 42)]  # loose thread tail
    for a, b in zip(tail, tail[1:]):
        fg.append(seg(a[0], a[1], b[0], b[1], 3.0, AMBER))
    return bg, fg


def render(size, bg, fg):
    s = size * SS
    scale = s / 64.0
    buf = [[0.0, 0.0, 0.0, 0.0] for _ in range(s * s)]

    def paint(prim, clip):
        cr, cg, cb, ca = prim.get("fill", prim.get("color"))
        sa = ca / 255.0
        for py in range(s):
            for px in range(s):
                x, y = (px + 0.5) / scale, (py + 0.5) / scale
                if clip and not in_rrect(x, y, clip["x"], clip["y"], clip["w"], clip["h"], clip["r"]):
                    continue
                a = sa * cov(prim, x, y)
                if a <= 0:
                    continue
                d = buf[py * s + px]
                d[0] = cr / 255.0 * a + d[0] * (1 - a)
                d[1] = cg / 255.0 * a + d[1] * (1 - a)
                d[2] = cb / 255.0 * a + d[2] * (1 - a)
                d[3] = a + d[3] * (1 - a)

    paint(bg, None)
    for prim in fg:
        paint(prim, bg)  # clip the spool to the rounded square

    out = bytearray(size * size * 4)
    for oy in range(size):
        for ox in range(size):
            r = g = b = a = 0.0
            for dy in range(SS):
                for dx in range(SS):
                    p = buf[(oy * SS + dy) * s + (ox * SS + dx)]
                    r, g, b, a = r + p[0], g + p[1], b + p[2], a + p[3]
            n = SS * SS
            r, g, b, a = r / n, g / n, b / n, a / n
            off = (oy * size + ox) * 4
            if a > 0:
                out[off:off + 4] = bytes(
                    (min(255, round(r / a * 255)), min(255, round(g / a * 255)),
                     min(255, round(b / a * 255)), min(255, round(a * 255)))
                )
    return out


def write_png(path, size, pixels):
    def chunk(tag, data):
        return struct.pack(">I", len(data)) + tag + data + struct.pack(">I", zlib.crc32(tag + data))

    raw = bytearray()
    stride = size * 4
    for y in range(size):
        raw.append(0)
        raw.extend(pixels[y * stride:(y + 1) * stride])
    with open(path, "wb") as f:
        f.write(b"\x89PNG\r\n\x1a\n")
        f.write(chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)))
        f.write(chunk(b"IDAT", zlib.compress(bytes(raw), 9)))
        f.write(chunk(b"IEND", b""))


def main():
    src = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "src")
    bg, fg = spool()
    for size, name in [(32, "favicon-32.png"), (180, "apple-touch-icon.png")]:
        write_png(os.path.join(src, name), size, render(size, bg, fg))
        print("wrote", name)


if __name__ == "__main__":
    main()
