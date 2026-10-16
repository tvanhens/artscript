"""What the token scripts share: the o200k_base tokenizer, the example project's files, and the rule
that refuses one- and two-letter names."""

import re
from pathlib import Path

import tiktoken

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "examples" / "chibi"
ENC = tiktoken.get_encoding("o200k_base")


def count(text: str) -> int:
    return len(ENC.encode(text))


def entries(project: Path = PROJECT) -> list[Path]:
    return sorted((project / "art").rglob("*.art"))


def libraries(project: Path = PROJECT) -> list[Path]:
    return sorted((project / "lib").rglob("*.art"))


def imports(art: Path, project: Path = PROJECT) -> list[Path]:
    libs = []
    for m in re.finditer(r"^use (\S+)", art.read_text(), re.M):
        lib = project / "lib" / f"{m.group(1)}.art"
        if lib.exists():
            libs.append(lib)
    return libs


def cryptic_names(text: str) -> list[str]:
    """Palettes, palette entries and top-level lets named in one or two letters. They shrink the
    count without making the art cheaper to write, so a file that has them is not scored."""
    names = []
    in_palette = False
    for line in text.splitlines():
        if m := re.match(r"^palette (\w+)", line):
            names.append(m.group(1))
            in_palette = True
            continue
        if in_palette and (m := re.match(r"^  (?:ramp|color) (\w+)", line)):
            names.append(m.group(1))
            continue
        in_palette = False
        if m := re.match(r"^let (\w+) =", line):
            names.append(m.group(1))
    return [n for n in names if len(n) <= 2]
