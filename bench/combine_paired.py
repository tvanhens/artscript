# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow==11.3.0"]
# ///
"""Recompute and combine sealed evaluations over disjoint prompt batches.

  uv run bench/combine_paired.py first/run second/run --out bench/out/paired/combined/report.json

The command verifies each source bundle and recomputes its report. All protocol fields except the
suite hash must match, as must both treatments. Duplicate prompt IDs are rejected. A combined
report is an analysis artifact, not a new sealed run or history entry.
"""
import argparse
import json
from pathlib import Path
from paired import canonical, report, save, summarize
from bundle import verify


def combine(paths: list[Path]) -> dict:
    if len(paths) < 2:
        raise ValueError("provide at least two sealed runs")
    metadata = None
    prompts, rows, sources = {}, [], []
    for path in paths:
        root = path.parent if path.name == "report.json" else path
        verified = verify(root)
        result = report(root)
        if not result["summary"]["complete"] or result["protocol_errors"]:
            raise ValueError(f"incomplete or invalid run: {root}")
        exp = result["experiment"]
        protocol = {k: v for k, v in exp["protocol"].items() if k != "suite_sha256"}
        current = {"protocol": protocol, "provenance": exp["provenance"], "mode": exp["mode"]}
        if metadata is not None and metadata != current:
            raise ValueError("experiment metadata differs")
        metadata = current
        for key, prompt in exp["prompts"]:
            if key in prompts:
                raise ValueError(f"duplicate prompt ID: {key}")
            prompts[key] = prompt
        rows.extend(result["pairs"])
        sources.append({"bundle_sha256": canonical(verified), "path": str(root)})
    repetitions = metadata["protocol"]["repetitions"]
    summary = summarize(rows, len(prompts) * repetitions)
    return {"analysis_version": "prompt-cluster-v2", "experiment": metadata | {"prompts": list(prompts.items())},
            "sources": sources, "protocol_errors": [], "summary": summary,
            "per_prompt": {key: summarize([r for r in rows if r["prompt_id"] == key], repetitions) for key in prompts}, "pairs": rows}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("reports", nargs="+", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = combine(args.reports)
        save(args.out if args.out.suffix == ".json" else args.out / "report.json", result)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        parser.error(str(exc))
    print(json.dumps(result["summary"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
