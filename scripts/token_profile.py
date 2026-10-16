"""Where the tokens of an artscript project go: per file, per lexical category, and per line role.

    uv run --with tiktoken python scripts/token_profile.py [PROJECT] [--json]

Each o200k_base token is charged to the lexeme its first non-blank character falls in, and to the
line it starts on. Whitespace-only tokens are newlines if they hold one, indentation if they sit
before a line's first character, and plain whitespace otherwise. PROJECT defaults to examples/chibi.
"""

import argparse
import json
import re
import sys
from collections import Counter
from pathlib import Path

from corpus import ENC, PROJECT, ROOT, cryptic_names, entries, libraries

CARD = ROOT / "docs" / "language.md"

# The house palette is always in scope as `p`. Its prefix is reported apart from the named ones.
DEFAULT_PALETTE = "p"

LEXEMES = {
    "comments": "comment to end of line",
    "strings": "quoted path data",
    "colors": "hex color literal",
    "keys": "a `key:` name and its colon",
    "palette_refs": "a reference that starts with a palette name, as in `peach.coral` or `p.glint`",
    "identifiers": "verbs, names, parameters, @references, and member access such as `p.x`",
    "numbers": "number literals with sign and unit (`-3`, `.5`, `30deg`), and ramp steps such as `.3`",
    "operators": "punctuation and operators",
    "indentation": "spaces before a line's first character",
    "newlines": "line breaks",
    "whitespace": "other spaces",
}

LEXEME_PATTERN = re.compile(
    r"""
    (?P<comments>\#(?=[ ]|$)[^\n]*)
  | (?P<strings>"[^"\n]*")
  | (?P<colors>\#[0-9a-fA-F]{3,8}\b)
  | (?P<keys>[a-z_][a-z0-9_]*:(?!:))
  | (?P<ref>@?[A-Za-z_]\w*(?:\.[A-Za-z_]\w*)*)
  | (?P<numbers>(?<![\w.])-?(?:\d+\.?\d*|\.\d+)(?:deg|turn|%)?|(?<=\w)\.\d+\b)
  | (?P<newlines>\n)
  | (?P<whitespace>[ \t]+)
  | (?P<operators>.)
    """,
    re.X | re.M,
)

# A line takes the first role whose rule matches. `under` is the role of the nearest enclosing line
# that claims its block (a cel boolean claims its operand copies, a palette claims its entries).
SHAPES = {"curve", "ellipse", "circle", "rect", "path", "poly", "star", "blob", "copy"}
BOOLEANS = {"subtract", "intersect", "union", "xor"}
LOGIC = {"let", "each", "if", "else", "assert", "expect", "rule"}


def has(line: str, *keys: str) -> bool:
    return all(re.search(rf"(?<![\w.]){k}:", line) for k in keys)


ROLES = {
    "comments": ("a comment-only line", lambda verb, line, under: verb == "#"),
    "palette": (
        "a palette, its entries, and `let x = name.entry` aliases",
        lambda verb, line, under: verb == "palette"
        or under == "palette"
        or re.fullmatch(r"\s*let \w+ = [a-z_]\w*\.\w+", line) is not None,
    ),
    "fx": ("an `fx` line", lambda verb, line, under: verb == "fx"),
    "cel_shading": (
        "a translucent or blurred boolean and its operands, or a `cel` call",
        lambda verb, line, under: under == "cel_shading"
        or verb == "cel"
        or (verb in BOOLEANS and (has(line, "a") or has(line, "blur"))),
    ),
    "soft_light_shadow": (
        "a blurred, translucent fill: any unstroked shape but an open curve",
        lambda verb, line, under: (verb in SHAPES - {"curve"} or (verb == "curve" and " closed" in line))
        and has(line, "a", "blur")
        and not has(line, "stroke"),
    ),
    "accent_strokes": (
        "an open curve with a stroke",
        lambda verb, line, under: verb == "curve" and " closed" not in line and has(line, "stroke"),
    ),
    "form_outlines": (
        "any other shape or boolean: the silhouettes and opaque details",
        lambda verb, line, under: verb in SHAPES | BOOLEANS,
    ),
    "logic_math": ("`let`, `each`, `if`, `else` and checks", lambda verb, line, under: verb in LOGIC),
    "structure": (
        "targets, `use`, `def`, layers, groups, clips, masks, slots, component calls and blank lines",
        lambda verb, line, under: True,
    ),
}
CLAIMS_BLOCK = {"palette", "cel_shading"}


def palettes(text: str) -> set[str]:
    return set(re.findall(r"^palette (\w+)", text, re.M))


def lex(text: str) -> list[tuple[int, int, str]]:
    """Spans (start, end, category) covering `text`."""
    names = palettes(text) | {DEFAULT_PALETTE}
    spans = []
    line_start = True
    for m in LEXEME_PATTERN.finditer(text):
        kind = m.lastgroup
        if kind == "whitespace" and line_start:
            kind = "indentation"
        elif kind == "ref":
            kind = "palette_refs" if "." in m.group() and m.group().split(".")[0] in names else "identifiers"
        spans.append((*m.span(), kind))
        line_start = kind == "newlines" or (line_start and kind == "indentation")
    return spans


