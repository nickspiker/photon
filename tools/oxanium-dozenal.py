#!/usr/bin/env python3
"""Oxanium, variable, with photon's dozenal digits (Nick 2026-10-14: "copy the dozenal over and get it all variable please").

Input: Oxanium's variable font (wght 200..800, default 200 = ExtraLight) and the static Regular+glyphs cut that carries the twelve dozenal digits in the reserved control block U+0010..U+001B (DLE..ESC = digits 0..11).
Output: one variable font carrying both — assets/Oxanium/Oxanium-VariableFont_wght+glyphs.ttf.

The digits were drawn once, at Regular, with a stroke a little heavier than Oxanium's Latin (their bar, the dozenal zero, is 100 units against the 'I' stem's 84). Their other weights are SYNTHESIZED the way FreeType emboldens an outline: every point moves along the bisector of its two edges' outward normals (a miter, limited), by a per-side amount measured from Oxanium's own plain bar 'I' at 200 / 400 / 800 and scaled by the digits' own stroke over the Latin one — so a dozenal stroke keeps the same proportion to a Latin stem at every weight (an absolute shift left the ExtraLight digits 38% heavier than their Latin neighbours). Regular (wght 400) is the drawing itself, untouched: the default master is the 200 form, one tuple peaks at 400 with the drawing, a second runs 400 -> 800. Advances stay fixed (the digits are 1000-unit tabular cells).

Needs fontTools (pip install fonttools). Run from the repo root: python3 tools/oxanium-dozenal.py [--proof proof.png]
"""

import copy
import math
import sys

from fontTools.pens.boundsPen import BoundsPen
from fontTools.ttLib import TTFont
from fontTools.ttLib.tables._g_l_y_f import GlyphCoordinates
from fontTools.ttLib.tables.TupleVariation import TupleVariation
from fontTools.varLib.instancer import instantiateVariableFont
from fontTools.varLib.models import normalizeValue, piecewiseLinearMap

VAR = "assets/Oxanium/Oxanium-VariableFont_wght.ttf"
GLYPHS = "assets/Oxanium/Oxanium-Regular+glyphs.ttf"
OUT = "assets/Oxanium/Oxanium-VariableFont_wght+glyphs.ttf"
DOZENAL = range(0x10, 0x1C)


def stem(weight):
    """Width of Oxanium's plain bar 'I' at a weight: the stroke the digits follow."""
    f = instantiateVariableFont(TTFont(VAR), {"wght": weight})
    gs = f.getGlyphSet()
    bp = BoundsPen(gs)
    gs[f.getBestCmap()[ord("I")]].draw(bp)
    return bp.bounds[2] - bp.bounds[0]


def dozenal_stroke():
    """The digits' own stroke at Regular: the height of the dozenal zero, a plain horizontal bar."""
    f = TTFont(GLYPHS)
    gs = f.getGlyphSet()
    bp = BoundsPen(gs)
    gs[f.getBestCmap()[0x10]].draw(bp)
    return bp.bounds[3] - bp.bounds[1]


def orientation(coords, end_pts):
    """+1 for clockwise outer contours (TrueType's rule), -1 for counter-clockwise: the sign that makes (-dy, dx) point away from the ink."""
    area, start = 0.0, 0
    for end in end_pts:
        pts = coords[start : end + 1]
        for i in range(len(pts)):
            x0, y0 = pts[i]
            x1, y1 = pts[(i + 1) % len(pts)]
            area += x0 * y1 - x1 * y0
        start = end + 1
    return 1 if area < 0 else -1


def contour_area(pts):
    a = 0.0
    for i in range(len(pts)):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % len(pts)]
        a += x0 * y1 - x1 * y0
    return a / 2


# A COUNTER grows inward at half the rate when the weight goes up (the first proof, 2026-10-14: at 800 the small loops of 2, 5 and 8 closed into blobs) — heavy weights gain their ink mostly on the outside, as Oxanium's own 8 does.
COUNTER_GAIN = 0.5


