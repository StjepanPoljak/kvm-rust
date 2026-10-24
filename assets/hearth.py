"""Hearth title screen: 320x200, <=256 colours, mode 13h style."""
import sys
import numpy as np
from PIL import Image, ImageDraw, ImageFont

W, H = 320, 200
rng = np.random.default_rng(13)
yy, xx = np.mgrid[0:H, 0:W]


def ramp(*hexes):
    return [tuple(int(h[i:i + 2], 16) for i in (0, 2, 4)) for h in hexes]


R = {
    'stone': ramp('120c10', '1e1418', '2c1e20', '3e2a28', '533831', '6b4a3b', '86604a',
                  'a27a58', 'c09868', 'dcb880'),
    'wood': ramp('100804', '1e0f07', '30180b', '462410', '5e3216', '78421e', '935628',
                 'b06c34', 'cc8a46'),
    'fire': ramp('2a0604', '5a0e04', '8e1c06', 'c03408', 'e2560c', 'f48018', 'faa82a',
                 'fcd050', 'fff090', 'fffcdc'),
    'sky': ramp('04040e', '080a1e', '0e1232', '161c48', '20285e'),
    'moon': ramp('5a6078', '8a92a8', 'b8c0d0', 'e2e6e8', 'fcfcf0'),
    'rug': ramp('180406', '2c080c', '440e12', '601618', '7e2020', '9c2e24', 'b8442c',
                'd06636'),
    'gold': ramp('1c0c02', '3a1e04', '5e3408', '86500e', 'ae7016', 'd09422', 'eab838',
                 'f8d860', 'fff0a0', 'fffce0'),
    'iron': ramp('08080a', '141418', '22212a', '34323c', '4a464e', '645d60', '847a74'),
    'ginger': ramp('1a0a04', '341406', '54220a', '783612', '9a4e1c', 'bc6c2c', 'd88c44',
                   'eeb066'),
    'herb': ramp('0a0e04', '141c08', '22300c', '324412', '465a1a', '5e7424', '7a9032'),
    'green': ramp('041006', '082010', '10381a', '1c5626', '2c7a34', '48a048', '7ccc6a',
                  'c0f0a0'),
    'blue': ramp('04081a', '0a1434', '122454', '1c3a7a', '2c58a0', '4880c4', '7cb0e0',
                 'c8e8fc'),
    'violet': ramp('10041a', '200a32', '36124e', '501c6c', '6c2c8a', '8e44a8', 'b46cc8',
                   'e0a8ec'),
    'wax': ramp('2a1c10', '4a3420', '6e5034', '927050', 'b49470', 'd2b890', 'eadab4',
                'fcf4dc'),
    'bone': ramp('1c140c', '342818', '544428', '78663e', '9c8a5a', 'c0ae7c', 'ded0a4'),
    'paper': ramp('20140a', '3c2814', '5c4022', '7e5c34', 'a07c4c', 'c0a06a', 'dcc28e'),
}
NAMES = list(R)
M = {n: i for i, n in enumerate(NAMES)}

mat = np.full((H, W), M['stone'])
alb = np.zeros((H, W))
emis = np.zeros((H, W), bool)


def put(mask, m, a, e=False):
    mat[mask] = M[m]
    alb[mask] = a[mask] if isinstance(a, np.ndarray) else a
    emis[mask] = e


def edge(mask):
    """Pixels of mask with a 4-neighbour outside it."""
    p = np.pad(mask, 1)
    inner = p[:-2, 1:-1] & p[2:, 1:-1] & p[1:-1, :-2] & p[1:-1, 2:]
    return mask & ~inner


def dilate(mask, n=1):
    for _ in range(n):
        p = np.pad(mask, 1)
        mask = (p[1:-1, 1:-1] | p[:-2, 1:-1] | p[2:, 1:-1] | p[1:-1, :-2] | p[1:-1, 2:]
                | p[:-2, :-2] | p[:-2, 2:] | p[2:, :-2] | p[2:, 2:])
    return mask


def shift(mask, dx, dy):
    out = np.zeros_like(mask)
    out[max(dy, 0):H + min(dy, 0), max(dx, 0):W + min(dx, 0)] = \
        mask[max(-dy, 0):H + min(-dy, 0), max(-dx, 0):W + min(-dx, 0)]
    return out


def ellipse(cx, cy, rx, ry):
    return ((xx - cx) / rx) ** 2 + ((yy - cy) / ry) ** 2 <= 1


