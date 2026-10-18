#!/usr/bin/env python3
"""Write a review sheet for every catalog golden that differs between BASE and the working tree.

usage: scripts/visual_review.py BASE OUT_DIR

Each sheet is `base | head | diff`. The diff dims unchanged pixels and paints changed pixels red,
where a pixel changed when a channel moved by more than TOLERANCE, as in the visual test.
An added golden's sheet is the head view alone, and a deleted golden's sheet is the base view
alone. OUT_DIR/index.md lists every changed golden with its fixture and its sheet.
"""

import struct
import subprocess
import sys
import zlib
from pathlib import Path

GOLDEN = "catalog/golden/"
GAP = 2
GAP_RGBA = (128, 128, 128, 255)
TOLERANCE = 8  # matches crates/cli/tests/visual.rs: a channel must move by more than this


def decode_png(data):
    assert data[:8] == b"\x89PNG\r\n\x1a\n", "not a png"
    pos, idat = 8, b""
    while pos < len(data):
        (length,) = struct.unpack(">I", data[pos : pos + 4])
        kind, body = data[pos + 4 : pos + 8], data[pos + 8 : pos + 8 + length]
        if kind == b"IHDR":
            width, height, depth, color, _, _, interlace = struct.unpack(">IIBBBBB", body)
            assert (depth, color, interlace) == (8, 6, 0), "goldens are 8-bit RGBA, not interlaced"
        elif kind == b"IDAT":
            idat += body
        pos += 12 + length
    raw, stride = zlib.decompress(idat), width * 4
    rows, prev = [], bytearray(stride)
    for y in range(height):
        start = y * (stride + 1)
        kind, line = raw[start], bytearray(raw[start + 1 : start + 1 + stride])
        for i in range(stride):
            a = line[i - 4] if i >= 4 else 0
            b = prev[i]
            c = prev[i - 4] if i >= 4 else 0
            if kind == 1:
                line[i] = (line[i] + a) & 255
            elif kind == 2:
                line[i] = (line[i] + b) & 255
            elif kind == 3:
                line[i] = (line[i] + (a + b) // 2) & 255
            elif kind == 4:
                p = a + b - c
                pa, pb, pc = abs(p - a), abs(p - b), abs(p - c)
                line[i] = (line[i] + (a if pa <= pb and pa <= pc else b if pb <= pc else c)) & 255
        rows.append(line)
        prev = line
    return width, height, [[tuple(r[x * 4 : x * 4 + 4]) for x in range(width)] for r in rows]


def encode_png(width, height, rows):
    def chunk(kind, body):
        return struct.pack(">I", len(body)) + kind + body + struct.pack(">I", zlib.crc32(kind + body))

    raw = b"".join(b"\x00" + bytes(c for px in row for c in px) for row in rows)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", struct.pack(">IIBBBBB", width, height, 8, 6, 0, 0, 0))
        + chunk(b"IDAT", zlib.compress(raw, 9))
        + chunk(b"IEND", b"")
    )


def sheet(views):
    height = max(h for _, h, _ in views)
    width = sum(w for w, _, _ in views) + GAP * (len(views) - 1)
    rows = [[GAP_RGBA] * width for _ in range(height)]
    x0 = 0
    for w, h, px in views:
        for y in range(h):
            rows[y][x0 : x0 + w] = px[y]
        x0 += w + GAP
    return width, height, rows


def diff(base, head):
    (bw, bh, bp), (w, h, hp) = base, head
    if (bw, bh) != (w, h):
        return (w, h, [[(255, 0, 0, 255)] * w for _ in range(h)]), w * h
    changed, rows = 0, []
    for y in range(h):
        row = []
        for x in range(w):
            if max(abs(b - h) for b, h in zip(bp[y][x], hp[y][x])) <= TOLERANCE:
                r, g, b, a = hp[y][x]
                level = (r + g + b) // 3 * a // 255 // 3 + 170
                row.append((level, level, level, 255))
            else:
                changed += 1
                row.append((255, 0, 0, 255))
        rows.append(row)
    return (w, h, rows), changed


def git(*args):
    return subprocess.run(["git", *args], check=True, capture_output=True).stdout


def fixture_for(golden):
    """The longest `catalog/art/<x>.art` whose `<x>` starts the golden's path, which the visual test enforces."""
    output = golden[len(GOLDEN) :]
    arts = [a for a in Path("catalog/art").rglob("*.art") if output.startswith(str(a.relative_to("catalog/art").with_suffix("")))]
    return str(max(arts, key=lambda a: len(str(a)))) if arts else "(no fixture)"


def main():
    base, out = sys.argv[1], Path(sys.argv[2])
    out.mkdir(parents=True, exist_ok=True)
    changes = git("diff", "--name-status", "--no-renames", f"{base}...HEAD", "--", GOLDEN).decode().split("\n")
    lines = ["| status | golden | changed pixels | fixture | sheet |", "|---|---|---|---|---|"]
    for change in filter(None, changes):
        status, path = change.split("\t")
        head = decode_png(Path(path).read_bytes()) if status != "D" else None
        old = decode_png(git("show", f"{base}:{path}")) if status != "A" else None
        if head and old:
            d, changed = diff(old, head)
            view = sheet([old, head, d])
        else:
            view, changed = head or old, "all"
        target = out / path[len(GOLDEN) :]
        target.parent.mkdir(parents=True, exist_ok=True)
        target.write_bytes(encode_png(*view))
        lines.append(f"| {status} | {path} | {changed} | {fixture_for(path)} | {target} |")
    if len(lines) == 2:
        lines = ["No catalog golden changed."]
    (out / "index.md").write_text("\n".join(lines) + "\n")
    print(f"{len(lines) - 2 if len(lines) > 1 else 0} changed golden(s), index at {out / 'index.md'}")


if __name__ == "__main__":
    main()
