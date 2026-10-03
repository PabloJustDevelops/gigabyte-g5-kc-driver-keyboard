#!/usr/bin/env python3
"""Report where the cards actually are in a snapshot, in logical pixels.

Usage: measure.py <png> [y_logical]

Scans one horizontal line for the card border colour (LINE, #272b33) and prints
the runs it finds. Used to check that a two-lane layout really is 2:1 and that
nothing is clipped at the right-hand edge.
"""
import sys
from PIL import Image

BG = (0x0b, 0x0d, 0x10)
LINE = (0x27, 0x2b, 0x33)


def close(a, b, tol=6):
    return all(abs(x - y) <= tol for x, y in zip(a, b))


def main():
    path = sys.argv[1]
    y_logical = float(sys.argv[2]) if len(sys.argv) > 2 else None
    im = Image.open(path).convert("RGB")
    w, h = im.size
    scale = 2
    y = int((y_logical if y_logical is not None else h / (2 * scale)) * scale)
    y = min(max(y, 0), h - 1)
    px = im.load()

    # Horizontal extent of anything that is not the window background: that is
    # the content, whatever colour it happens to be.
    left = right = None
    for x in range(w):
        if not close(px[x, y], BG, 3):
            if left is None:
                left = x
            right = x
    print(f"{path} {w}x{h} scanline y={y/scale:.0f}px")
    if left is None:
        print("  (window background only)")
        return
    print(f"  content spans {left/scale:.0f} .. {right/scale:.0f} logical "
          f"({(right-left+1)/scale:.0f} wide)")

    # Vertical border runs, i.e. the card edges.
    runs = []
    start = None
    for x in range(w):
        on = close(px[x, y], LINE, 8)
        if on and start is None:
            start = x
        elif not on and start is not None:
            runs.append((start, x - 1))
            start = None
    if start is not None:
        runs.append((start, w - 1))
    edges = [f"{a/scale:.0f}..{b/scale:.0f}" for a, b in runs if (b - a) / scale < 2]
    print("  vertical edges:", ", ".join(edges[:24]) if edges else "(none)")


main()