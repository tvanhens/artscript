"""Rename a palette and top-level identifiers in one .art file.

usage: python3 scripts/rename_idents.py FILE PALETTE_OLD=PALETTE_NEW [entry_old=entry_new ...] [--let old=new ...]

Palette entries are renamed inside the palette block and in every `PALETTE.entry` reference.
`--let` names are renamed as whole identifiers that are not member accesses.
"""

import re
import sys
from pathlib import Path


def main() -> None:
    args = sys.argv[1:]
    path = Path(args.pop(0))
    pal_old, pal_new = args.pop(0).split("=")
    entries: dict[str, str] = {}
    lets: dict[str, str] = {}
    target = entries
    for a in args:
        if a == "--let":
            target = lets
            continue
        old, new = a.split("=")
        target[old] = new

    out = []
    in_palette = False
    for line in path.read_text().split("\n"):
        if line == f"palette {pal_old}":
            out.append(f"palette {pal_new}")
            in_palette = True
            continue
        if in_palette:
            m = re.match(r"^  (\w+)( .*)$", line)
            if m:
                out.append("  " + entries.get(m.group(1), m.group(1)) + m.group(2))
                continue
            in_palette = False
        out.append(line)
    src = "\n".join(out)

    src = re.sub(
        rf"(?<![\w.]){re.escape(pal_old)}\.(\w+)",
        lambda m: f"{pal_new}.{entries.get(m.group(1), m.group(1))}",
        src,
    )
    for old, new in lets.items():
        src = re.sub(rf"(?<![\w.#@]){re.escape(old)}(?![\w:])", new, src)
    path.write_text(src)
    left = re.findall(rf"(?<![\w.]){re.escape(pal_old)}\.", src)
    print(f"{path}: done, {len(left)} `{pal_old}.` references left")


if __name__ == "__main__":
    main()
