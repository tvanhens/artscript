"""Token counts of each artscript entry file (plus the libraries it imports) against the chibi
Python source that drew the same piece.

    uv run --with tiktoken python scripts/tokens.py
"""

import re
import sys
from pathlib import Path

import tiktoken

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "examples" / "chibi"
CHIBI = ROOT.parent / "chibi"
ENC = tiktoken.get_encoding("o200k_base")


def count(path: Path) -> int:
    return len(ENC.encode(path.read_text()))


def imports(art: Path) -> list[Path]:
    libs = []
    for m in re.finditer(r"^use (\S+)", art.read_text(), re.M):
        lib = PROJECT / "lib" / f"{m.group(1)}.art"
        if lib.exists():
            libs.append(lib)
    return libs


def cryptic_names(art: Path) -> list[str]:
    """Palettes, palette entries and top-level lets named in one or two letters. They shrink the
    count without making the art cheaper to write, so a file that has them is not scored."""
    names = []
    in_palette = False
    for line in art.read_text().splitlines():
        if m := re.match(r"^palette (\w+)", line):
            names.append(m.group(1))
            in_palette = True
            continue
        if in_palette and (m := re.match(r"^  (\w+) ", line)):
            names.append(m.group(1))
            continue
        in_palette = False
        if m := re.match(r"^let (\w+) =", line):
            names.append(m.group(1))
    return [n for n in names if len(n) <= 2]


rows = []
cryptic = {}
for art in sorted((PROJECT / "art").rglob("*.art")):
    if bad := cryptic_names(art):
        cryptic[art.relative_to(PROJECT / "art")] = bad
    rel = art.relative_to(PROJECT / "art").with_suffix(".py")
    py = CHIBI / "art" / rel
    if not py.exists():
        continue
    own = count(art)
    libs = sum(count(lib) for lib in imports(art))
    rows.append((str(rel.with_suffix("")), own, libs, count(py)))

if not rows:
    sys.exit("no artscript ports with a matching chibi source")

print(f"{'piece':32} {'art':>6} {'+libs':>6} {'python':>7} {'ratio':>6}")
for name, own, libs, py in rows:
    print(f"{name:32} {own:6} {own + libs:6} {py:7} {py / own:6.2f}x")
tot_own = sum(r[1] for r in rows)
tot_py = sum(r[3] for r in rows)
print(f"{'total':32} {tot_own:6} {'':6} {tot_py:7} {tot_py / tot_own:6.2f}x")
print("ratio = python tokens / artscript entry-file tokens (o200k_base). Shared libraries are")
print("listed separately because, like the chibi toolkit, they are written once per project.")
if cryptic:
    for art, bad in cryptic.items():
        print(f"cryptic names in {art}: {' '.join(bad)}", file=sys.stderr)
    sys.exit("rename them descriptively (scripts/rename_idents.py); the counts above are not valid")
