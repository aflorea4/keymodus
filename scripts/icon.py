"""Render the Keymodus app icon (no dependencies).

    python3 scripts/icon.py app/icons && iconutil -c icns app/icons/icon.iconset -o app/icons/icon.icns

A volt keycap with a hard offset shadow, stamped with a blocky K, on a carbon
squircle with a faint HUD grid.
"""
import math, os, struct, sys, zlib


def png(path, w, h, px):
    raw = b"".join(b"\x00" + bytes(px[y * w * 4:(y + 1) * w * 4]) for y in range(h))
    def chunk(t, d): return struct.pack(">I", len(d)) + t + d + struct.pack(">I", zlib.crc32(t + d) & 0xffffffff)
    open(path, "wb").write(b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
                           + chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b""))


def sd_rrect(x, y, cx, cy, hw, hh, r):
    qx = abs(x - cx) - hw + r; qy = abs(y - cy) - hh + r
    return math.hypot(max(qx, 0), max(qy, 0)) + min(max(qx, qy), 0) - r


def sd_seg(x, y, ax, ay, bx, by, w):
    px, py, dx, dy = x - ax, y - ay, bx - ax, by - ay
    t = max(0.0, min(1.0, (px * dx + py * dy) / (dx * dx + dy * dy)))
    # square caps: measure along the segment with a box
    return math.hypot(px - dx * t, py - dy * t) - w


def cov(d, aa): return max(0.0, min(1.0, 0.5 - d / aa))


def mix(c, d, a): return [c[i] * (1 - a) + d[i] * a for i in range(3)]


CARBON = (0.085, 0.088, 0.075)
BONE = (0.93, 0.93, 0.88)
VOLT = (0.87, 0.95, 0.12)
VOLT_DEEP = (0.36, 0.40, 0.05)
INK = (0.08, 0.08, 0.07)


def render(S):
    px = bytearray(S * S * 4); ss = 3 if S <= 64 else 2
    for j in range(S):
        for i in range(S):
            acc = [0.0] * 4
            for sy in range(ss):
                for sx in range(ss):
                    x = (i + (sx + 0.5) / ss) / S; y = (j + (sy + 0.5) / ss) / S
                    aa = 1.0 / S
                    a = cov(sd_rrect(x, y, .5, .5, .4023, .4023, .18), aa)  # macOS icon grid
                    if a <= 0: continue
                    c = list(CARBON)
                    # faint HUD grid (only where it reads)
                    if S >= 128:
                        g = min(abs((x - .5) * 10 - round((x - .5) * 10)), abs((y - .5) * 10 - round((y - .5) * 10))) / 10
                        c = mix(c, BONE, 0.07 * cov(g - 0.0012, aa))
                    # crop marks in the corners of the keycap area
                    if S >= 64:
                        for cx, cy, sxn, syn in [(.2, .2, 1, 1), (.8, .2, -1, 1), (.2, .8, 1, -1), (.8, .8, -1, -1)]:
                            m = min(sd_seg(x, y, cx, cy, cx + sxn * .07, cy, .006), sd_seg(x, y, cx, cy, cx, cy + syn * .07, .006))
                            c = mix(c, BONE, 0.55 * cov(m, aa))
                    # hard offset shadow, then the keycap
                    c = mix(c, VOLT_DEEP, cov(sd_rrect(x, y, .535, .535, .21, .21, .01), aa))
                    c = mix(c, VOLT, cov(sd_rrect(x, y, .48, .48, .21, .21, .01), aa))
                    # blocky K
                    k = min(
                        sd_rrect(x, y, .405, .48, .032, .125, 0),
                        sd_seg(x, y, .44, .50, .565, .36, .032),
                        sd_seg(x, y, .47, .465, .565, .605, .032),
                    )
                    if sd_rrect(x, y, .48, .48, .21, .21, .01) < 0:
                        k = max(k, sd_rrect(x, y, .48, .48, .16, .125, 0))
                    c = mix(c, INK, cov(k, aa))
                    for n in range(3): acc[n] += c[n] * a
                    acc[3] += a
            n = ss * ss; A = acc[3] / n; k = (j * S + i) * 4
            if A > 0:
                px[k:k + 4] = bytes([int(min(1, acc[0] / acc[3]) * 255 + .5), int(min(1, acc[1] / acc[3]) * 255 + .5),
                                     int(min(1, acc[2] / acc[3]) * 255 + .5), int(A * 255 + .5)])
    return px


out = sys.argv[1]
os.makedirs(out + "/icon.iconset", exist_ok=True)
cache = {s: render(s) for s in [16, 32, 64, 128, 256, 512, 1024]}
for base in [16, 32, 128, 256, 512]:
    png(f"{out}/icon.iconset/icon_{base}x{base}.png", base, base, cache[base])
    png(f"{out}/icon.iconset/icon_{base}x{base}@2x.png", base * 2, base * 2, cache[base * 2])
png(f"{out}/32x32.png", 32, 32, cache[32]); png(f"{out}/128x128.png", 128, 128, cache[128])
png(f"{out}/128x128@2x.png", 256, 256, cache[256]); png(f"{out}/icon.png", 512, 512, cache[512])


def render_tray(S):
    """Menu bar template: keycap outline with the K, black on transparent."""
    px = bytearray(S * S * 4); ss = 4
    for j in range(S):
        for i in range(S):
            a = 0.0
            for sy in range(ss):
                for sx in range(ss):
                    x = (i + (sx + 0.5) / ss) / S; y = (j + (sy + 0.5) / ss) / S
                    aa = 1.0 / S
                    frame = abs(sd_rrect(x, y, .5, .5, .40, .40, .02)) - .045
                    k = min(sd_rrect(x, y, .37, .5, .06, .22, 0),
                            sd_seg(x, y, .43, .53, .64, .28, .06), sd_seg(x, y, .47, .47, .64, .72, .06))
                    k = max(k, sd_rrect(x, y, .5, .5, .26, .22, 0))
                    a += cov(min(frame, k), aa)
            k = (j * S + i) * 4
            px[k:k + 4] = bytes([0, 0, 0, int(a / (ss * ss) * 255 + .5)])
    return px


png(f"{out}/tray.png", 36, 36, render_tray(36))
