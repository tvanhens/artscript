#!/usr/bin/env python3
"""Render an .art file and place it beside a reference image, for reviewing a drawing as it develops.

usage: compare.py REFERENCE.png FILE.art OUT.png [--project examples/live] [--bin target/release/artscript]

Prints the render's diagnostics and exits with its code. The PNG is written even when the file has errors.
"""

import argparse
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent.parent


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("reference", type=Path)
    ap.add_argument("art", type=Path)
    ap.add_argument("out", type=Path)
    ap.add_argument("--project", type=Path, default=REPO / "examples/live")
    ap.add_argument("--bin", type=Path, default=REPO / "target/release/artscript")
    ap.add_argument("--height", type=int, default=512)
    args = ap.parse_args()

    frame = Path(tempfile.mkdtemp()) / "render.png"
    proc = subprocess.run(
        [str(args.bin.resolve()), "render", "art/live.art", "--stdin", "-o", str(frame)],
        cwd=args.project, input=args.art.read_bytes(), stderr=subprocess.PIPE,
    )
    sys.stderr.write(proc.stderr.decode())
    if not frame.exists():
        return proc.returncode or 1
    h = args.height
    subprocess.run(
        ["ffmpeg", "-v", "error", "-y", "-i", str(args.reference), "-i", str(frame), "-filter_complex",
         f"[0]scale=-2:{h}[a];[1]scale=-2:{h}[b];[a][b]hstack", str(args.out)],
        check=True,
    )
    return proc.returncode


if __name__ == "__main__":
    sys.exit(main())
