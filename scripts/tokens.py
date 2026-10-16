"""Token counts of each artscript entry file (plus the libraries it imports) against the chibi
Python source that drew the same piece.

    uv run --with tiktoken python scripts/tokens.py
"""

import sys

from corpus import PROJECT, ROOT, count, cryptic_names, imports

CHIBI = ROOT.parent / "chibi"


rows = []
cryptic = {}
for art in sorted((PROJECT / "art").rglob("*.art")):
    if bad := cryptic_names(art.read_text()):
        cryptic[art.relative_to(PROJECT / "art")] = bad
    rel = art.relative_to(PROJECT / "art").with_suffix(".py")
    py = CHIBI / "art" / rel
    if not py.exists():
        continue
    own = count(art.read_text())
    libs = sum(count(lib.read_text()) for lib in imports(art))
    rows.append((str(rel.with_suffix("")), own, libs, count(py.read_text())))

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
