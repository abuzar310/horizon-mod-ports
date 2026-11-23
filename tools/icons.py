"""Draws each mod's icon.png (128 x 128, RGBA) from simple shapes, with no image library: signed
distance shapes sampled 3 x 3 per pixel, then a plain PNG writer (zlib). Run from the repository root:
`python tools/icons.py` writes mods/<id>/icon.png for every mod below that has a folder."""
import math
import os
import struct
import zlib

SIZE = 128
SS = 3  # samples per pixel per axis


def hexc(h, a=255):
    h = h.lstrip("#")
    return (int(h[0:2], 16), int(h[2:4], 16), int(h[4:6], 16), a)


# --- shapes: each returns a signed distance (negative inside), in pixels -----------------------
def rrect(x0, y0, x1, y1, r):
    cx, cy, hw, hh = (x0 + x1) / 2, (y0 + y1) / 2, (x1 - x0) / 2 - r, (y1 - y0) / 2 - r

    def d(x, y):
        qx, qy = abs(x - cx) - hw, abs(y - cy) - hh
        return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r
    return d


def circle(cx, cy, r):
    return lambda x, y: math.hypot(x - cx, y - cy) - r


def ring(cx, cy, r, t):
    return lambda x, y: abs(math.hypot(x - cx, y - cy) - r) - t / 2


def arc(cx, cy, r, t, a0, a1):
    """A ring between angles a0..a1 in degrees, 0 = right, counter-clockwise, y up."""
    def d(x, y):
        ang = math.degrees(math.atan2(-(y - cy), x - cx)) % 360
        lo, hi = a0 % 360, a1 % 360
        inside = lo <= ang <= hi if lo <= hi else (ang >= lo or ang <= hi)
        return abs(math.hypot(x - cx, y - cy) - r) - t / 2 if inside else 1e9
    return d


def capsule(x0, y0, x1, y1, t):
    def d(x, y):
        vx, vy, wx, wy = x1 - x0, y1 - y0, x - x0, y - y0
        L = vx * vx + vy * vy
        k = 0.0 if L == 0 else max(0.0, min(1.0, (wx * vx + wy * vy) / L))
        return math.hypot(wx - k * vx, wy - k * vy) - t / 2
    return d


def poly(points):
    def d(x, y):
        inside = False
        n = len(points)
        for i in range(n):
            (ax, ay), (bx, by) = points[i], points[(i + 1) % n]
            if (ay > y) != (by > y) and x < (bx - ax) * (y - ay) / (by - ay) + ax:
                inside = not inside
        return -1.0 if inside else 1.0
    return d


def outline(shape_inner, shape_outer):
    """The band between two shapes: inside outer, outside inner."""
    return lambda x, y: max(shape_outer(x, y), -shape_inner(x, y))


# A 5 x 7 pixel font for the few characters icons use.
FONT = {
    "1": ["..#..", ".##..", "..#..", "..#..", "..#..", "..#..", ".###."],
    "2": [".###.", "#...#", "....#", "...#.", "..#..", ".#...", "#####"],
    "3": ["####.", "....#", "....#", ".###.", "....#", "....#", "####."],
}


def text_shapes(s, x, y, px):
    shapes = []
    for i, ch in enumerate(s):
        for r, row in enumerate(FONT[ch]):
            for c, cell in enumerate(row):
                if cell == "#":
                    x0, y0 = x + (i * 6 + c) * px, y + r * px
                    shapes.append(rrect(x0, y0, x0 + px, y0 + px, 0.01))
    return shapes