def embolden(coords, end_pts, s, sign):
    """Move every point `s` units away from the ink along its miter (negative = into the ink), the miter limited to 2x s; a counter (a contour wound against the glyph) moves only COUNTER_GAIN of a positive `s`."""
    out = [tuple(c) for c in coords]
    start = 0
    for end in end_pts:
        pts = [tuple(c) for c in coords[start : end + 1]]
        n = len(pts)
        # Outer contours wind with the glyph's sign; a counter winds against it.
        is_counter = (contour_area(pts) < 0) != (sign > 0)
        sc = s * COUNTER_GAIN if (is_counter and s > 0) else s
        for i in range(n):
            j = (i - 1) % n
            while pts[j] == pts[i] and j != i:
                j = (j - 1) % n
            k = (i + 1) % n
            while pts[k] == pts[i] and k != i:
                k = (k + 1) % n
            (px, py), (cx, cy), (nx, ny) = pts[j], pts[i], pts[k]
            ix, iy, ox, oy = cx - px, cy - py, nx - cx, ny - cy
            li, lo = math.hypot(ix, iy), math.hypot(ox, oy)
            if li == 0 or lo == 0:
                continue
            ix, iy, ox, oy = ix / li, iy / li, ox / lo, oy / lo
            n1 = (-iy * sign, ix * sign)
            n2 = (-oy * sign, ox * sign)
            mx, my = n1[0] + n2[0], n1[1] + n2[1]
            ml = math.hypot(mx, my)
            if ml < 1e-6:
                continue  # a full reversal: leave the point where it is
            mx, my = mx / ml, my / ml
            c = max(mx * n1[0] + my * n1[1], 0.5)  # cos of half the turn; 0.5 = miter limit 2
            d = sc / c
            out[start + i] = (cx + mx * d, cy + my * d)
        start = end + 1
    return [(round(x), round(y)) for x, y in out]


def build():
    var = TTFont(VAR)
    src = TTFont(GLYPHS)
    s200, s400, s800 = stem(200), stem(400), stem(800)
    ratio = dozenal_stroke() / s400
    off200, off800 = ratio * (s200 - s400) / 2, ratio * (s800 - s400) / 2
    print(f"stem 'I': {s200:.0f} / {s400:.0f} / {s800:.0f} at 200 / 400 / 800; dozenal stroke {ratio:.2f}x -> per side {off200:+.1f} / 0 / {off800:+.1f}")

    axis = next(a for a in var["fvar"].axes if a.axisTag == "wght")
    n = normalizeValue(400, (axis.minValue, axis.defaultValue, axis.maxValue))
    p_reg = piecewiseLinearMap(n, var["avar"].segments["wght"]) if "avar" in var else n
    p_reg = round(p_reg * 16384) / 16384  # F2Dot14, as the tuple stores it

    src_cmap = src.getBestCmap()
    order = var.getGlyphOrder()[:]
    glyf, hmtx, gvar = var["glyf"], var["hmtx"], var["gvar"]
    added = []
    for cp in DOZENAL:
        name = src_cmap[cp]
        if name in order:
            sys.exit(f"{name} already in the variable font")
        g = copy.deepcopy(src["glyf"][name])
        if g.isComposite():
            sys.exit(f"{name} is a composite — not handled")
        coords = [tuple(c) for c in g.coordinates]
        ends = list(g.endPtsOfContours)
        sign = orientation(coords, ends)
        c200 = embolden(coords, ends, off200, sign)
        c800 = embolden(coords, ends, off800, sign)
        reg = [(round(x), round(y)) for x, y in coords]
        g.coordinates = GlyphCoordinates(c200)
        g.program = copy.deepcopy(g.program) if hasattr(g, "program") else g.program
        order.append(name)
        var.setGlyphOrder(order)
        glyf.glyphOrder = order
        glyf.glyphs[name] = g
        g.recalcBounds(glyf)
        hmtx.metrics[name] = (src["hmtx"][name][0], g.xMin)
        phantom = [(0, 0)] * 4  # origin and advance fixed at every weight
        d_reg = [(a[0] - b[0], a[1] - b[1]) for a, b in zip(reg, c200)] + phantom
        d_800 = [(a[0] - b[0], a[1] - b[1]) for a, b in zip(c800, c200)] + phantom
        gvar.variations[name] = [
            TupleVariation({"wght": (0.0, p_reg, 1.0)}, d_reg),
            TupleVariation({"wght": (p_reg, 1.0, 1.0)}, d_800),
        ]
        for t in var["cmap"].tables:
            if t.isUnicode():
                t.cmap[cp] = name
        added.append(name)

    # HVAR maps every glyph explicitly: the new ones take a row of zero deltas (their advance never varies).
    if "HVAR" in var:
        h = var["HVAR"].table
        store = h.VarStore
        zero = None
        for outer, vd in enumerate(store.VarData):
            for inner, row in enumerate(vd.Item):
                if all(v == 0 for v in row):
                    zero = (outer << 16) | inner
                    break
            if zero is not None:
                break
        if zero is None:
            vd = store.VarData[0]
            vd.Item.append([0] * vd.VarRegionCount)
            vd.ItemCount = len(vd.Item)
            zero = len(vd.Item) - 1
        for m in (h.AdvWidthMap, getattr(h, "LsbMap", None), getattr(h, "RsbMap", None)):
            if m is not None:
                for name in added:
                    m.mapping[name] = zero
    if "GDEF" in var and var["GDEF"].table.GlyphClassDef is not None:
        for name in added:
            var["GDEF"].table.GlyphClassDef.classDefs[name] = 1  # base glyph

    # One family name for photon's TextStyle::font("Oxanium"): the legacy ID 1 said "Oxanium ExtraLight" (the default instance).
    var["name"].setName("Oxanium", 1, 3, 1, 0x409)
    var["name"].setName("Oxanium", 1, 1, 0, 0)
    var.save(OUT)
    print(f"wrote {OUT}: {len(added)} dozenal digits added, {len(order)} glyphs")


