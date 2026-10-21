# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow==11.3.0"]
# ///
"""Read-only preview evidence, counted per command event rather than invocation."""

import argparse
import json
from pathlib import Path
import shlex

from PIL import Image


def preview_command(command: str) -> bool:
    if "<<" in command:
        return False
    try:
        lexer = shlex.shlex(command, posix=False, punctuation_chars=";&|()\n")
        lexer.whitespace = " \t\r"
        lexer.whitespace_split = True
        words = list(lexer)
    except ValueError:
        return False
    if words and Path(words[0]).name in {"sh", "bash", "zsh"}:
        for index, word in enumerate(words[1:], 1):
            if word.startswith("-") and "c" in word and index + 1 < len(words):
                script = shlex.split(words[index + 1])
                return preview_command(script[0]) if len(script) == 1 else False
        return False
    start = True
    for index, word in enumerate(words):
        if word and all(c in ";&|()\n" for c in word):
            start = True
        elif start:
            if "=" in word and word.split("=", 1)[0].isidentifier():
                continue
            if word in {"env", "command"}:
                continue
            if Path(word).name == "ramose" and words[index + 1:index + 2] == ["preview"]:
                return True
            if Path(word).name == "npx":
                remaining = words[index + 1:]
                if remaining[:1] in (["--yes"], ["-y"]):
                    remaining = remaining[1:]
                if remaining[:2] == ["ramose", "preview"]:
                    return True
            start = False
    return False


def receipts(output: str) -> list[dict]:
    decoder = json.JSONDecoder()
    found = []
    offset = 0
    for line in output.splitlines(keepends=True):
        if line.lstrip().startswith("{"):
            try:
                value, _ = decoder.raw_decode(output[offset:].lstrip())
            except ValueError:
                value = None
            if (isinstance(value, dict)
                    and isinstance(value.get("source"), str)
                    and isinstance(value.get("target"), str)
                    and type(value.get("target_line")) is int
                    and value.get("backend") in {"cpu", "gpu"}
                    and type(value.get("exit_code")) is int
                    and isinstance(value.get("diagnostics"), list)
                    and isinstance(value.get("times"), list)
                    and isinstance(value.get("size"), list) and len(value["size"]) == 2
                    and isinstance(value.get("paths"), list) and value["paths"]
                    and all(isinstance(p, str) and p.endswith(".png") for p in value["paths"])
                    and all(key in value for key in ("crop_canvas", "crop_pixels", "parameters", "sampling"))):
                found.append(value)
        offset += len(line)
    return found


def read_json(path: Path, errors: list[str]):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError) as exc:
        errors.append(f"{path.name}: {exc}")
        return None


def author(folder: Path) -> dict:
    errors = []
    process = read_json(folder / "codex.json", errors) if (folder / "codex.json").exists() else None
    events_path = folder / "events.json"
    events = read_json(events_path, errors) if events_path.exists() else None
    if events is None and isinstance(process, dict):
        events = []
        for line in process.get("stdout", "").splitlines():
            try:
                events.append(json.loads(line))
            except ValueError:
                errors.append("codex stdout contains an incomplete or non-JSON event")
    if not isinstance(events, list):
        events = []
    command = process.get("command", []) if isinstance(process, dict) else []
    origin = None
    if isinstance(command, list) and "-C" in command:
        index = command.index("-C")
        if index + 1 < len(command):
            origin = Path(command[index + 1])
    items = {}
    for index, event in enumerate(events):
        if not isinstance(event, dict) or event.get("type") not in {"item.started", "item.updated", "item.completed"}:
            continue
        item = event.get("item", {})
        if isinstance(item, dict) and item.get("type") == "command_execution":
            key = item.get("id", f"unidentified-{index}")
            items[key] = {**items.get(key, {}), **item}
    calls = []
    for key, item in items.items():
        if not preview_command(item.get("command", "")):
            continue
        evidence = []
        for receipt in receipts(item.get("aggregated_output", "")):
            artifacts = []
            for value in receipt["paths"]:
                path = Path(value)
                if path.is_absolute():
                    relative = path.relative_to(origin) if origin and path.is_relative_to(origin) else None
                else:
                    relative = path
                retained = folder / "project" / relative if relative is not None else None
                if retained is not None and not retained.resolve().is_relative_to((folder / "project").resolve()):
                    retained = None
                exists = retained is not None and retained.is_file()
                valid = False
                if exists:
                    try:
                        with Image.open(retained) as image:
                            valid = image.format == "PNG"
                            image.verify()
                    except (OSError, ValueError, SyntaxError):
                        valid = False
                artifacts.append({"receipt_path": value, "retained_path": str(retained) if retained else None,
                                  "exists": exists, "valid_png": valid})
            evidence.append({"exit_code": receipt["exit_code"], "artifacts": artifacts})
        calls.append({"item_id": key, "command": item.get("command"),
                      "exit_code": item.get("exit_code"), "receipts": evidence})
    return {"pair": folder.parent.name, "arm": folder.name,
            "events_available": bool(events), "turn_completed": any(isinstance(e, dict) and e.get("type") == "turn.completed" for e in events),
            "preview_command_events": len(calls), "valid_receipts": sum(len(c["receipts"]) for c in calls),
            "retained_pngs": sum(a["valid_png"] for c in calls for r in c["receipts"] for a in r["artifacts"]),
            "viewer_observation": "unknown", "calls": calls, "read_errors": errors}


def analyze(bundle: Path) -> dict:
    folders = sorted(p for p in (bundle / "pairs").glob("*/*") if p.is_dir() and p.name in {"baseline", "candidate"})
    return {"schema_version": 1, "bundle": str(bundle),
            "limitations": ["Preview command events are conservative shell-token matches, not individual invocation counts or a shell interpreter.",
                            "Command events containing heredoc or here-string markers are unobserved, including any real preview calls in those events.",
                            "Image viewing is unknown because the author event protocol has no verified viewer event schema.",
                            "Retained PNG counts are per receipt reference, not unique images or proof of viewing.",
                            "This report does not verify bundle seals or judge visual quality."],
            "authors": [author(folder) for folder in folders]}


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("bundle", type=Path)
    args = parser.parse_args()
    print(json.dumps(analyze(args.bundle.resolve()), indent=2))
