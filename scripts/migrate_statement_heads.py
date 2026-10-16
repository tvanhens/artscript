"""Rewrite .art files from the old statement heads to the new ones, in place.

usage: python3 scripts/migrate_statement_heads.py FILE...

- Palette rows: `name ramp <rest>` becomes `ramp name <rest>`, `name <rest>` becomes `color name <rest>`.
  A row that already starts with `ramp` or `color` is left alone, including a swatch named `ramp`, so a rerun is a no-op.
- `mask:` ending a line becomes `mask` over the same block. `mask:<value>` becomes `mask <value>`.
- A `mask:<value>` named argument on a draw line moves to a child line `mask <value>`.
- `let fx` is renamed to `fiber_x` through the end of its block, and a `let fy` beside it to `fiber_y`.
  If that name is already used, the next free `fiber_x2` or `fiber_y2` is chosen.
  A line whose first word is the keyword `fx` keeps it.

A top-level `fx` line has no draw to attach to. It is reported and left for a hand fix.
"""

import re
import sys
from pathlib import Path
from typing import NamedTuple, Optional

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


def skip_string(text: str, i: int) -> int:
    if text.startswith('"""', i):
        j = text.find('"""', i + 3)
        return len(text) if j < 0 else j + 3
    i += 1
    while i < len(text):
        if text[i] == "\\":
            i += 2
            continue
        if text[i] == '"':
            return i + 1
        i += 1
    return len(text)


def mask_value_at(code: str) -> Optional[int]:
    i = 0
    while i < len(code):
        if code[i] == '"':
            i = skip_string(code, i)
            continue
        if code[i] in " \t" and code.startswith("mask:", i + 1):
            after = i + 1 + len("mask:")
            if after < len(code) and code[after] not in " \t":
                return i + 1
        i += 1
    return None


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


def word_at(text: str, i: int, word: str) -> bool:
    end = i + len(word)
    if not text.startswith(word, i):
        return False
    return end == len(text) or not (text[end].isalnum() or text[end] == "_")


OPS = r"(?:->|\.\.|>=|<=|==|!=|~=|[<>+\-*/]|and|or|in)"


def expression_len(text: str) -> int:
    n = prefix_len(text, 0)
    while True:
        m = re.match(rf"\s+{OPS}\s+", text[n:])
        if not m:
            return n
        n += len(m.group(0))
        n = prefix_len(text, n)


def prefix_len(text: str, i: int) -> int:
    if word_at(text, i, "not"):
        n = i + 3
        gap = re.match(r"\s+", text[n:])
        if gap is not None:
            return expression_len_from(text, n + len(gap.group(0)))
    if word_at(text, i, "if"):
        n = i + 2
        gap = re.match(r"\s+", text[n:])
        if gap is None:
            return n
        n += len(gap.group(0))
        n = expression_len_from(text, n)
        then = re.match(r"\s+then\b", text[n:])
        if then is None:
            return n
        n += len(then.group(0))
        gap = re.match(r"\s+", text[n:])
        if gap is None:
            return n
        n += len(gap.group(0))
        n = expression_len_from(text, n)
        otherwise = re.match(r"\s+else\b", text[n:])
        if otherwise is None:
            return n
        n += len(otherwise.group(0))
        gap = re.match(r"\s+", text[n:])
        if gap is None:
            return n
        n += len(gap.group(0))
        return expression_len_from(text, n)
    return i + glued_chunk_len(text[i:])


def expression_len_from(text: str, i: int) -> int:
    return i + expression_len(text[i:])


def lift_mask_args(lines: list[str]) -> list[str]:
    out: list[str] = []
    i = 0
    while i < len(lines):
        line = lines[i]
        code, comment = split_comment(line)
        head = re.match(rf"^(\s*)({NAME})\b", code)
        start = None if head is None or head.group(2) in KEYWORD_HEADS else mask_value_at(code)
        if start is None:
            out.append(line)
            i += 1
            continue
        n = expression_len(code[start + len("mask:"):])
        value = code[start + len("mask:"):start + len("mask:") + n]
        cut = start - 1 if start > 0 and code[start - 1] in " \t" else start
        kept = (code[:cut] + code[start + len("mask:") + n:]).rstrip()
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
        pairs = {"fx": fresh_name(lines, "fiber_x")}
        if any(re.match(rf"^{' ' * indent_of(line)}let fy\s*=", l) for l in lines[i:end]):
            pairs["fy"] = fresh_name(lines, "fiber_y")
        for j in range(i, end):
            lines[j] = rename_in_line(lines[j], pairs)
    return lines


def ident_used(lines: list[str], name: str) -> bool:
    pat = re.compile(rf"(?<![\w.#@]){name}(?![\w:])")
    return any(pat.search(split_comment(line).code) for line in lines)


def fresh_name(lines: list[str], base: str) -> str:
    name = base
    n = 2
    while ident_used(lines, name):
        name = f"{base}{n}"
        n += 1
    return name


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
