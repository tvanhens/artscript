"""Rewrite .art files from the old statement heads to the new ones, in place.

usage: python3 scripts/migrate_statement_heads.py FILE...

- Palette rows: `name ramp <rest>` becomes `ramp name <rest>`, `name <rest>` becomes `color name <rest>`.
  A row that already starts with `ramp` or `color` is left alone, including a swatch named `ramp`, so a rerun is a no-op.
- `mask:` ending a line becomes `mask` over the same block. `mask:<value>` becomes `mask <value>`.
- A `mask:<value>` named argument on a draw line moves to a child line `mask <value>`.
- `let fx` is renamed to `fiber_x` through the end of its block, and a `let fy` beside it to `fiber_y`.
  A line whose first word is the keyword `fx` keeps it.

A top-level `fx` line has no draw to attach to. It is reported and left for a hand fix.
"""

import re
import sys
from pathlib import Path
from typing import NamedTuple

KEYWORD_HEADS = {
    "use", "palette", "style", "def", "rule", "let", "shader", "light", "brush",
    "assert", "expect", "if", "else", "each", "slot", "fx", "mask",
}
NAME = r"[A-Za-z_]\w*"


class CodeAndComment(NamedTuple):
    code: str
    comment: str


def split_comment(line: str) -> CodeAndComment:
    i = 0
    while i < len(line):
        if line[i] == '"':
            j = line.find('"', i + 1)
            i = len(line) if j < 0 else j + 1
            continue
        if line[i] == "#":
            m = re.match(r"#([0-9A-Fa-f]+)", line[i:])
            after = line[i + len(m.group(0))] if m and i + len(m.group(0)) < len(line) else ""
            if m and len(m.group(1)) in (3, 4, 6, 8) and after in ("", " ", "\t", ",", ")", "]"):
                i += len(m.group(0))
                continue
            k = i
            while k > 0 and line[k - 1] in " \t":
                k -= 1
            return CodeAndComment(line[:k], line[k:])
        i += 1
    return CodeAndComment(line, "")


def indent_of(line: str) -> int:
    return len(line) - len(line.lstrip(" "))


def block_end(lines: list[str], i: int) -> int:
    base = indent_of(lines[i])
    j = i + 1
    while j < len(lines) and (not lines[j].strip() or indent_of(lines[j]) > base):
        j += 1
    while j > i + 1 and not lines[j - 1].strip():
        j -= 1
    return j


def swatch_row(line: str) -> str:
    code, comment = split_comment(line)
    m = re.match(rf"^(\s+)({NAME})(\s+)(.*)$", code)
    if not m:
        return line
    pad, first, _, rest = m.groups()
    if first in ("ramp", "color") and re.match(rf"{NAME}(\s|$)", rest):
        return line
    r = re.match(r"ramp(\s+)(.*)$", rest)
    if r:
        return f"{pad}ramp {first} {r.group(2)}{comment}"
    return f"{pad}color {first} {rest}{comment}"


def mask_head(line: str) -> str:
    code, comment = split_comment(line)
    m = re.match(r"^(\s*)mask:(.*)$", code)
    if not m:
        return line
    pad, value = m.groups()
    if not value.strip():
        return f"{pad}mask{comment}"
    if value[0] in " \t":
        return line
    return f"{pad}mask {value}{comment}"


def glued_chunk_len(text: str) -> int:
    depth = 0
    for k, c in enumerate(text):
        if c in "([":
            depth += 1
        elif c in ")]":
            depth -= 1
        elif c in " \t" and depth == 0:
            return k
    return len(text)


def expression_len(text: str) -> int:
    n = glued_chunk_len(text)
    while True:
        m = re.match(r"\s+(>=|<=|==|!=|->|>|<|\+|-|\*|/|and|or)\s+", text[n:])
        if not m:
            return n
        n += len(m.group(0))
        n += glued_chunk_len(text[n:])


def lift_mask_args(lines: list[str]) -> list[str]:
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        code, comment = split_comment(line)
        head = re.match(rf"^(\s*)({NAME})\b", code)
        m = re.search(r"\s(mask:)(\S)", code)
        if not head or head.group(2) in KEYWORD_HEADS or not m:
            out.append(line)
            i += 1
            continue
        start = m.start(1)
        n = expression_len(code[start + len("mask:"):])
        value = code[start + len("mask:"):start + len("mask:") + n]
        kept = (code[:m.start()] + code[start + len("mask:") + n:]).rstrip()
        end = block_end(lines, i)
        child = next((indent_of(l) for l in lines[i + 1:end] if l.strip()), indent_of(line) + 2)
        out.append(kept + comment)
        out.append(" " * child + "mask " + value)
        i += 1
    return out


def rename_fx_lets(lines: list[str]) -> list[str]:
    lines = list(lines)
    for i, line in enumerate(lines):
        if not re.match(r"^\s*let fx\s*=", line):
            continue
        end = block_end_same_level(lines, i)
        pairs = {"fx": "fiber_x"}
        if any(re.match(rf"^{' ' * indent_of(line)}let fy\s*=", l) for l in lines[i:end]):
            pairs["fy"] = "fiber_y"
        for j in range(i, end):
            lines[j] = rename_in_line(lines[j], pairs)
    return lines


def block_end_same_level(lines: list[str], i: int) -> int:
    base = indent_of(lines[i])
    j = i + 1
    while j < len(lines) and (not lines[j].strip() or indent_of(lines[j]) >= base):
        j += 1
    return j


def rename_outside_strings(text: str, pairs: dict[str, str]) -> str:
    out = []
    i = 0
    while i < len(text):
        if text[i] == '"':
            j = text.find('"', i + 1)
            j = len(text) if j < 0 else j + 1
            out.append(text[i:j])
            i = j
            continue
        j = text.find('"', i)
        if j < 0:
            j = len(text)
        chunk = text[i:j]
        for old, new in pairs.items():
            chunk = re.sub(rf"(?<![\w.#@]){old}(?![\w:])", new, chunk)
        out.append(chunk)
        i = j
    return "".join(out)


def rename_in_line(line: str, pairs: dict[str, str]) -> str:
    code, comment = split_comment(line)
    m = re.match(r"^(\s*fx\b)(.*)$", code)
    head, rest = (m.group(1), m.group(2)) if m else ("", code)
    return head + rename_outside_strings(rest, pairs) + comment


class Migrated(NamedTuple):
    source: str
    notes: list[str]


def migrate(src: str) -> Migrated:
    lines = src.split("\n")
    out = []
    in_palette = False
    for line in lines:
        if re.match(r"^palette\s", line):
            in_palette = True
            out.append(line)
            continue
        if in_palette and line.strip() and indent_of(line) == 0:
            in_palette = False
        if in_palette and line.strip() and not line.lstrip().startswith("#"):
            out.append(swatch_row(line))
        else:
            out.append(mask_head(line))
    out = rename_fx_lets(lift_mask_args(out))
    notes = [
        f"line {k + 1}: top-level `fx` has no draw to attach to"
        for k, l in enumerate(out)
        if re.match(r"^fx\b", l)
    ]
    return Migrated("\n".join(out), notes)


def main() -> None:
    for name in sys.argv[1:]:
        path = Path(name)
        src = path.read_text()
        new, notes = migrate(src)
        if new != src:
            path.write_text(new)
        for note in notes:
            print(f"{path}: {note}", file=sys.stderr)
        print(f"{path}: {'rewritten' if new != src else 'unchanged'}")


if __name__ == "__main__":
    main()
