"""Write a task's solution.patch from an edited copy of the project, and its reference transcript
from the patch.

    python3 bench/author.py patch <id> <edited copy of examples/chibi>
    python3 bench/author.py steps [<id>...]      # every task when no id is given

The transcript follows today's workflow as bench/README.md describes it.
"""

import difflib
import json
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

from transcript import PROJECT, TASKS, Edit, apply, hunk_from_edit, load_task, tasks

SOURCES = ("art", "lib")


def sources(root: Path) -> set[str]:
    return {str(p.relative_to(root)) for d in SOURCES for p in (root / d).rglob("*.art")}


def write_patch(task_id: str, edited: Path) -> None:
    out = []
    for rel in sorted(sources(PROJECT) | sources(edited)):
        before = (PROJECT / rel).read_text().splitlines(keepends=True) if (PROJECT / rel).exists() else []
        after = (edited / rel).read_text().splitlines(keepends=True) if (edited / rel).exists() else []
        source = f"a/{rel}" if before else "/dev/null"
        target = f"b/{rel}" if after else "/dev/null"
        out += difflib.unified_diff(before, after, source, target)
    if not out:
        sys.exit(f"{edited} does not differ from {PROJECT}")
    (TASKS / task_id).mkdir(parents=True, exist_ok=True)
    (TASKS / task_id / "solution.patch").write_text("".join(out))


def patched(task_dir: Path) -> dict[str, str]:
    """Every source file the solution changes or adds, after `git apply`."""
    with tempfile.TemporaryDirectory() as tmp:
        for d in SOURCES:
            shutil.copytree(PROJECT / d, Path(tmp) / d)
        subprocess.run(["git", "apply", str(task_dir / "solution.patch")], cwd=tmp, check=True)
        after = sources(Path(tmp))
        return {
            rel: (Path(tmp) / rel).read_text()
            for rel in sorted(after)
            if not (PROJECT / rel).exists() or (PROJECT / rel).read_text() != (Path(tmp) / rel).read_text()
        }


def edits(path: str, before: str, after: str) -> list[tuple[Edit, int]]:
    """Search-and-replace edits that turn `before` into `after`, top to bottom. Each old side is the
    changed lines plus the fewest context lines that make it unique in the file at that point."""
    out = []
    current = before
    while current != after:
        lines, goal = current.splitlines(keepends=True), after.splitlines(keepends=True)
        tag, i1, i2, j1, j2 = next(op for op in difflib.SequenceMatcher(None, lines, goal, autojunk=False).get_opcodes() if op[0] != "equal")
        ahead = behind = 0
        while True:
            old = "".join(lines[i1 - ahead : i2 + behind])
            if old and current.count(old) == 1:
                break
            if i1 - ahead > 0 and (ahead <= behind or i2 + behind >= len(lines)):
                ahead += 1
            else:
                behind += 1
        new = "".join(lines[i1 - ahead : i1] + goal[j1:j2] + lines[i2 : i2 + behind])
        edit = Edit(path, old, new)
        out.append((edit, i1 - ahead + 1))
        files = {path: current}
        apply(files, edit)
        current = files[path]
    return out


def target_sizes(texts: list[str]) -> dict[str, str]:
    return {m[0]: m[1] for t in texts for m in re.findall(r"^@target (\S+\.png) (\d+x\d+)", t, re.M)}


def write_steps(task_id: str) -> None:
    task = load_task(TASKS / task_id)
    changed = patched(task.dir)
    steps = []
    reads = list(task.reads) + [p for p in changed if (PROJECT / p).exists() and p not in task.reads]
    steps += [{"read": p} for p in reads]
    sizes = target_sizes([(PROJECT / p).read_text() for p in sources(PROJECT)] + list(changed.values()))
    if task.kind == "construction":
        reference = next(p for p in task.reads if p.startswith("art/"))
        png = reference.removeprefix("art/").removesuffix(".art") + ".png"
        steps.append({"look": sizes[png], "path": f"assets/{png}"})
    for path, after in changed.items():
        before = (PROJECT / path).read_text() if (PROJECT / path).exists() else ""
        if not before:
            steps.append({"write": hunk_from_edit(Edit(path, "", after), 1)})
            continue
        steps += [{"write": hunk_from_edit(edit, line)} for edit, line in edits(path, before, after)]
    steps.append({"build": f"{task.piece}.*"})
    for piece in (task.piece, *task.looks):
        steps.append({"look": sizes[f"{piece}.png"], "path": f"assets/{piece}.png"})
    (task.dir / "steps.jsonl").write_text("".join(json.dumps(s) + "\n" for s in steps))


def main() -> None:
    match sys.argv[1:]:
        case ["patch", task_id, edited]:
            write_patch(task_id, Path(edited).resolve())
        case ["steps", *ids]:
            for task_id in ids or [t.id for t in tasks()]:
                write_steps(task_id)
        case _:
            sys.exit(__doc__)


if __name__ == "__main__":
    main()