def proof(path):
    """A sheet: the twelve dozenal digits and 0-9 at wght 200, 300, 400, 600, 800 — for a person to look at."""
    import numpy as np
    from PIL import Image, ImageDraw
    from fontTools.pens.recordingPen import DecomposingRecordingPen

    rows = [200, 300, 400, 600, 800]
    text = [chr(c) for c in DOZENAL] + list("0123456789")
    cell = 96
    scale = cell / 1000
    img = np.zeros((cell * len(rows), cell * len(text)), dtype=bool)
    for r, w in enumerate(rows):
        f = instantiateVariableFont(TTFont(OUT), {"wght": w})
        gs, cm = f.getGlyphSet(), f.getBestCmap()
        for c, ch in enumerate(text):
            pen = DecomposingRecordingPen(gs)
            gs[cm[ord(ch)]].draw(pen)
            contours, cur = [], []
            for op, args in pen.value:
                if op == "moveTo":
                    cur = [args[0]]
                elif op == "lineTo":
                    cur.append(args[0])
                elif op == "qCurveTo":
                    pts = [cur[-1]] + list(args)
                    # implied on-curve points between consecutive off-curve ones, then flatten each quadratic
                    segs, on = [], pts[0]
                    offs = pts[1:-1]
                    for a in range(len(offs)):
                        end = pts[-1] if a == len(offs) - 1 else ((offs[a][0] + offs[a + 1][0]) / 2, (offs[a][1] + offs[a + 1][1]) / 2)
                        for t in range(1, 9):
                            tt = t / 8
                            x = (1 - tt) ** 2 * on[0] + 2 * (1 - tt) * tt * offs[a][0] + tt ** 2 * end[0]
                            y = (1 - tt) ** 2 * on[1] + 2 * (1 - tt) * tt * offs[a][1] + tt ** 2 * end[1]
                            cur.append((x, y))
                        on = end
                    if not offs:
                        cur.append(pts[-1])
                elif op in ("closePath", "endPath"):
                    contours.append(cur)
                    cur = []
            layer = np.zeros((cell, cell), dtype=bool)
            for con in contours:
                m = Image.new("1", (cell, cell), 0)
                ImageDraw.Draw(m).polygon([(x * scale + 6, cell - 18 - y * scale) for x, y in con], fill=1)
                layer ^= np.array(m, dtype=bool)
            img[r * cell : (r + 1) * cell, c * cell : (c + 1) * cell] = layer
    Image.fromarray(np.where(img, 0, 255).astype("uint8")).save(path)
    print(f"proof: {path}")


if __name__ == "__main__":
    build()
    if "--proof" in sys.argv:
        proof(sys.argv[sys.argv.index("--proof") + 1])
