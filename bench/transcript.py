"""The step model `cost.py` and `author.py` share: tasks, transcript steps, and one-hunk
search-and-replace edits."""

import json
import re
import sys
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PROJECT = ROOT / "examples" / "chibi"
TASKS = ROOT / "bench" / "tasks"
sys.path.insert(0, str(ROOT / "scripts"))

KINDS = ("value", "structural", "multi", "variant", "construction")


@dataclass(frozen=True)
class Edit:
    """Replace the one occurrence of `old` in `path` with `new`. An empty `old` creates the file."""

    path: str
    old: str
    new: str


def edit_from_hunk(hunk: str) -> Edit:
    lines = hunk.split("\n")
    target = next(l for l in lines if l.startswith("+++ "))[4:]
    source = next(l for l in lines if l.startswith("--- "))[4:]
    body = [l for l in lines[lines.index(next(l for l in lines if l.startswith("@@"))) + 1 :] if l]
    old = "".join(l[1:] + "\n" for l in body if l[0] in " -")
    new = "".join(l[1:] + "\n" for l in body if l[0] in " +")
    if source == "/dev/null":
        old = ""
    return Edit(target.removeprefix("b/"), old, new)


def hunk_from_edit(edit: Edit, line: int) -> str:
    """The edit as a one-hunk unified diff. Context lines shared by both sides stay context."""
    old, new = edit.old.splitlines(), edit.new.splitlines()
    lead = 0
    while lead < min(len(old), len(new)) and old[lead] == new[lead]:
        lead += 1
    trail = 0
    while trail < min(len(old), len(new)) - lead and old[-1 - trail] == new[-1 - trail]:
        trail += 1
    body = [" " + l for l in old[:lead]]
    body += ["-" + l for l in old[lead : len(old) - trail]]
    body += ["+" + l for l in new[lead : len(new) - trail]]
    body += [" " + l for l in old[len(old) - trail :]]
    source = "/dev/null" if not edit.old else f"a/{edit.path}"
    head = f"@@ -{line},{len(old)} +{line},{len(new)} @@" if edit.old else f"@@ -0,0 +1,{len(new)} @@"
    return "\n".join([f"--- {source}", f"+++ b/{edit.path}", head, *body]) + "\n"


def apply(files: dict[str, str], edit: Edit) -> None:
    if not edit.old:
        if edit.path in files:
            raise ValueError(f"{edit.path} already exists")
        files[edit.path] = edit.new
        return
    text = files[edit.path]
    if text.count(edit.old) != 1:
        raise ValueError(f"{edit.path}: the old side occurs {text.count(edit.old)} times, not once")
    files[edit.path] = text.replace(edit.old, edit.new)


class Files(dict):
    """File texts of the project, read from disk on first use."""

    def __missing__(self, path: str) -> str:
        text = (PROJECT / path).read_text()
        self[path] = text
        return text

    def __contains__(self, path: object) -> bool:
        return dict.__contains__(self, path) or (PROJECT / str(path)).is_file()


@dataclass(frozen=True)
class Task:
    id: str
    dir: Path
    kind: str
    piece: str
    reads: tuple[str, ...]
    looks: tuple[str, ...]


def load_task(dir: Path) -> Task:
    text = (dir / "task.md").read_text()
    meta = dict(re.findall(r"^(kind|piece|reads|looks): *(.*)$", text, re.M))
    if meta.get("kind") not in KINDS:
        raise ValueError(f"{dir.name}: kind must be one of {', '.join(KINDS)}")
    return Task(
        dir.name,
        dir,
        meta["kind"],
        meta["piece"],
        tuple(meta.get("reads", "").split()),
        tuple(meta.get("looks", "").split()),
    )


def tasks() -> list[Task]:
    return [load_task(d) for d in sorted(TASKS.iterdir()) if (d / "task.md").is_file()]


def load_steps(path: Path) -> list[dict]:
    return [json.loads(line) for line in path.read_text().splitlines() if line.strip()]
