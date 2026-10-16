"""Rewrites `f(a, b)`, `[a, b]` and `{ a, b }` to space-separated form in .art files and in
artscript code inside markdown (fenced blocks and inline backticks). Tight commas (`14,0`) stay."""

import re
import sys
from pathlib import Path


def fix_code(text: str) -> str:
    out, depth, in_str = [], 0, False
    i = 0
    while i < len(text):
        ch = text[i]
        if ch == '"':
            in_str = not in_str
        elif not in_str and ch == "#" and depth == 0 and (i + 1 == len(text) or not text[i + 1].isalnum()):
            nl = text.find("\n", i)
            end = len(text) if nl == -1 else nl
            out.append(text[i:end])
            i = end
            continue
        elif not in_str and ch in "([{":
            depth += 1
        elif not in_str and ch in ")]}":
            depth = max(depth - 1, 0)
        elif not in_str and ch == "," and depth > 0:
            j = i + 1
            while j < len(text) and text[j] in " \t":
                j += 1
            prev_space = i > 0 and text[i - 1] in " \t"
            if j > i + 1 or prev_space:
                while out and out[-1] in (" ", "\t"):
                    out.pop()
                out.append(" ")
                i = j
                continue
        out.append(ch)
        i += 1
    return "".join(out)


def fix_markdown(text: str) -> str:
    def fence(m: re.Match) -> str:
        return m.group(1) + fix_code(m.group(2)) + m.group(3)

    text = re.sub(r"(```[a-z]*\n)(.*?)(```)", fence, text, flags=re.S)
    return re.sub(r"`([^`\n]+)`", lambda m: "`" + fix_code(m.group(1)) + "`", text)


for arg in sys.argv[1:]:
    p = Path(arg)
    src = p.read_text()
    new = fix_markdown(src) if p.suffix == ".md" else fix_code(src)
    if new != src:
        p.write_text(new)
        print(f"rewrote {p}")
