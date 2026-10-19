"""Token counts of each chibi entry file against the SVG it builds.

    uv run --with tiktoken python scripts/svg_tokens.py
"""

import re
from pathlib import Path

import tiktoken

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "examples" / "chibi"
ENC = tiktoken.get_encoding("o200k_base")


def count(text: str) -> int:
    return len(ENC.encode(text))


# Grain and other raster effects ship as embedded images. A model would not write those bytes by
# hand, so they are cut and only the vector markup is scored.
EMBEDDED = re.compile(r'data:image/[^"]+')

rows = []
for art in sorted((PROJECT / "art").rglob("*.ramo")):
    rel = art.relative_to(PROJECT / "art").with_suffix("")
    svg = PROJECT / "assets" / rel.with_suffix(".svg")
    rows.append((str(rel), count(art.read_text()), count(EMBEDDED.sub("", svg.read_text()))))

print(f"{'piece':28} {'art':>6} {'svg':>8} {'ratio':>6}")
for name, art, svg in rows:
    print(f"{name:28} {art:6} {svg:8} {svg / art:5.1f}x")
tot_art = sum(r[1] for r in rows)
tot_svg = sum(r[2] for r in rows)
print(f"{'total':28} {tot_art:6} {tot_svg:8} {tot_svg / tot_art:5.1f}x")
print("svg = the built SVG with embedded images removed (o200k_base).")
