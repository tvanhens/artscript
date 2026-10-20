# /// script
# requires-python = ">=3.11"
# ///
"""Run artbench N times for each arm and compare the arms with bootstrap confidence intervals.

    uv run bench/ab.py --arm base=../base:../base/target/release/ramose --arm new=.:target/release/ramose -n 5
    uv run bench/ab.py --report bench/ab-out        # re-aggregate an earlier run

An arm is NAME=ROOT:BINARY. ROOT is a checkout whose plugins/ramose/skills/ramose is under test, and BINARY is the ramose it runs.
Every arm must carry the same bench/artbench.py, so only the skill and the binary differ.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import random
import statistics
import subprocess
import sys
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

METRICS = (
    ("cost_usd", lambda d: d["totals"]["cost"]["total"], False),
    ("cache_write", lambda d: d["totals"]["usage"]["cache_write_5m"] + d["totals"]["usage"]["cache_write_1h"], False),
    ("cache_read", lambda d: d["totals"]["usage"]["cache_read"], False),
    ("output", lambda d: d["totals"]["usage"]["output"], False),
    ("turns", lambda d: d["totals"]["num_turns"], False),
    ("duration_s", lambda d: d["totals"]["duration_s"], False),
    ("judge_mean", lambda d: statistics.mean(s["judge"]["score"] for s in d["steps"]), True),
    ("checks_failed", lambda d: sum(not c["ok"] for s in d["steps"] for c in s["checks"].values()), False),
)
STEP_METRICS = (
    ("cost", lambda s: s["cost"]["total"]),
    ("turns", lambda s: s["num_turns"]),
    ("judge", lambda s: s["judge"]["score"]),
)


def run_one(name: str, root: Path, binary: Path, out: Path, model: str) -> None:
    out.mkdir(parents=True, exist_ok=True)
    log = (out / "artbench.log").open("w")
    cmd = ["uv", "run", str(root / "bench/artbench.py"), "--ramose", str(binary), "--out", str(out / "run"), "--model", model]
    subprocess.run(cmd, stdout=log, stderr=subprocess.STDOUT)
    print(f"done {name} {out.name}", file=sys.stderr, flush=True)


def load(out: Path) -> dict[str, list[dict]]:
    arms: dict[str, list[dict]] = {}
    for report in sorted(out.glob("*/*/run/report.json")):
        arms.setdefault(report.parent.parent.parent.name, []).append(json.loads(report.read_text()))
    return arms


def boot_ratio(a: list[float], b: list[float], n: int = 4000) -> tuple[float, float]:
    rng = random.Random(0)
    ratios = []
    for _ in range(n):
        ma = statistics.mean(rng.choice(a) for _ in a)
        mb = statistics.mean(rng.choice(b) for _ in b)
        if ma:
            ratios.append(mb / ma - 1)
    ratios.sort()
    return ratios[int(0.025 * len(ratios))], ratios[int(0.975 * len(ratios)) - 1]


def fmt(v: float) -> str:
    return f"{v:,.3f}" if abs(v) < 100 else f"{v:,.0f}"


def report(arms: dict[str, list[dict]]) -> str:
    names = list(arms)
    base = names[0]
    lines = [f"runs: " + ", ".join(f"{n}={len(arms[n])}" for n in names), ""]
    header = "| metric | " + " | ".join(f"{n} mean ± sd (median)" for n in names) + " | " + " | ".join(f"{n} vs {base} [95% CI]" for n in names[1:]) + " |"
    lines += [header, "|" + "---|" * (1 + 2 * len(names) - 1)]
    for key, fn, _ in METRICS:
        vals = {n: [float(fn(d)) for d in arms[n]] for n in names}
        cells = []
        for n in names:
            v = vals[n]
            sd = statistics.stdev(v) if len(v) > 1 else 0.0
            cells.append(f"{fmt(statistics.mean(v))} ± {fmt(sd)} ({fmt(statistics.median(v))})")
        deltas = []
        for n in names[1:]:
            ma = statistics.mean(vals[base])
            delta = statistics.mean(vals[n]) / ma - 1 if ma else 0.0
            lo, hi = boot_ratio(vals[base], vals[n]) if ma else (0.0, 0.0)
            deltas.append(f"{delta:+.1%} [{lo:+.1%}, {hi:+.1%}]")
        lines.append(f"| {key} | " + " | ".join(cells) + " | " + " | ".join(deltas) + " |")
    steps = [s["id"] for s in arms[base][0]["steps"]]
    lines += ["", "| step | " + " | ".join(f"{n} {k}" for k, _ in STEP_METRICS for n in names) + " |", "|" + "---|" * (1 + len(STEP_METRICS) * len(names))]
    for sid in steps:
        cells = []
        for _, fn in STEP_METRICS:
            for n in names:
                v = [float(fn(s)) for d in arms[n] for s in d["steps"] if s["id"] == sid]
                cells.append(fmt(statistics.mean(v)) if v else "")
        lines.append(f"| {sid} | " + " | ".join(cells) + " |")
    failed = [f"{n}/{i}: {s['id']} `{c}`" for n in names for i, d in enumerate(arms[n]) for s in d["steps"] for c, r in s["checks"].items() if not r["ok"]]
    if failed:
        lines += ["", "failed checks:", *[f"- {f}" for f in failed]]
    return "\n".join(lines)


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--arm", action="append", default=[])
    ap.add_argument("-n", type=int, default=5)
    ap.add_argument("-j", type=int, default=4)
    ap.add_argument("--model", default="claude-opus-5-5")
    ap.add_argument("--out", default="bench/ab-out")
    ap.add_argument("--report", help="aggregate an existing --out directory and exit")
    args = ap.parse_args()
    if args.report:
        print(report(load(Path(args.report))))
        return 0
    arms = []
    for spec in args.arm:
        name, _, rest = spec.partition("=")
        root, _, binary = rest.partition(":")
        arms.append((name, Path(root).resolve(), Path(binary).resolve()))
    if len(arms) < 1:
        ap.error("give at least one --arm")
    shas = {hashlib.sha256((root / "bench/artbench.py").read_bytes()).hexdigest() for _, root, _ in arms}
    if len(shas) != 1:
        ap.error("arms carry different bench/artbench.py files, so the harness is not frozen")
    out = Path(args.out).resolve()
    jobs = [(name, root, binary, out / name / f"{i:02d}") for i in range(args.n) for name, root, binary in arms]
    jobs = [j for j in jobs if not (j[3] / "run/report.json").exists()]
    with ThreadPoolExecutor(args.j) as pool:
        list(pool.map(lambda j: run_one(*j, args.model), jobs))
    text = report(load(out))
    (out / "summary.md").write_text(text + "\n")
    print(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
