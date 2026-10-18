"""Writes the fuzz seed corpora and the `.art` dictionary from the repo's own sources.

    python3 fuzz/seeds.py [CORPUS_DIR]
"""

import hashlib
import os
import random
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
FUZZ = ROOT / "fuzz"
CORPUS = Path(sys.argv[1]) if len(sys.argv) > 1 else FUZZ / "corpus"
GEN_SEED_MIN_BYTES = 1024
GEN_SEED_MAX_BYTES = 12288
GEN_SEEDS = int(os.environ.get("GEN_SEEDS", "256"))
ART_TARGETS = ["parse", "format", "eval", "render", "fmt_eval"]

LITERAL = re.compile(r'r(#*)"(.*?)"\1|"((?:[^"\\]|\\.)*)"', re.S)
LIT = LITERAL.pattern.replace("(#*)", "(?:#*)").replace("\\1", "#*")
CONST = re.compile(r"const (\w+): &str =\s*(" + LIT + ");", re.S)
FILE_ARG = re.compile(r'\(\s*"([\w/.-]+\.(?:art|toml))",\s*(' + LIT + r"|[A-Z_]+)\s*\)", re.S)
ESCAPES = {"n": "\n", "t": "\t", "r": "\r", "0": "\0", '"': '"', "'": "'", "\\": "\\"}


def unescape(body: str) -> str:
    out, i = [], 0
    while i < len(body):
        c = body[i]
        if c != "\\":
            out.append(c)
            i += 1
            continue
        nxt = body[i + 1]
        if nxt == "\n":
            i += 2
            while i < len(body) and body[i] in " \t\n":
                i += 1
        elif nxt == "u":
            end = body.index("}", i)
            out.append(chr(int(body[i + 3 : end], 16)))
            i = end + 1
        else:
            out.append(ESCAPES.get(nxt, nxt))
            i += 2
    return "".join(out)


def literal(text: str) -> str:
    m = LITERAL.fullmatch(text.strip())
    return m.group(2) if m.group(2) is not None else unescape(m.group(3))


def literals(text: str):
    for m in LITERAL.finditer(text):
        yield literal(m.group(0))


def is_manifest(lit: str) -> bool:
    return lit.lstrip().startswith("[project]") or "\n[lint]" in lit


def is_art(lit: str) -> bool:
    return "\n" in lit and ("@target" in lit or re.search(r"^(def|palette|layer|use|let) ", lit, re.M) is not None)


def projects(text: str):
    consts = {m.group(1): literal(m.group(2)) for m in CONST.finditer(text)}
    for body in text.split("#[test]")[1:]:
        files = {}
        for m in FILE_ARG.finditer(body):
            src = consts.get(m.group(2)) if m.group(2).isupper() else literal(m.group(2))
            if src is not None:
                files[m.group(1)] = src
        named = set(files.values())
        manifest = next((lit for lit in literals(body) if is_manifest(lit)), consts.get("MANIFEST"))
        if manifest is not None:
            files["artscript.toml"] = manifest
        for lit in literals(body):
            if "@target" in lit and lit not in named:
                for k, v in consts.items():
                    lit = lit.replace("{" + k + "}", v)
                yield "\0".join([lit] + [f"{path}\n{src}" for path, src in sorted(files.items())])


def rust_tests():
    for path in sorted((ROOT / "crates").rglob("*.rs")):
        if "tests" in path.parts or path.name == "tests.rs" or "#[cfg(test)]" in path.read_text():
            yield path


def write(target: str, text: str) -> None:
    data = text.encode()
    d = CORPUS / target
    d.mkdir(parents=True, exist_ok=True)
    (d / hashlib.sha1(data).hexdigest()).write_bytes(data)


def seeds() -> None:
    art = [p.read_text() for p in sorted(ROOT.rglob("*.art")) if "target" not in p.parts]
    manifests = [p.read_text() for p in sorted(ROOT.rglob("artscript.toml"))]
    multi = []
    for path in rust_tests():
        text = path.read_text()
        for lit in literals(text):
            if is_manifest(lit):
                manifests.append(lit)
            elif is_art(lit):
                art.append(lit)
        multi.extend(projects(text))
    for src in art:
        for t in ART_TARGETS:
            write(t, src)
    for src in multi:
        for t in ["eval", "render", "fmt_eval"]:
            write(t, src)
    for src in manifests:
        write("manifest", src)
    print(f"{len(set(art))} art seeds, {len(set(multi))} project seeds, {len(set(manifests))} manifest seeds")


def generator_seeds() -> None:
    d = CORPUS / "gen"
    d.mkdir(parents=True, exist_ok=True)
    for i in range(GEN_SEEDS):
        rng = random.Random(i)
        data = rng.randbytes(rng.randrange(GEN_SEED_MIN_BYTES, GEN_SEED_MAX_BYTES))
        (d / hashlib.sha1(data).hexdigest()).write_bytes(data)
    print(f"{GEN_SEEDS} generator seeds")


def dict_escape(word: str) -> str:
    """libFuzzer dictionaries accept only `\\`, `\\"` and `\\xNN` escapes."""
    return "".join(
        "\\" + chr(b) if chr(b) in '"\\' else chr(b) if 32 <= b < 127 else f"\\x{b:02x}" for b in word.encode()
    )


def dictionary() -> None:
    words = set(re.findall(r'\(Word::\w+, "([^"]+)"', (ROOT / "crates/syntax/src/words.rs").read_text()))
    for path in sorted((ROOT / "crates/eval/src").glob("*.rs")):
        if path.name != "tests.rs":
            words |= set(re.findall(r'"([a-z_][a-z0-9_-]*)"\s*(?:\||=>)', path.read_text()))
    lex = (ROOT / "crates/syntax/src/lex.rs").read_text()
    words |= {w for w in re.findall(r"'([^'\\])'", lex) if not w.isalnum() and w not in " _"}
    words |= {"@target ", " 64x64", ".png", ".svg", ".gif", "\n  ", "\0", "use a", "use b", "#ff0000", "0..3", "->"}
    lines = [f'"{dict_escape(w)}"' for w in sorted(words)]
    (FUZZ / "art.dict").write_text("\n".join(lines) + "\n")
    print(f"{len(lines)} dictionary entries")


seeds()
generator_seeds()
dictionary()