def rect(x0, y0, x1, y1):
    return (xx >= x0) & (xx < x1) & (yy >= y0) & (yy < y1)


def blocks(region, x0, y0, rows, wmin, wmax, base, spread, seed):
    """Rough stone courses: bevelled blocks separated by mortar."""
    r = np.random.default_rng(seed)
    a = np.zeros((H, W))
    y = y0
    for rh in rows:
        x = x0 - r.integers(0, wmax)
        while x < W:
            bw = int(r.integers(wmin, wmax + 1))
            v = base + r.uniform(-spread, spread)
            b = rect(x + 1, y + 1, x + bw, y + rh)
            a[b] = v
            a[rect(x + 1, y + 1, x + bw, y + 2) & b] = v + 0.14       # top bevel
            a[rect(x + 1, y + 1, x + 2, y + rh) & b] = v + 0.08       # left bevel
            a[rect(x + 1, y + rh - 1, x + bw, y + rh) & b] = v - 0.16  # bottom
            a[rect(x + bw - 1, y + 1, x + bw, y + rh) & b] = v - 0.12  # right
            x += bw
        y += rh
    a = np.where(a == 0, 0.12, a)                                     # mortar
    a += r.normal(0, 0.035, (H, W))
    a[r.random((H, W)) < 0.025] -= 0.12                               # pits
    put(region, 'stone', a)


# ---------------------------------------------------------------- back wall
blocks(np.ones((H, W), bool), 0, 6, [11, 12, 10, 12, 11, 12, 10, 12, 11, 12, 11, 12, 13],
       14, 28, 0.52, 0.09, 1)

# ceiling beam
beam = rect(0, 0, W, 7)
put(beam, 'wood', 0.55 + 0.1 * np.sin(xx / 7.0) * np.sin(xx / 23.0) - 0.25 * (yy == 6))

# --------------------------------------------------------------------- floor
FY = 152
floor = yy >= FY
VX, VY = 160, 40
fa = np.full((H, W), 0.5)
t = (yy - VY) / (H - VY)
u = (xx - VX) / np.maximum(t, 1e-3)              # board coordinate
board = np.floor(u / 26).astype(int)
r = np.random.default_rng(5)
bv = r.uniform(0.42, 0.62, 400)
fa = bv[(board + 200) % 400]
fa = fa + 0.05 * np.sin(u / 3.1 + board * 1.7) * np.sin(yy * 0.9 + board)   # grain
seam = np.abs(((u + 13) % 26) - 13) > 13 - 1.1 / np.maximum(t, 0.05) * 0.5
fa = np.where(seam, 0.1, fa)
for b in range(-12, 13):                          # butt joints
    jy = int(r.integers(FY + 4, H))
    fa = np.where((board == b) & (yy == jy), 0.12, fa)
put(floor, 'wood', fa)
put(rect(0, FY - 3, W, FY), 'wood', 0.3 - 0.12 * (yy == FY - 1))   # skirting

# ---------------------------------------------------------------- fireplace
CX = 160
breast = rect(110, 7, 210, 76)
blocks(breast, 110, 7, [11, 12, 11, 12, 11, 12], 16, 30, 0.6, 0.08, 7)
put(edge(breast) & (xx == 110) | rect(108, 7, 110, 76), 'stone', 0.08)
put(rect(210, 7, 213, 76), 'stone', 0.08)

surround = rect(112, 84, 208, FY)
blocks(surround, 112, 84, [10, 11, 10, 11, 10, 12, 4], 12, 22, 0.66, 0.07, 9)
put(rect(110, 84, 112, FY) | rect(208, 84, 211, FY), 'stone', 0.08)

# mantel beam with brackets
mantel = rect(98, 74, 222, 84)
ma = 0.6 + 0.06 * np.sin(xx / 5.0 + np.sin(xx / 17.0) * 3)
ma = np.where(yy == 74, 0.85, ma)
ma = np.where(yy >= 82, 0.25, ma)
put(mantel, 'wood', ma)
put(edge(mantel), 'wood', 0.05)
for bx in (114, 200):
    br = rect(bx, 84, bx + 6, 96) & (yy - 84 < 12 - (xx - bx) * 0)
    br &= ~((yy > 88) & ((xx - bx) < (yy - 88) * 0.75 - 0.5))
    put(br, 'wood', 0.45)
    put(edge(br), 'wood', 0.05)

