# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow==11.3.0"]
# ///
r"""Verify portable experiment receipts and record comparable benchmark history.

  uv run bench/bundle.py record bench/out/paired/round-01 \
    --out bench/history/round-01.json
  uv run bench/bundle.py history bench/history/*.json

History records are immutable compact JSON files. Recording requires a verified, complete strict
run. Keep records outside the run bundle. Groups match the protocol, suite, compute settings and
environment. Fixed-image runs must share the reference fingerprint. Fresh pairs must share the
baseline skill and binary. Candidate hashes may differ. Compare paired candidate-minus-reference
deltas within a group. Absolute judge scores from different paired contexts or unmatched baselines
are not a trend.

For combining disjoint prompt batches, see `uv run bench/combine_paired.py --help`.
"""
from __future__ import annotations

import argparse
from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path

EXCLUDED = {"seal.json", "report.json", ".lock"}


def fingerprint(value: object) -> str:
    return hashlib.sha256(json.dumps(value, sort_keys=True, separators=(",", ":"),
                                     allow_nan=False).encode()).hexdigest()


def inventory(root: Path) -> dict:
    if root.is_symlink():
        raise ValueError("bundle root is a symlink")
    files = {}
    for path in sorted(root.rglob("*")):
        if path.is_symlink():
            raise ValueError(f"bundle contains a symlink: {path.relative_to(root)}")
        if path.is_dir():
            continue
        if not path.is_file():
            raise ValueError(f"bundle contains a nonregular file: {path.relative_to(root)}")
        relative = path.relative_to(root).as_posix()
        if relative in EXCLUDED:
            continue
        data = path.read_bytes()
        files[relative] = {"sha256": hashlib.sha256(data).hexdigest(), "size": len(data)}
    return files


def manifest(root: Path) -> dict:
    value = json.loads((root / "experiment.json").read_text())
    if not isinstance(value, dict) or value.get("schema_version") != 2:
        raise ValueError("only schema_version 2 bundles can be sealed")
    if not isinstance(value.get("protocol"), dict) or value.get("protocol_key") != fingerprint(value["protocol"]):
        raise ValueError("experiment protocol fingerprint mismatch")
    return value


def seal(root: Path) -> dict:
    root = Path(root)
    if (root / "seal.json").exists():
        return verify(root)
    manifest(root)
    value = {"schema_version": 1, "files": inventory(root)}
    value["fingerprint"] = fingerprint(value)
    with (root / "seal.json").open("x") as stream:
        json.dump(value, stream, indent=2, allow_nan=False)
        stream.write("\n")
    return value


def verify(root: Path) -> dict:
    root = Path(root)
    value = json.loads((root / "seal.json").read_text())
    if not isinstance(value, dict) or value.get("schema_version") != 1:
        raise ValueError("unsupported bundle seal")
    unsigned = {key: entry for key, entry in value.items() if key != "fingerprint"}
    if value.get("fingerprint") != fingerprint(unsigned):
        raise ValueError("bundle seal fingerprint mismatch")
    expected = value.get("files")
    if not isinstance(expected, dict):
        raise ValueError("bundle inventory missing")
    for name in expected:
        path = Path(name)
        if path.is_absolute() or ".." in path.parts or path.as_posix() != name or name in EXCLUDED:
            raise ValueError(f"invalid bundle inventory path: {name}")
    actual = inventory(root)
    missing = sorted(expected.keys() - actual.keys())
    unexpected = sorted(actual.keys() - expected.keys())
    changed = sorted(name for name in expected.keys() & actual.keys() if expected[name] != actual[name])
    if missing or unexpected or changed:
        raise ValueError(f"bundle integrity failed; missing={missing}, unexpected={unexpected}, changed={changed}")
    manifest(root)
    return value


@contextmanager
def lock(root: Path):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    path = root / ".lock"
    fd = os.open(path, os.O_CREAT | os.O_RDWR | os.O_NOFOLLOW, 0o600)
    with os.fdopen(fd, "a") as stream:
        try:
            fcntl.flock(stream, fcntl.LOCK_EX | fcntl.LOCK_NB)
        except BlockingIOError as exc:
            raise ValueError(f"experiment is locked: {root}") from exc
        try:
            yield
        finally:
            fcntl.flock(stream, fcntl.LOCK_UN)