def prefix_tokens(text: str, names: set[str]) -> int:
    """Tokens the `name.` prefixes of these palettes add: the count with them deleted, subtracted."""
    if not names:
        return 0
    bare = re.sub(rf"(?<![\w.@])(?:{'|'.join(sorted(names))})\.(?=[a-z_])", "", text)
    return len(ENC.encode(text)) - len(ENC.encode(bare))


def line_roles(text: str) -> list[str]:
    roles = []
    claims: list[tuple[int, str]] = []
    for line in text.split("\n"):
        body = line.strip()
        if not body:
            roles.append("structure")
            continue
        indent = len(line) - len(line.lstrip(" "))
        while claims and claims[-1][0] >= indent:
            claims.pop()
        under = claims[-1][1] if claims else None
        verb = "#" if body.startswith("#") else re.match(r"[@\w:]*", body).group().rstrip(":")
        role = next(name for name, (_, rule) in ROLES.items() if rule(verb, line, under))
        roles.append(role)
        if role in CLAIMS_BLOCK:
            claims.append((indent, role))
    return roles


def profile_text(text: str) -> dict:
    tokens = ENC.encode(text)
    _, offsets = ENC.decode_with_offsets(tokens)
    spans = lex(text)
    category_at = [""] * (len(text) + 1)
    for start, end, kind in spans:
        for i in range(start, end):
            category_at[i] = kind
    line_of = [0] * (len(text) + 1)
    line = 0
    for i, ch in enumerate(text):
        line_of[i] = line
        line += ch == "\n"
    roles = line_roles(text)

    lexemes, by_role = Counter(), Counter()
    ends = offsets[1:] + [len(text)]
    for start, end in zip(offsets, ends):
        piece = text[start:end]
        first = start + (len(piece) - len(piece.lstrip())) if piece.strip() else start
        kind = category_at[first]
        if not piece.strip():
            kind = "newlines" if "\n" in piece else category_at[start]
        lexemes[kind] += 1
        by_role[roles[line_of[first]]] += 1
    return {
        "tokens": len(tokens),
        "lines": text.count("\n"),
        "lexemes": dict(lexemes),
        "roles": dict(by_role),
        "palette_prefix_tokens": prefix_tokens(text, palettes(text)),
        "house_prefix_tokens": prefix_tokens(text, {DEFAULT_PALETTE}),
        "cryptic_names": cryptic_names(text),
    }


def shares(counts: dict, total: int) -> dict:
    return {k: {"tokens": counts.get(k, 0), "share": round(counts.get(k, 0) / total, 4)} for k in counts}


def profile(project: Path) -> dict:
    files = {}
    for path in entries(project) + libraries(project):
        files[str(path.relative_to(project))] = profile_text(path.read_text())
    total = sum(f["tokens"] for f in files.values())
    lexemes, roles = Counter(), Counter()
    for f in files.values():
        lexemes.update(f["lexemes"])
        roles.update(f["roles"])
    return {
        "encoding": "o200k_base",
        "project": str(project.relative_to(ROOT)) if project.is_relative_to(ROOT) else str(project),
        "files": {
            name: {"tokens": f["tokens"], "lines": f["lines"], "palette_prefix_tokens": f["palette_prefix_tokens"]}
            for name, f in files.items()
        },
        "total_tokens": total,
        "lexemes": shares({k: lexemes[k] for k in LEXEMES}, total),
        "roles": shares({k: roles[k] for k in ROLES}, total),
        "palette_prefix_tokens": sum(f["palette_prefix_tokens"] for f in files.values()),
        "house_prefix_tokens": sum(f["house_prefix_tokens"] for f in files.values()),
        "card_tokens": len(ENC.encode(CARD.read_text())),
        "cryptic_names": {name: f["cryptic_names"] for name, f in files.items() if f["cryptic_names"]},
    }


def table(title: str, rows: dict, notes: dict) -> list[str]:
    out = [f"{title:22} {'tokens':>7} {'share':>7}"]
    for name, row in rows.items():
        out.append(f"{name:22} {row['tokens']:7} {row['share'] * 100:6.1f}%  {notes[name]}")
    return out


def report(p: dict) -> str:
    out = [f"{'file':40} {'tokens':>7} {'lines':>6}"]
    for name, f in p["files"].items():
        out.append(f"{name:40} {f['tokens']:7} {f['lines']:6}")
    out.append(f"{'total':40} {p['total_tokens']:7}")
    out.append("")
    out += table("lexeme", p["lexemes"], LEXEMES)
    out.append("")
    out += table("line role", p["roles"], {k: v[0] for k, v in ROLES.items()})
    out.append("")
    out.append(f"named palette prefixes (`peach.`, `lantern.` ...) add {p['palette_prefix_tokens']} tokens")
    out.append(f"the house palette prefix `p.` adds {p['house_prefix_tokens']} tokens")
    out.append(f"language card {CARD.relative_to(ROOT)}: {p['card_tokens']} tokens")
    return "\n".join(out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("project", nargs="?", type=Path, default=PROJECT)
    parser.add_argument("--json", action="store_true", help="print the profile as JSON")
    args = parser.parse_args()
    p = profile(args.project.resolve())
    print(json.dumps(p, indent=2) if args.json else report(p))
    if p["cryptic_names"]:
        for name, bad in p["cryptic_names"].items():
            print(f"cryptic names in {name}: {' '.join(bad)}", file=sys.stderr)
        sys.exit("rename them descriptively (scripts/rename_idents.py); the profile above is not valid")


if __name__ == "__main__":
    main()
