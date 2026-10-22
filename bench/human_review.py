# /// script
# requires-python = ">=3.11"
# dependencies = ["pillow==11.3.0"]
# ///
#!/usr/bin/env python3
"""Export blinded local review packets and summarize real human annotations.

  uv run bench/human_review.py export bench/out/paired/round-01 bench/out/human/round-01
  uv run bench/human_review.py import bench/out/human/round-01 /path/to/completed-answers.json

Share only the packet's `public/` directory with reviewers. Its HTML shows anonymous images and
prompts. Reviewers fill the provided answer schema with a reviewer identifier and left, right or
tie preferences. Keep `private.json` private until annotation is finished. Imports validate item
identities and retained images, and write immutable `calibrations/` receipts tied to the exact
packet and answers. No annotations means calibration is absent. Agreement is reported against
unanimous machine orientation votes. Observed left/right counts describe position preference, not
measured swap sensitivity. Multiple reviewers and repeated blinded judgments are needed to assess
human reliability.
"""
import argparse
import hashlib
import html
import json
import random
import shutil
from pathlib import Path


PREFERENCES = {"left", "right", "tie"}


def read(path):
    return json.loads(Path(path).read_text())


def write(path, value):
    Path(path).write_text(json.dumps(value, indent=2, sort_keys=True) + "\n")


def contained(root, name):
    path = Path(name)
    if path.is_absolute():
        raise ValueError("packet and bundle paths must be relative")
    resolved = (root / path).resolve()
    if not resolved.is_relative_to(root.resolve()):
        raise ValueError("path escapes its bundle or packet")
    return resolved


def export_packet(run, out):
    from bundle import verify
    from paired import report

    run, out = Path(run).resolve(), Path(out).resolve()
    verify(run)
    result = report(run)
    if not result["summary"]["complete"] or result.get("protocol_errors"):
        raise ValueError("human review requires a complete verified run")
    if out.exists():
        raise ValueError("review packet output already exists")
    rows = [row for row in result["pairs"]
            if all(row["generations"][arm]["status"] == "success" for arm in ("baseline", "candidate"))]
    rng = random.SystemRandom()
    rng.shuffle(rows)
    public = out / "public"
    images = public / "images"
    images.mkdir(parents=True)
    items, mapping, sections = [], {}, []
    for index, row in enumerate(rows):
        item_id = f"item-{index + 1:04d}"
        order = ["baseline", "candidate"]
        rng.shuffle(order)
        paths = {}
        for side, arm in zip(("left", "right"), order):
            source = contained(run, row["generations"][arm]["image"])
            target = images / f"{item_id}-{side}.png"
            shutil.copyfile(source, target)
            paths[side] = f"images/{target.name}"
        votes = row["judgment"]["preferences"]
        machine = votes[0] if votes and len(set(votes)) == 1 else None
        mapping[item_id] = {"prompt_id": row["prompt_id"], "repetition": row["repetition"],
                            "left": order[0], "right": order[1], "machine_preference": machine}
        items.append({"id": item_id, "prompt": row["prompt"], **paths})
        sections.append(f'<section><h2>{item_id}</h2><p>{html.escape(row["prompt"])}</p>'
                        f'<div><figure><figcaption>Left</figcaption><img src="{paths["left"]}"></figure>'
                        f'<figure><figcaption>Right</figcaption><img src="{paths["right"]}"></figure></div></section>')
    packet = {"schema_version": 1, "items": items}
    write(public / "packet.json", packet)
    write(public / "answers.json", {"reviewer": "", "annotations": [{"id": i["id"], "preference": None} for i in items]})
    (public / "index.html").write_text('<!doctype html><meta charset="utf-8"><title>Blind art review</title>'
        '<style>body{font:18px system-ui;max-width:1100px;margin:auto}section{border-bottom:1px solid #aaa;padding:24px 0}'
        'section div{display:flex}figure{width:50%;margin:8px}img{max-width:100%;max-height:550px}</style>'
        '<h1>Blind art review</h1><p>Judge how well each image fulfills the prompt, including visual quality. '
        'Record left, right, or tie in answers.json. Leave uncertain items unanswered. '
        'Use a real reviewer name or stable pseudonym. Share only this public directory with reviewers.</p>' + ''.join(sections))
    hashes = {str(p.relative_to(public)): hashlib.sha256(p.read_bytes()).hexdigest()
              for p in sorted(public.rglob("*")) if p.is_file() and p.name != "answers.json"}
    write(out / "private.json", {"schema_version": 1, "items": mapping, "public_sha256": hashes,
                                "bundle_sha256": hashlib.sha256((run / "seal.json").read_bytes()).hexdigest()})
    return {"items": len(items), "calibration_status": "absent", "public_directory": str(public)}


