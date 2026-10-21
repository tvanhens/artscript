# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow"]
# ///
"""Combine completed paired evaluations over disjoint prompt batches."""
from __future__ import annotations

import argparse
import json
from pathlib import Path

from paired import ARMS, digest, save, summarize


def combine(paths: list[Path]) -> dict:
    if len(paths) < 2:
        raise ValueError("provide at least two completed reports")
    metadata = None
    prompts, rows, sources = {}, [], []
    pair_ids = set()
    for path in paths:
        report = json.loads(path.read_text())
        if not report["summary"]["complete"] or report.get("protocol_errors") or report["summary"].get("protocol_errors"):
            raise ValueError(f"incomplete or invalid report: {path}")
        experiment = report["experiment"]
        provenance = experiment["provenance"]
        current = {key: experiment[key] for key in (
            "harness_sha256", "rubric_sha256", "model", "judge_model", "invocation_form", "repetitions")}
        current["provenance"] = {key: provenance[key] for key in (
            *ARMS, "binary_sha256", "model_resolution", "judge_provider", "judge_model_resolution")}
        if metadata is not None and metadata != current:
            raise ValueError(f"experiment metadata differs: {path}")
        metadata = current
        for key, prompt in experiment["prompts"]:
            if key in prompts:
                raise ValueError(f"duplicate prompt ID: {key}")
            prompts[key] = prompt
        for row in report["pairs"]:
            identity = (row["prompt_id"], row["repetition"])
            if identity in pair_ids:
                raise ValueError(f"duplicate pair: {identity}")
            pair_ids.add(identity)
            rows.append(row)
        sources.append({"path": str(path.resolve()), "sha256": digest(path)})
    expected = {(key, rep) for key in prompts for rep in range(metadata["repetitions"])}
    if pair_ids != expected:
        raise ValueError("pair receipts do not cover the combined prompt schedule")
    summary = summarize(rows, len(expected))
    if not summary["complete"]:
        raise ValueError("pair receipts are incomplete")
    return {"experiment": metadata | {"prompts": list(prompts.items())}, "sources": sources,
            "protocol_errors": [], "summary": summary,
            "per_prompt": {key: summarize([r for r in rows if r["prompt_id"] == key], metadata["repetitions"])
                           for key in prompts}, "pairs": rows}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reports", nargs="+", type=Path)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    try:
        result = combine(args.reports)
    except (OSError, ValueError, KeyError) as exc:
        parser.error(str(exc))
    output = args.out if args.out.suffix == ".json" else args.out / "report.json"
    save(output, result)
    print(json.dumps(result["summary"], indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