# --- rendering ---------------------------------------------------------------------------------
def render(layers):
    """layers: list of (shape, rgba) painted in order, or (shape, (top, bottom)) for a gradient."""
    out = bytearray()
    for py in range(SIZE):
        out.append(0)  # PNG filter byte
        for px in range(SIZE):
            acc = [0.0, 0.0, 0.0, 0.0]
            for sy in range(SS):
                for sx in range(SS):
                    x, y = px + (sx + 0.5) / SS, py + (sy + 0.5) / SS
                    col = (0.0, 0.0, 0.0, 0.0)
                    for shape, paint in layers:
                        if shape(x, y) < 0:
                            if isinstance(paint[0], tuple):
                                t = y / SIZE
                                a, b = paint
                                c = tuple(a[k] + (b[k] - a[k]) * t for k in range(4))
                            else:
                                c = paint
                            al = c[3] / 255
                            col = tuple(col[k] * (1 - al) + c[k] * al for k in range(3)) + (
                                al + col[3] * (1 - al),)
                    acc = [acc[k] + col[k] for k in range(3)] + [acc[3] + col[3]]
            n = SS * SS
            a = acc[3] / n
            rgb = [acc[k] / n / a if a > 0 else 0 for k in range(3)]
            out += bytes(int(max(0, min(255, round(v)))) for v in rgb) + bytes([int(round(a * 255))])
    return bytes(out)


def png(path, raw):
    def chunk(kind, data):
        c = struct.pack(">I", len(data)) + kind + data
        return c + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)
    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    open(path, "wb").write(data)
    return len(data)


def bg(top, bottom):
    return (rrect(4, 4, 124, 124, 24), (hexc(top), hexc(bottom)))


WHITE, DARK = hexc("#f4f4f4"), hexc("#1b1d24")