def import_answers(packet, answers):
    packet = Path(packet).resolve()
    private_bytes = (packet / "private.json").read_bytes()
    private = json.loads(private_bytes)
    packet_sha256 = hashlib.sha256(private_bytes).hexdigest()
    for name, expected in private["public_sha256"].items():
        if hashlib.sha256(contained(packet / "public", name).read_bytes()).hexdigest() != expected:
            raise ValueError("review packet has changed")
    answers_bytes = Path(answers).read_bytes()
    data = json.loads(answers_bytes)
    answers_sha256 = hashlib.sha256(answers_bytes).hexdigest()
    if not isinstance(data, dict) or set(data) != {"reviewer", "annotations"}:
        raise ValueError("answers require reviewer and annotations")
    if not isinstance(data["reviewer"], str) or not isinstance(data["annotations"], list):
        raise ValueError("invalid reviewer or annotations")
    seen, observations = set(), []
    for item in data["annotations"]:
        if not isinstance(item, dict) or set(item) != {"id", "preference"} or not isinstance(item["id"], str):
            raise ValueError("invalid annotation")
        key, preference = item["id"], item["preference"]
        if key in seen or key not in private["items"]:
            raise ValueError("duplicate or unknown annotation ID")
        seen.add(key)
        if preference is None:
            continue
        if not isinstance(preference, str) or preference not in PREFERENCES:
            raise ValueError("preference must be left, right, tie, or null")
        if not data["reviewer"].strip():
            raise ValueError("completed annotations require a reviewer")
        mapping = private["items"][key]
        human = "tie" if preference == "tie" else mapping[preference]
        observations.append({"id": key, "human_preference": human, "position": preference,
                             "machine_preference": mapping["machine_preference"]})
    compared = [o for o in observations if o["machine_preference"] is not None]
    result = {"schema_version": 1, "status": "observed" if observations else "absent",
              "reviewer": data["reviewer"], "answers_sha256": answers_sha256,
              "packet_sha256": packet_sha256,
              "annotated_pairs": len(observations),
              "available_pairs": len(private["items"]), "compared_pairs": len(compared),
              "machine_orientation_disagreements": sum(o["machine_preference"] is None for o in observations),
              "agreement": sum(o["human_preference"] == o["machine_preference"] for o in compared) / len(compared) if compared else None,
              "observed_position_counts": {side: sum(o["position"] == side for o in observations) for side in sorted(PREFERENCES)},
              "bundle_sha256": private["bundle_sha256"], "observations": observations,
              "interpretation": "Observed agreement on annotated successful pairs only. Position counts describe choices in one presentation per pair, not sensitivity to swapping positions. Reviewer independence and representativeness are not established by this tool."}
    destination = packet / "calibrations" / f"{packet_sha256}-{answers_sha256}.json"
    destination.parent.mkdir(exist_ok=True)
    serialized = json.dumps(result, indent=2, sort_keys=True) + "\n"
    try:
        with destination.open("x") as stream:
            stream.write(serialized)
    except FileExistsError:
        if destination.read_text() != serialized:
            raise ValueError("existing calibration receipt differs; refusing to overwrite")
    return result


def main():
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    commands = parser.add_subparsers(dest="command", required=True)
    export = commands.add_parser("export")
    export.add_argument("run", type=Path)
    export.add_argument("out", type=Path)
    ingest = commands.add_parser("import")
    ingest.add_argument("packet", type=Path)
    ingest.add_argument("answers", type=Path)
    args = parser.parse_args()
    try:
        result = export_packet(args.run, args.out) if args.command == "export" else import_answers(args.packet, args.answers)
    except (ValueError, OSError, KeyError, TypeError) as exc:
        parser.exit(1, f"human review failed: {exc}\n")
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