# Tudor arch opening
OL, OR, OT = 128, 192, 96
arch_top = OT + ((xx - CX) / 32.0) ** 4 * 10
opening = (xx >= OL) & (xx < OR) & (yy >= arch_top) & (yy < FY)
# voussoirs: radial joints around the arch
ring = (xx >= OL - 6) & (xx < OR + 6) & (yy >= arch_top - 7) & (yy < arch_top + 0.5) & (yy < 112)
ang = np.arctan2(yy - 160, xx - CX)
va = 0.72 + 0.05 * np.sin(ang * 60)
va = np.where(np.abs(np.sin(ang * 22)) < 0.12, 0.12, va)
put(ring, 'stone', va)
put(opening, 'stone', 0.0)
# soot-dark firebox with faint brick
fb = opening.copy()
bri = 0.16 + 0.04 * ((yy // 5 + (xx + (yy // 5) % 2 * 4) // 8) % 2)
bri = np.where((yy % 5 == 0) | ((xx + (yy // 5) % 2 * 4) % 8 == 0), 0.07, bri)
put(fb, 'stone', bri)
put(edge(opening), 'stone', 0.03)

# hearthstone
hs = rect(100, FY, 220, FY + 7)
put(hs, 'stone', np.where(yy == FY, 0.9, 0.62 - 0.04 * (yy - FY)) + 0.04 * ((xx // 20) % 2))
put(edge(hs) & (yy > FY), 'stone', 0.1)

# andirons and logs
for ax in (138, 180):
    put(rect(ax, 136, ax + 3, FY), 'iron', 0.6)
    put(ellipse(ax + 1, 135, 2.5, 2.5), 'iron', 0.8)
    put(rect(ax - 3, FY - 2, ax + 6, FY), 'iron', 0.4)


def log(x0, y0, x1, y1, r_):
    d = np.abs((y1 - y0) * xx - (x1 - x0) * yy + x1 * y0 - y1 * x0) / np.hypot(x1 - x0, y1 - y0)
    tt = ((xx - x0) * (x1 - x0) + (yy - y0) * (y1 - y0)) / ((x1 - x0) ** 2 + (y1 - y0) ** 2)
    m = (d <= r_) & (tt >= 0) & (tt <= 1)
    side = ((y1 - y0) * xx - (x1 - x0) * yy + x1 * y0 - y1 * x0)
    a = 0.5 - 0.25 * (side > 0) * (d / r_) + 0.06 * np.sin(tt * 40)
    put(m, 'wood', a)
    put(edge(m), 'wood', 0.04)
    end = ellipse(x1, y1, r_ * 0.7, r_)
    put(end, 'wood', 0.75 - 0.3 * ((np.hypot(xx - x1, yy - y1) % 2) < 1))
    put(edge(end), 'wood', 0.1)
    # embers glowing in the bark cracks
    cr = m & (np.sin(tt * 55 + d * 2) > 0.93) & (side < 0)
    put(cr, 'fire', 0.55, True)


log(132, 147, 186, 143, 4.5)
log(140, 141, 184, 149, 4)
put(ellipse(160, 149, 22, 3) & opening, 'fire', 0.35, True)            # ember bed
put(ellipse(160, 149, 22, 3) & opening & (rng.random((H, W)) < 0.35), 'fire', 0.7, True)

# flames: one central mass broken into tongues by stretched value noise
def vnoise(sx, sy, seed):
    g = np.random.default_rng(seed).random((H // sy + 3, W // sx + 3))
    fx, fy = xx / sx, yy / sy
    x0, y0 = np.floor(fx).astype(int), np.floor(fy).astype(int)
    tx, ty = fx - x0, fy - y0
    tx, ty = tx * tx * (3 - 2 * tx), ty * ty * (3 - 2 * ty)
    a = g[y0, x0] * (1 - tx) + g[y0, x0 + 1] * tx
    b = g[y0 + 1, x0] * (1 - tx) + g[y0 + 1, x0 + 1] * tx
    return a * (1 - ty) + b * ty


base = 147
v = np.clip((base - yy) / 52.0, 0, 2)
n1 = vnoise(7, 13, 21)
n2 = vnoise(3, 6, 22)
dx = xx - 160 + (n1 - 0.5) * 10 * v + np.sin(v * 7) * 3 * v
width = 25 * (1 - v * 0.6) * np.clip(v * 6 + 0.35, 0, 1)
shape = np.clip(1 - np.abs(dx) / np.maximum(width, 1e-3), 0, 1) ** 0.7
flame = shape * (1.15 - v + (n1 - 0.5) * 1.3 * v + (n2 - 0.5) * 0.45)
flame = np.clip(flame, 0, 1) * (yy <= base)
fl = (flame > 0.12) & opening
put(fl, 'fire', np.clip(0.1 + flame * 1.15, 0, 1), True)
# sparks
for _ in range(16):
    sx, sy = int(rng.integers(140, 182)), int(rng.integers(98, 112))
    if opening[sy, sx]:
        put((xx == sx) & (yy == sy), 'fire', rng.uniform(0.6, 0.95), True)

# --------------------------------------------------------------- the window
WX0, WX1, WY0, WY1 = 24, 74, 44, 120
wcx = (WX0 + WX1) / 2
wr = (WX1 - WX0) / 2
win = rect(WX0, WY0 + wr, WX1, WY1) | (ellipse(wcx, WY0 + wr, wr, wr) & (yy < WY0 + wr))
frame = dilate(win, 4) & ~win
put(frame, 'stone', 0.7 + 0.05 * np.sin(np.arctan2(yy - WY0 - wr, xx - wcx) * 30))
put(edge(dilate(win, 4)), 'stone', 0.1)
sky = np.clip((yy - WY0) / (WY1 - WY0), 0, 1)
put(win, 'sky', 0.15 + sky * 0.75, True)
for _ in range(26):
    sx, sy = int(rng.integers(WX0, WX1)), int(rng.integers(WY0, WY1 - 20))
    if win[sy, sx]:
        put((xx == sx) & (yy == sy), 'moon', rng.uniform(0.4, 0.9), True)
moon = ellipse(57, 62, 7, 7)
put(moon & win, 'moon', 0.95 - 0.35 * ellipse(54.5, 60, 6.5, 6.5), True)
put(moon & win & (np.hypot(xx - 59, yy - 64) < 1.6), 'moon', 0.6, True)
# distant hills and a crooked tower
hill = win & (yy > 104 + 4 * np.sin(xx / 9.0) + 2 * np.sin(xx / 4.0))
put(hill, 'sky', 0.02, True)
tower = win & rect(36, 92, 42, 120)
tower |= win & (yy >= 86 + np.abs(xx - 39) * 1.6) & rect(34, 84, 45, 93)
put(tower, 'sky', 0.0, True)
put(win & (xx == 39) & (yy == 98), 'fire', 0.85, True)                    # lit window
# leaded diamond lattice
lat = win & ((((xx + yy) % 10) == 0) | (((xx - yy) % 10) == 0))
put(lat, 'iron', 0.35)
put(win & ((xx == int(wcx)) | (yy == 82)), 'iron', 0.4)
# sill
sill = rect(WX0 - 7, WY1 + 4, WX1 + 7, WY1 + 9)
put(sill, 'stone', np.where(yy == WY1 + 4, 0.92, 0.6))
put(edge(sill), 'stone', 0.08)

# --------------------------------------------------------- candles on mantel
def candle(cx_, top, h_):
    c = rect(cx_ - 2, top, cx_ + 3, top + h_)
    put(c, 'wax', 0.72 - 0.35 * (xx > cx_) + 0.2 * (xx == cx_ - 1))
    put(edge(c), 'wax', 0.12)
    drip = rect(cx_ - 2, top, cx_ + 3, top + 2) | ((xx == cx_ + 1) & (yy >= top) & (yy < top + 4))
    put(drip, 'wax', 0.95)
    put(rect(cx_ - 4, top + h_, cx_ + 5, top + h_ + 2), 'iron', 0.55)
    put((xx == cx_) & (yy == top - 1), 'iron', 0.1)
    fl_ = ellipse(cx_, top - 4, 1.6, 3.2)
    put(fl_, 'fire', 0.75, True)
    put(ellipse(cx_, top - 3, 0.7, 1.6), 'fire', 1.0, True)


candle(118, 61, 13)
candle(203, 64, 10)

# tankard and a small hourglass on the mantel
tk = rect(130, 64, 138, 74)
put(tk, 'iron', 0.62 - 0.3 * (xx > 134) + 0.25 * (xx == 131))
put(edge(tk), 'iron', 0.08)
hnd = (np.abs(np.hypot(xx - 139, yy - 69) - 3) < 0.8) & (xx > 138)
put(hnd, 'iron', 0.45)
hg = rect(187, 62, 196, 64) | rect(187, 72, 196, 74)
glass = (np.abs(xx - 191.5) <= 1 + np.abs(yy - 68) * 0.75) & rect(187, 64, 196, 72)
put(glass, 'blue', 0.55)
put(glass & (yy > 69) & (np.abs(xx - 191.5) < np.abs(yy - 68) * 0.75), 'paper', 0.85)
put(glass & (yy < 67) & (np.abs(xx - 191.5) < 2.5 - (67 - yy) * 0.5), 'paper', 0.8)
put(edge(glass), 'blue', 0.3)
put(hg, 'wood', 0.55)

# ------------------------------------------------------- shelves on the right
for sy in (88, 118):
    sh = rect(238, sy, 310, sy + 4)
    put(sh, 'wood', np.where(yy == sy, 0.85, 0.5))
    put(edge(sh), 'wood', 0.06)
    for bx in (246, 300):
        br = rect(bx, sy + 4, bx + 3, sy + 10) & ((yy - sy - 4) < (bx + 3 - xx) * 2)
        put(br, 'wood', 0.35)


def flask(cx_, bottom, r_, neck, col):
    body = ellipse(cx_, bottom - r_, r_, r_)
    nk = rect(cx_ - 1, bottom - 2 * r_ - neck, cx_ + 2, bottom - 2 * r_ + 1)
    m = body | nk
    liq = body & (yy > bottom - r_ * 1.2)
    put(m, col, 0.35 - 0.12 * (xx > cx_))
    put(liq, col, 0.7 - 0.25 * ((xx - cx_) / r_) - 0.1 * ((yy - bottom + r_) / r_))
    put(edge(m), col, 0.08)
    put((xx == cx_ - r_ // 2) & (yy == bottom - r_ - r_ // 2), col, 1.0)   # glint
    put(rect(cx_ - 1, bottom - 2 * r_ - neck - 2, cx_ + 2, bottom - 2 * r_ - neck), 'wood', 0.6)


flask(250, 88, 6, 3, 'green')
flask(266, 88, 4, 4, 'violet')
tall = rect(276, 72, 282, 88)
put(tall, 'blue', 0.55 - 0.25 * (xx > 279))
put(rect(276, 72, 282, 78), 'blue', 0.3)
put(edge(tall), 'blue', 0.08)
put(rect(277, 69, 281, 72), 'wood', 0.55)
# skull
sk = ellipse(296, 80, 6, 5.5) | rect(292, 82, 301, 88)
put(sk, 'bone', 0.75 - 0.3 * (xx > 298) - 0.15 * (yy > 85))
put(edge(sk), 'bone', 0.1)
put(ellipse(293.5, 81, 1.6, 1.6) | ellipse(298.5, 81, 1.6, 1.6), 'bone', 0.02)
put((xx == 296) & (yy == 84), 'bone', 0.05)
put(sk & (yy == 87) & (xx % 2 == 0), 'bone', 0.15)
# books
bx = 241
for wb, hb, col in [(5, 22, 'rug'), (4, 19, 'green'), (6, 24, 'blue'), (4, 17, 'paper'),
                    (5, 21, 'violet'), (6, 20, 'rug')]:
    b = rect(bx, 118 - hb, bx + wb, 118)
    put(b, col, 0.5 - 0.2 * (xx == bx + wb - 1) + 0.15 * (xx == bx))
    put(b & ((yy == 118 - hb + 3) | (yy == 118 - 4)), 'gold', 0.7)
    put(edge(b), col, 0.06)
    bx += wb
lean = (xx - 272) * 0.5 + (118 - yy) * 0.0
b = (yy < 118) & (yy >= 100) & (xx - 271 >= (118 - yy) * 0.45) & (xx - 277 < (118 - yy) * 0.45)
put(b, 'wax', 0.45)
put(edge(b), 'wax', 0.08)
# scroll
sc = rect(282, 112, 304, 118)
put(sc, 'paper', 0.7 - 0.3 * (yy > 115))
put(ellipse(282, 115, 2, 3) | ellipse(304, 115, 2, 3), 'paper', 0.45)
put(edge(sc), 'paper', 0.1)

# ------------------------------------------------------------------- barrel
bar = (np.abs(xx - 274) <= 18 - ((yy - 140) / 14.0) ** 2 * 3) & rect(250, 126, 300, FY)
stave = np.abs(((xx - 274) * 1.0) % 7) < 0.8
ba = 0.55 - 0.3 * ((xx - 262) / 30.0) ** 2 * (xx > 262) + 0.1 * (xx < 264)
ba = np.where(stave, 0.18, ba)
put(bar, 'wood', ba)
for hy in (131, 146):
    put(bar & (np.abs(yy - hy) <= 1), 'iron', 0.5 - 0.2 * (yy == hy + 1))
put(edge(bar), 'wood', 0.04)
lid = ellipse(274, 127, 15, 2.5)
put(lid, 'wood', 0.7)
put(edge(lid), 'wood', 0.1)

# ------------------------------------------------------------- herb bundles
hn = vnoise(2, 2, 31)
for hx, hl in [(36, 18), (52, 24), (68, 15), (252, 20), (268, 14), (290, 22)]:
    put((xx == hx) & (yy >= 6) & (yy < 10), 'paper', 0.4)
    q = (yy - 13) / hl
    prof = np.where(q < 0.6, 1.2 + q * 7, 1.2 + (1 - q) / 0.4 * 4.2)
    prof = prof * (0.75 + 0.5 * hn)
    bundle = (q >= 0) & (q <= 1) & (np.abs(xx - hx + np.sin(q * 3 + hx) * 1.5) <= prof)
    put(bundle, 'herb', 0.3 + 0.45 * hn - 0.15 * q + 0.2 * (xx < hx))
    put(edge(bundle), 'herb', 0.05)
    stems = (yy >= 10) & (yy < 14) & (np.abs(xx - hx) <= 1)
    put(stems, 'herb', 0.35)
    put(rect(hx - 2, 10, hx + 3, 12), 'paper', 0.65)

# ------------------------------------------------------------- firewood pile
for (lx, ly, lr) in [(80, 148, 5), (92, 148, 5), (104, 148, 5), (86, 139, 5), (98, 139, 5),
                     (92, 130, 5)]:
    rr = np.hypot(xx - lx, yy - ly)
    e = rr <= lr
    put(e, 'wood', np.where(rr > lr - 1.4, 0.3, 0.85 - 0.12 * (xx > lx)))
    put(e & (np.abs(rr - 2.4) < 0.5), 'wood', 0.6)
    put(e & (rr < 0.8), 'wood', 0.5)
    put(edge(e), 'wood', 0.04)

# ----------------------------------------------------------------- the rug
rug = ellipse(160, 178, 74, 15)
rd = ((xx - 160) / 74) ** 2 + ((yy - 178) / 15) ** 2
ra = np.where(rd > 0.78, 0.82, 0.48)
ra = np.where((rd > 0.62) & (rd <= 0.78), 0.3, ra)
motif = (np.abs(xx - 160) / 30 + np.abs(yy - 178) / 6) < 1
ra = np.where(motif, 0.78, ra)
ra = np.where(motif & ((np.abs(xx - 160) / 18 + np.abs(yy - 178) / 3.6) < 1), 0.4, ra)
ra = np.where((rd > 0.62) & (rd <= 0.78) & (((xx // 4) + (yy // 2)) % 2 == 0), 0.6, ra)
put(rug, 'rug', ra)
fr = rug & (rd > 0.94)
put(fr & (xx % 2 == 0), 'gold', 0.55)
put(edge(rug), 'rug', 0.05)

# ------------------------------------------------------------------ the cat
CXc, CYc = 212, 173
body = ellipse(CXc, CYc, 18, 8)
head = ellipse(CXc - 17, CYc - 4, 7, 6)
ear1 = (yy >= CYc - 14) & (yy <= CYc - 8) & (np.abs(xx - (CXc - 21)) <= (yy - (CYc - 14)) * 0.55)
ear2 = (yy >= CYc - 14) & (yy <= CYc - 8) & (np.abs(xx - (CXc - 13)) <= (yy - (CYc - 14)) * 0.55)
tail = (np.abs(np.hypot((xx - CXc + 3) / 1.0, (yy - CYc - 1) / 0.5) - 19) < 1.8) & (yy > CYc + 2) & (xx > CXc - 20)
cat = body | head | ear1 | ear2 | tail
lit = 0.7 - 0.32 * ((xx - (CXc - 20)) / 38) - 0.28 * ((yy - CYc + 9) / 17)
stripes = (body & (np.sin((xx - CXc) * 0.75) > 0.5) & (yy < CYc + 3)) | (head & (yy == CYc - 8) & (np.abs(xx - CXc + 17) <= 2) & (xx % 2 == 0))
put(cat, 'ginger', np.clip(lit, 0.1, 0.9))
put(stripes, 'ginger', np.clip(lit - 0.22, 0.05, 1))
put(edge(cat), 'ginger', 0.03)
put((yy == CYc - 4) & ((xx == CXc - 21) | (xx == CXc - 20) | (xx == CXc - 15) | (xx == CXc - 14)),
    'ginger', 0.02)                                                       # closed eyes
put((xx >= CXc - 18) & (xx <= CXc - 17) & (yy == CYc - 2), 'rug', 0.7)                      # nose
# ---------------------------------------------------------------- lighting
d = np.hypot((xx - 160) * 1.0, (yy - 132) * 1.15)
L = 0.13 + 1.3 / (1 + (d / 72) ** 2)
for cx_, cy_ in ((118, 56), (203, 59)):
    L += 0.22 / (1 + (np.hypot(xx - cx_, yy - cy_) / 14) ** 2)
L += 0.12 * np.exp(-((xx - 49) ** 2 / 900 + (yy - 135) ** 2 / 80))      # moon on the floor
# soft occlusion under the mantel and inside the firebox edges
L -= 0.18 * rect(108, 84, 212, 92)
L = np.where(rect(0, 0, W, 7), L * 0.7, L)

# ------------------------------------------------------------------- title
font = ImageFont.truetype('/usr/share/fonts/opentype/mathjax/MathJax_Fraktur-Bold.otf', 46)
ti = Image.new('L', (W, 80), 0)
ImageDraw.Draw(ti).text((0, 0), 'Hearth', font=font, fill=255)
ta = np.array(ti) > 110
ys, xs = np.nonzero(ta)
ta = ta[ys.min():ys.max() + 1, xs.min():xs.max() + 1]
th, tw = ta.shape
TX, TY = (W - tw) // 2, 10
T = np.zeros((H, W), bool)
T[TY:TY + th, TX:TX + tw] = ta
O = dilate(T) & ~T
S = (shift(T | O, 2, 2) | shift(T | O, 1, 2) | shift(T | O, 2, 1)) & ~(T | O)
put(S, 'stone', 0.0, True)
put(O, 'gold', 0.02, True)
gy = (yy - TY) / th
ga = 0.92 - 0.6 * gy
ga = np.where(np.abs(gy - 0.3) < 0.035, 1.0, ga)                         # gilt band
ga = np.where(edge(T) & shift(~T, 0, 1), np.minimum(ga + 0.2, 1), ga)     # top light
ga = np.where(edge(T) & shift(~T, 0, -1), ga - 0.25, ga)                  # bottom dark
put(T, 'gold', np.clip(ga, 0, 1), True)
# a few sparkles on the gilt
for (sx, sy) in [(TX + 16, TY + 4), (TX + tw - 14, TY + 3)]:
    put(((xx == sx) & (np.abs(yy - sy) <= 2)) | ((yy == sy) & (np.abs(xx - sx) <= 2)),
        'gold', 1.0, True)

# ---------------------------------------------------------------- quantize
bayer = np.array([[0, 8, 2, 10], [12, 4, 14, 6], [3, 11, 1, 9], [15, 7, 13, 5]]) / 16.0
bt = bayer[yy % 4, xx % 4]
val = np.where(emis, alb, alb * L)
val = np.clip(val, 0, 1)

offsets, pal = [], []
for n in NAMES:
    offsets.append(len(pal))
    pal += R[n]
assert len(pal) <= 256, len(pal)
offs = np.array(offsets)
sizes = np.array([len(R[n]) for n in NAMES])
n = sizes[mat]
idx = np.clip(np.floor(val * (n - 1) + bt * 0.85 + 0.075), 0, n - 1).astype(int)
out = (offs[mat] + idx).astype(np.uint8)

img = Image.fromarray(out, 'P')
flat = [c for rgb in pal for c in rgb]
img.putpalette(flat + [0] * (768 - len(flat)))
img.save(sys.argv[1])
img.resize((W * 4, H * 4), Image.NEAREST).save(sys.argv[2])
print('colours used:', len(np.unique(out)), 'of', len(pal))