ICONS = {
    "speedometer": [
        bg("#1d4b57", "#0e2329"),
        (arc(64, 72, 40, 13, 150, 225), hexc("#3fbf5a")),
        (arc(64, 72, 40, 13, 62, 150), hexc("#f2c93a")),
        (arc(64, 72, 40, 13, 315, 62), hexc("#e04848")),
        (capsule(64, 72, 90, 46, 7), WHITE),
        (circle(64, 72, 9), WHITE),
        (circle(64, 72, 4), hexc("#0e2329")),
        (rrect(46, 98, 82, 108, 4), hexc("#f4f4f4", 70)),
    ],
    "hotbar_numbers": [
        bg("#3a3f4f", "#1e212b"),
        *[(rrect(14 + i * 34, 44, 46 + i * 34, 76, 4), hexc("#8b8f99")) for i in range(3)],
        *[(rrect(18 + i * 34, 48, 42 + i * 34, 72, 2), hexc("#2a2d36")) for i in range(3)],
        (outline(rrect(46, 42, 82, 78, 4), rrect(43, 39, 85, 81, 6)), WHITE),
        *[(s, WHITE) for s in text_shapes("1", 20, 50, 2)],
        *[(s, WHITE) for s in text_shapes("2", 54, 50, 2)],
        *[(s, WHITE) for s in text_shapes("3", 88, 50, 2)],
        (rrect(14, 88, 114, 96, 3), hexc("#5a5f6b")),
    ],
    "held_item_details": [
        bg("#3b2a5c", "#1c1430"),
        (capsule(40, 86, 88, 38, 12), hexc("#d9dde3")),
        (capsule(44, 82, 86, 40, 4), hexc("#ffffff")),
        (capsule(30, 70, 56, 96, 9), hexc("#8b5a2b")),
        (capsule(28, 98, 38, 88, 9), hexc("#5c3a1a")),
        (rrect(26, 104, 102, 114, 5), hexc("#14101f")),
        (rrect(28, 106, 78, 112, 3), hexc("#55e05a")),
    ],
    "speedrun_clock": [
        bg("#1f5a37", "#0d2817"),
        (rrect(56, 18, 72, 30, 3), WHITE),
        (capsule(84, 34, 92, 26, 6), WHITE),
        (circle(64, 72, 38), WHITE),
        (circle(64, 72, 31), hexc("#13331f")),
        (capsule(64, 72, 64, 48, 6), hexc("#7dff8a")),
        (capsule(64, 72, 80, 80, 5), WHITE),
        (circle(64, 72, 5), WHITE),
    ],
    "death_coords": [
        bg("#5a1e1e", "#2a0c0c"),
        (poly([(40, 60), (88, 60), (64, 108)]), hexc("#e84a4a")),
        (circle(64, 54, 26), hexc("#e84a4a")),
        (circle(64, 54, 11), WHITE),
        (capsule(58, 48, 70, 60, 3), hexc("#5a1e1e")),
        (capsule(70, 48, 58, 60, 3), hexc("#5a1e1e")),
    ],
    "chat_calculator": [
        bg("#1e3a6e", "#0c1a35"),
        (poly([(36, 84), (56, 84), (34, 104)]), WHITE),
        (rrect(20, 26, 108, 88, 18), WHITE),
        (capsule(42, 48, 86, 48, 9), hexc("#1e3a6e")),
        (capsule(42, 66, 86, 66, 9), hexc("#1e3a6e")),
    ],
    "rarity_slots": [
        bg("#33363d", "#16181c"),
        *[(rrect(14 + i * 34, 46, 46 + i * 34, 78, 3), hexc(c))
          for i, c in enumerate(["#ffff55", "#55ffff", "#ff55ff"])],
        *[(rrect(18 + i * 34, 50, 42 + i * 34, 74, 2), hexc("#24262c")) for i in range(3)],
        (poly([(30, 54), (36, 62), (30, 70), (24, 62)]), hexc("#ffff55")),
        (poly([(64, 54), (70, 62), (64, 70), (58, 62)]), hexc("#55ffff")),
        (poly([(98, 54), (104, 62), (98, 70), (92, 62)]), hexc("#ff55ff")),
    ],
    "sprint_sneak_toggle": [
        bg("#2c3e50", "#141d26"),
        (capsule(30, 34, 44, 46, 7), WHITE),
        (capsule(44, 46, 30, 58, 7), WHITE),
        (capsule(52, 34, 66, 46, 7), WHITE),
        (capsule(66, 46, 52, 58, 7), WHITE),
        (rrect(24, 70, 104, 102, 16), hexc("#3fbf5a")),
        (circle(88, 86, 12), WHITE),
    ],
    "block_outline": [
        bg("#262a33", "#101217"),
        (poly([(64, 26), (100, 44), (64, 62), (28, 44)]), hexc("#b9c0c8")),
        (poly([(28, 44), (64, 62), (64, 104), (28, 86)]), hexc("#7d848c")),
        (poly([(100, 44), (64, 62), (64, 104), (100, 86)]), hexc("#545a61")),
        *[(capsule(*e, 5), hexc("#38f0f0")) for e in [
            (64, 26, 100, 44), (100, 44, 64, 62), (64, 62, 28, 44), (28, 44, 64, 26),
            (28, 44, 28, 86), (100, 44, 100, 86), (64, 62, 64, 104),
            (28, 86, 64, 104), (100, 86, 64, 104)]],
    ],
    "xray_single_player": [
        bg("#1a1f2e", "#07090f"),
        *[(outline(rrect(20 + c * 22 + 2, 20 + r * 22 + 2, 20 + c * 22 + 20, 20 + r * 22 + 20, 2),
                   rrect(20 + c * 22, 20 + r * 22, 20 + c * 22 + 22, 20 + r * 22 + 22, 3)),
           hexc("#5c6370", 150)) for r in range(4) for c in range(4)],
        (rrect(44, 44, 62, 62, 3), hexc("#5fe6e6")),
        (rrect(66, 66, 84, 84, 3), hexc("#f5d742")),
        (rrect(22, 88, 40, 106, 3), hexc("#e03c3c")),
        (rrect(88, 22, 106, 40, 3), hexc("#3fd46b")),
        (rrect(88, 88, 106, 106, 3), hexc("#5fe6e6")),
    ],
}


def main():
    root = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
    for mod, layers in ICONS.items():
        folder = os.path.join(root, "mods", mod)
        if not os.path.isdir(folder):
            print(f"{mod}: no folder yet, skipped")
            continue
        size = png(os.path.join(folder, "icon.png"), render(layers))
        print(f"{mod}: icon.png {size} bytes")


if __name__ == "__main__":
    main()