def compatibility(experiment: dict) -> dict:
    if experiment.get("schema_version") != 2 or experiment.get("mode") != "strict":
        raise ValueError("history requires a strict schema_version 2 experiment")
    protocol = experiment.get("protocol", {})
    if experiment.get("protocol_key") != fingerprint(protocol):
        raise ValueError("history protocol fingerprint mismatch")
    if any(not isinstance(protocol.get(key), str) or not protocol[key].strip()
           for key in ("model", "judge_model", "effort", "judge_effort")):
        raise ValueError("history requires explicit models and reasoning efforts")
    provenance = experiment["provenance"]
    if protocol.get("kind") == "fixed-image-reference":
        return {"protocol_key": experiment["protocol_key"], "reference_fingerprint": protocol["reference_fingerprint"]}
    return {"protocol_key": experiment["protocol_key"], "baseline": provenance["baseline"],
            "baseline_binary": provenance["binaries"]["baseline"]}


def history_record(root: Path, dest: Path) -> dict:
    import paired

    root, dest = Path(root).resolve(), Path(dest).resolve()
    if dest.is_relative_to(root):
        raise ValueError("history records must be outside the sealed bundle")
    with lock(root):
        sealed = verify(root)
        result = paired.report(root)
        experiment = result["experiment"]
        group = compatibility(experiment)
        if not result["summary"].get("complete") or result.get("protocol_errors"):
            raise ValueError("history requires a complete verified experiment")
        record = {"schema_version": 1, "bundle_fingerprint": sealed["fingerprint"],
                  "compatibility": group, "compatibility_key": fingerprint(group),
                  "experiment": experiment, "summary": result["summary"],
                  "per_prompt": result["per_prompt"], "resources": result.get("resources"), "report_fingerprint": fingerprint(result)}
        record["fingerprint"] = fingerprint(record)
        dest.parent.mkdir(parents=True, exist_ok=True)
        with dest.open("x") as stream:
            json.dump(record, stream, indent=2, allow_nan=False)
            stream.write("\n")
        return record


def history(paths: list[Path]) -> dict:
    groups = {}
    seen = set()
    for path in paths:
        record = json.loads(Path(path).read_text())
        if not isinstance(record, dict):
            raise ValueError(f"invalid history record: {path}")
        unsigned = {key: value for key, value in record.items() if key != "fingerprint"}
        if record.get("schema_version") != 1 or record.get("fingerprint") != fingerprint(unsigned):
            raise ValueError(f"history record fingerprint mismatch: {path}")
        group = compatibility(record["experiment"])
        key = fingerprint(group)
        if record.get("compatibility") != group or record.get("compatibility_key") != key:
            raise ValueError(f"history compatibility mismatch: {path}")
        if not record["summary"].get("complete"):
            raise ValueError(f"history record is incomplete: {path}")
        if record["bundle_fingerprint"] in seen:
            raise ValueError(f"duplicate experiment in history: {path}")
        seen.add(record["bundle_fingerprint"])
        groups.setdefault(key, {"compatibility": group, "records": []})["records"].append(
            {"record": str(path), "bundle_fingerprint": record["bundle_fingerprint"],
             "candidate": record["experiment"]["provenance"]["candidate"],
             "candidate_binary": record["experiment"]["provenance"]["binaries"]["candidate"],
             "created_at_utc": record["experiment"].get("created_at_utc", record["experiment"].get("created_at")),
             "resources": record.get("resources"), "summary": record["summary"]})
    for group in groups.values():
        group["records"].sort(key=lambda record: (record["created_at_utc"] is None, record["created_at_utc"] or ""))
    return {"schema_version": 1, "groups": groups,
            "interpretation": "Compare paired deltas within a group. Groups use different protocols or references."}


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = parser.add_subparsers(dest="action", required=True)
    for action in ("seal", "verify"):
        sub.add_parser(action).add_argument("root", type=Path)
    record = sub.add_parser("record")
    record.add_argument("root", type=Path)
    record.add_argument("--out", type=Path, required=True)
    trend = sub.add_parser("history")
    trend.add_argument("records", nargs="+", type=Path)
    args = parser.parse_args()
    try:
        if args.action == "record":
            result = history_record(args.root, args.out)
        elif args.action == "history":
            result = history(args.records)
        else:
            with lock(args.root):
                result = globals()[args.action](args.root)
    except (OSError, ValueError, KeyError, TypeError) as exc:
        parser.exit(1, f"{exc}\n")
    print(json.dumps(result, indent=2, allow_nan=False))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
