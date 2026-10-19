#!/usr/bin/env python3
"""Time cold `ramose build` runs of the catacombs scene and check that every run writes the same bytes.

    python3 bench/render/bench.py [--runs 5] [--backend cpu] BINARY [BINARY...]

Exits 1 when two runs of one binary write different outputs.
"""

import argparse
import hashlib
import json
import resource
import shutil
import statistics
import subprocess
import sys
import tempfile
import time
from pathlib import Path

SCENE = Path(__file__).resolve().parent / "catacombs"


def build(binary: Path, backend: str) -> tuple[float, float, dict[str, str]]:
    with tempfile.TemporaryDirectory(prefix="ramose-bench-") as tmp:
        project = Path(tmp) / "catacombs"
        shutil.copytree(SCENE, project, ignore=shutil.ignore_patterns("assets", ".ramose"))
        before = resource.getrusage(resource.RUSAGE_CHILDREN)
        start = time.perf_counter()
        done = subprocess.run(
            [str(binary), "build", "--backend", backend],
            cwd=project,
            capture_output=True,
            text=True,
        )
        real = time.perf_counter() - start
        after = resource.getrusage(resource.RUSAGE_CHILDREN)
        if done.returncode != 0:
            sys.exit(f"{binary}: build exited {done.returncode}\n{done.stdout}{done.stderr}")
        cpu = (after.ru_utime - before.ru_utime) + (after.ru_stime - before.ru_stime)
        assets = project / "assets"
        hashes = {
            p.name: hashlib.sha256(p.read_bytes()).hexdigest()
            for p in sorted(assets.iterdir())
            if p.suffix == ".png"
        }
        if len(hashes) != 2:
            sys.exit(f"{binary}: expected 2 PNG outputs, got {sorted(hashes)}")
        return real, cpu, hashes


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("binaries", nargs="+", type=Path)
    ap.add_argument("--runs", type=int, default=5)
    ap.add_argument("--backend", default="cpu")
    args = ap.parse_args()

    results = {str(b): {"real": [], "cpu": [], "hashes": []} for b in args.binaries}
    for run in range(args.runs):
        for binary in args.binaries:
            real, cpu, hashes = build(binary.resolve(), args.backend)
            r = results[str(binary)]
            r["real"].append(real)
            r["cpu"].append(cpu)
            r["hashes"].append(hashes)
            print(f"run {run + 1} {binary}: real {real:.2f}s cpu {cpu:.2f}s", flush=True)

    deterministic = True
    for binary, r in results.items():
        same = all(h == r["hashes"][0] for h in r["hashes"])
        deterministic &= same
        print(
            f"{binary}: real median {statistics.median(r['real']):.2f}s "
            f"(range {min(r['real']):.2f}-{max(r['real']):.2f}), "
            f"cpu median {statistics.median(r['cpu']):.2f}s, "
            f"outputs {'identical' if same else 'DIFFER'} across {len(r['real'])} runs"
        )
        print("  " + json.dumps(r["hashes"][0]))
    if len(results) > 1:
        first = next(iter(results.values()))["hashes"][0]
        for binary, r in results.items():
            if r["hashes"][0] != first:
                print(f"{binary}: outputs differ from {args.binaries[0]}")
    sys.exit(0 if deterministic else 1)


if __name__ == "__main__":
    main()
