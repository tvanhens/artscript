"""Replay each task's transcript and price it: tokens read, tokens written, image tokens and round
trips, per task and in total.

    uv run --with tiktoken python bench/cost.py [--json] [--variant NAME]... [--image-formula EXPR]

`steps.jsonl` is today's workflow, reported as variant `today`. `--variant cli` adds the tasks'
`steps.cli.jsonl` and the change against `today` over the tasks that have both.
"""

import argparse
import json
import math
import re
import sys

from transcript import Files, apply, edit_from_hunk, load_steps, tasks

from corpus import count

BASELINE = "today"
DEFAULT_IMAGE_FORMULA = "ceil(w*h/750)"
METRICS = ("tokens_read", "tokens_written", "image_tokens", "total_tokens", "round_trips")


def read_text(files: Files, spec: str) -> str:
    path, _, span = spec.partition(":")
    text = files[path]
    if not span:
        return text
    lo, hi = (int(n) for n in span.split("-"))
    return "".join(text.splitlines(keepends=True)[lo - 1 : hi])


def replay(steps: list[dict], image_tokens) -> dict:
    files = Files()
    cost = dict.fromkeys(METRICS, 0)

    def read(value):
        cost["tokens_read"] += count(read_text(files, value))

    def write(value):
        edit = edit_from_hunk(value)
        cost["tokens_written"] += count(edit.old) + count(edit.new)
        apply(files, edit)

    def round_trip(value):
        cost["round_trips"] += 1

    def look(value):
        w, h = (int(n) for n in re.fullmatch(r"(\d+)x(\d+)", value).groups())
        cost["image_tokens"] += image_tokens(w, h)
        cost["round_trips"] += 1

    kinds = {"read": read, "write": write, "build": round_trip, "check": round_trip, "look": look}
    for step in steps:
        kind = next(k for k in step if k in kinds)
        kinds[kind](step[kind])
    cost["total_tokens"] = cost["tokens_read"] + cost["tokens_written"] + cost["image_tokens"]
    return cost


def run(variants: list[str], formula: str) -> dict:
    image_tokens = lambda w, h: int(eval(formula, {"__builtins__": {}, "ceil": math.ceil}, {"w": w, "h": h}))
    report = {"image_formula": formula, "variants": {}}
    for variant in [BASELINE, *variants]:
        name = "steps.jsonl" if variant == BASELINE else f"steps.{variant}.jsonl"
        per_task = {}
        for task in tasks():
            if (task.dir / name).is_file():
                try:
                    per_task[task.id] = {"kind": task.kind, **replay(load_steps(task.dir / name), image_tokens)}
                except Exception as err:
                    sys.exit(f"{task.id}/{name}: {err}")
        total = {m: sum(t[m] for t in per_task.values()) for m in METRICS}
        report["variants"][variant] = {"tasks": per_task, "total": {"tasks": len(per_task), **total}}
    for variant in variants:
        mine = report["variants"][variant]["tasks"]
        base = report["variants"][BASELINE]["tasks"]
        shared = sorted(set(mine) & set(base))
        report["variants"][variant]["against_today"] = {
            m: {
                "today": sum(base[t][m] for t in shared),
                variant: sum(mine[t][m] for t in shared),
            }
            for m in METRICS
        } | {"tasks": len(shared)}
    return report


def table(name: str, block: dict) -> list[str]:
    head = f"{'task (' + name + ')':34} {'kind':12} {'read':>7} {'write':>6} {'image':>6} {'total':>7} {'trips':>5}"
    out = [head]
    for task, c in block["tasks"].items():
        out.append(
            f"{task:34} {c['kind']:12} {c['tokens_read']:7} {c['tokens_written']:6} "
            f"{c['image_tokens']:6} {c['total_tokens']:7} {c['round_trips']:5}"
        )
    t = block["total"]
    out.append(
        f"{'total (' + str(t['tasks']) + ' tasks)':34} {'':12} {t['tokens_read']:7} {t['tokens_written']:6} "
        f"{t['image_tokens']:6} {t['total_tokens']:7} {t['round_trips']:5}"
    )
    return out


def text_report(report: dict) -> str:
    out = []
    for name, block in report["variants"].items():
        out += table(name, block) + [""]
        if "against_today" in block:
            for metric in METRICS:
                pair = block["against_today"][metric]
                change = (pair[name] - pair[BASELINE]) / pair[BASELINE] * 100 if pair[BASELINE] else 0
                out.append(f"{metric:16} {pair[BASELINE]:8} -> {pair[name]:8}  {change:+6.1f}%")
            out += [f"over the {block['against_today']['tasks']} tasks both variants cover", ""]
    out.append(f"image tokens = {report['image_formula']}; total = read + write + image (o200k_base)")
    return "\n".join(out)


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    parser.add_argument("--json", action="store_true", help="print the report as JSON")
    parser.add_argument("--variant", action="append", default=[], help="also replay steps.<NAME>.jsonl")
    parser.add_argument("--image-formula", default=DEFAULT_IMAGE_FORMULA, help="tokens per image in w, h, ceil")
    args = parser.parse_args()
    report = run(args.variant, args.image_formula)
    print(json.dumps(report, indent=2) if args.json else text_report(report))


if __name__ == "__main__":
    main()
