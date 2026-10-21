import json
import os
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


BENCH = Path(__file__).resolve().parent
FAKE_CODEX = r'''
import json, os, subprocess, sys
from pathlib import Path
from PIL import Image
args = sys.argv[1:]
if args == ["--version"]:
    print("codex-cli test")
    raise SystemExit(0)
with open(os.environ["FAKE_CALLS"], "a") as stream:
    stream.write(json.dumps(args) + "\n")
if os.environ.get("FAKE_CAPACITY") or (os.environ.get("FAKE_JUDGE_CAPACITY") and "--output-schema" in args):
    print(json.dumps({"type": "turn.failed", "error": {"message": "at capacity"}}))
    raise SystemExit(1)
cwd = Path(args[args.index("-C") + 1])
if "--output-schema" in args:
    schema = json.loads(Path(args[args.index("--output-schema") + 1]).read_text())
    paired = "left" in schema["properties"]
    score_schema = schema["properties"]["left"] if paired else schema
    def score(name):
        value = 16 if Image.open(cwd / (name + ".png")).getpixel((0, 0))[0] else 12
        return {key: "test image" if key == "notes" else value for key in score_schema["properties"]}
    result = {"left": score("left"), "right": score("right"), "preference": "left" if score("left")["form"] > score("right")["form"] else "right"} if paired else score("left")
    Path(args[args.index("-o") + 1]).write_text(json.dumps(result))
else:
    (cwd / "ramose.toml").write_text('[project]\nout = "assets"\n')
    (cwd / "art.ramo").write_text("candidate" if "candidate" in (cwd / ".agents/skills/ramose/SKILL.md").read_text() else "baseline")
    if not os.environ.get("FAKE_INVALID_ART"):
        subprocess.run(["ramose", "build"], cwd=cwd, check=True)
print(json.dumps({"type": "turn.completed", "usage": {"input_tokens": 20, "output_tokens": 10}}))
'''
FAKE_RENDERER = r'''
import json, os, sys
from pathlib import Path
from PIL import Image
if os.environ.get("FAKE_RENDER_CALLS"):
    with open(os.environ["FAKE_RENDER_CALLS"], "a") as stream:
        stream.write(json.dumps(sys.argv[1:]) + "\n")
if sys.argv[1:] == ["--version"]:
    print("ramose test")
elif sys.argv[1:] == ["build"]:
    Path("assets").mkdir(exist_ok=True)
    red = 255 if Path("art.ramo").read_text() == "candidate" else 0
    Image.new("RGBA", (4, 4), (red, 20, 30, 255)).save("assets/art.png")
'''


class PairedCliTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.out = self.root / "experiment"
        self.calls = self.root / "calls.jsonl"
        bindir = self.root / "bin"
        bindir.mkdir()
        for name, script in (("codex", FAKE_CODEX), ("ramose", FAKE_RENDERER)):
            path = bindir / name
            path.write_text(f"#!{sys.executable}\n" + script)
            path.chmod(0o755)
        for arm in ("baseline", "candidate"):
            skill = self.root / arm
            skill.mkdir()
            (skill / "SKILL.md").write_text(arm)
        self.prompts = self.root / "prompts.json"
        self.prompts.write_text(json.dumps({"boat": "draw a boat", "fern": "draw a fern"}))
        self.env = dict(os.environ, PATH=str(bindir) + os.pathsep + os.environ["PATH"],
                        FAKE_CALLS=str(self.calls), PYTHONDONTWRITEBYTECODE="1")
        self.env.pop("FAKE_CAPACITY", None)
        self.run_args = ["run", "--baseline", str(self.root / "baseline"), "--candidate", str(self.root / "candidate"),
                         "--ramose", str(bindir / "ramose"), "--prompts", str(self.prompts), "--out", str(self.out),
                         "--repetitions", "1", "--jobs", "1"]
        self.pins = ["--model", "test-codex", "--judge-model", "test-codex-judge", "--effort", "high", "--judge-effort", "medium"]

    def cli(self, *args, script="paired.py", expected=0):
        result = subprocess.run([sys.executable, str(BENCH / script), *map(str, args)],
                                env=self.env, capture_output=True, text=True, timeout=60)
        self.assertEqual(result.returncode, expected, result.stdout + result.stderr)
        return result

    def run_experiment(self):
        return self.cli(*self.run_args, *self.pins)

    def recorded_calls(self):
        return [json.loads(line) for line in self.calls.read_text().splitlines()]

    def test_strict_run_requires_all_model_and_effort_pins(self):
        result = self.cli(*self.run_args, expected=2)
        self.assertIn("strict runs require", result.stderr)
        self.assertFalse(self.calls.exists())

    def test_run_report_relocation_and_history(self):
        self.run_experiment()
        report = json.loads((self.out / "report.json").read_text())
        self.assertTrue(report["summary"]["complete"])
        self.assertEqual(report["summary"]["scheduled_pairs"], 2)
        self.assertEqual(report["summary"]["arms"]["baseline"]["mean"], 12)
        self.assertEqual(report["summary"]["arms"]["candidate"]["mean"], 16)
        self.assertEqual(report["summary"]["paired_delta_mean"], 4)
        self.assertEqual(report["summary"]["paired_delta_ci95"], [4, 4])
        calls = self.recorded_calls()
        self.assertEqual(len(calls), 8)
        for call in calls:
            judge = "--output-schema" in call
            self.assertEqual(call[call.index("--model") + 1], "test-codex-judge" if judge else "test-codex")
            self.assertEqual(call[call.index("-c") + 1], 'model_reasoning_effort="medium"' if judge else 'model_reasoning_effort="high"')
        moved = self.root / "moved"
        shutil.move(self.out, moved)
        self.cli("report", moved)
        self.cli("verify", moved)
        record = self.root / "history.json"
        self.cli("record", moved, "--out", record, script="bundle.py")
        history = json.loads(self.cli("history", record, script="bundle.py").stdout)
        groups = list(history["groups"].values())
        self.assertEqual(len(groups), 1)
        self.assertEqual(groups[0]["records"][0]["summary"]["paired_delta_mean"], 4)
        self.cli("history", record, record, script="bundle.py", expected=1)
        self.assertEqual(self.recorded_calls(), calls)

    def test_tampered_pair_prompt_or_score_and_seal_are_rejected(self):
        self.run_experiment()
        for field in ("score", "prompt", "seal", "malformed"):
            with self.subTest(field=field):
                copy = self.root / field
                shutil.copytree(self.out, copy)
                path = copy / "seal.json" if field == "seal" else copy / "pairs/000-00/pair.json"
                value = json.loads(path.read_text())
                if field == "seal":
                    value["fingerprint"] = "0" * 64
                elif field == "score":
                    value["judgment"]["scores"]["candidate"] = 20
                else:
                    value["prompt"] = "a changed prompt"
                path.write_text("invalid JSON" if field == "malformed" else json.dumps(value))
                self.cli("report", copy, expected=1)
                report = json.loads((copy / "report.json").read_text())
                self.assertFalse(report["summary"]["complete"])
                self.assertIsNone(report["summary"]["paired_delta_mean"])
                self.assertTrue(report["protocol_errors"])

    def test_resealed_injected_score_fails_judge_receipt_validation(self):
        self.run_experiment()
        path = self.out / "pairs/000-00/pair.json"
        value = json.loads(path.read_text())
        value["judgment"]["scores"]["candidate"] = 20
        path.write_text(json.dumps(value))
        (self.out / "seal.json").unlink()
        self.cli("seal", self.out, script="bundle.py")
        self.cli("report", self.out, expected=1)
        result = json.loads((self.out / "report.json").read_text())
        self.assertIn("pair judgment contradicts retained judge outputs", result["protocol_errors"])
        self.assertNotIn("mean", next(iter(result["per_prompt"].values()))["arms"]["baseline"])

    def test_resume_rejects_changed_manifest_before_model_calls(self):
        self.run_experiment()
        (self.out / "seal.json").unlink()
        path = self.out / "experiment.json"
        value = json.loads(path.read_text())
        value["model"] = "changed"
        path.write_text(json.dumps(value))
        calls = self.recorded_calls()
        self.cli("resume", self.out, expected=2)
        self.assertEqual(self.recorded_calls(), calls)

    def test_resume_completed_run_does_not_invoke_models(self):
        self.run_experiment()
        calls = self.recorded_calls()
        self.cli("resume", self.out)
        self.assertEqual(self.recorded_calls(), calls)

    def test_resume_interrupted_pair_preserves_completed_arm(self):
        self.run_experiment()
        pair = self.out / "pairs/000-00"
        baseline = (pair / "baseline/generation.json").read_bytes()
        (self.out / "seal.json").unlink()
        (pair / "pair.json").unlink()
        (pair / "checkpoint.json").unlink()
        shutil.rmtree(pair / "candidate")
        calls = self.recorded_calls()
        self.cli("resume", self.out)
        self.assertEqual((pair / "baseline/generation.json").read_bytes(), baseline)
        new_calls = self.recorded_calls()[len(calls):]
        self.assertEqual(sum("--output-schema" not in call for call in new_calls), 1)
        self.assertEqual(sum("--output-schema" in call for call in new_calls), 2)
        report = json.loads((self.out / "report.json").read_text())
        self.assertTrue(report["summary"]["complete"])
        self.assertEqual(report["summary"]["paired_delta_mean"], 4)

    def test_replay_recovers_judge_outage_without_author_reruns(self):
        self.env["FAKE_JUDGE_CAPACITY"] = "1"
        self.cli(*self.run_args, *self.pins, expected=1)
        self.env.pop("FAKE_JUDGE_CAPACITY")
        calls = self.recorded_calls()
        replay = self.root / "replayed"
        args = list(self.run_args)
        args[args.index("--out") + 1] = str(replay)
        self.cli(*args, *self.pins, "--replay", self.out)
        new_calls = self.recorded_calls()[len(calls):]
        self.assertEqual(len(new_calls), 4)
        self.assertTrue(all("--output-schema" in call for call in new_calls))
        result = json.loads((replay / "report.json").read_text())
        self.assertEqual(result["summary"]["paired_delta_mean"], 4)
        self.assertEqual(result["resources"]["groups"]["author"]["calls"], 0)
        self.assertEqual(result["resources"]["groups"]["reused_author"]["calls"], 4)

    def test_combine_recomputes_mutable_reports(self):
        self.run_experiment()
        report_path = self.out / "report.json"
        value = json.loads(report_path.read_text())
        value["summary"]["paired_delta_mean"] = 999
        report_path.write_text(json.dumps(value))
        second = self.root / "second"
        self.prompts.write_text(json.dumps({"cloud": "draw a cloud"}))
        args = list(self.run_args)
        args[args.index("--out") + 1] = str(second)
        self.cli(*args, *self.pins)
        combined = self.root / "combined.json"
        self.cli(self.out, second, "--out", combined, script="combine_paired.py")
        result = json.loads(combined.read_text())
        self.assertEqual(result["summary"]["paired_delta_mean"], 4)
        self.assertEqual(result["summary"]["prompt_count"], 3)
        self.cli(self.out, self.out, "--out", combined, script="combine_paired.py", expected=2)

    def test_provider_failure_is_inconclusive(self):
        self.env["FAKE_CAPACITY"] = "1"
        self.cli(*self.run_args, *self.pins, expected=1)
        report = json.loads((self.out / "report.json").read_text())
        self.assertFalse(report["summary"]["complete"])
        self.assertIsNone(report["summary"]["paired_delta_mean"])
        self.assertEqual(report["summary"]["arms"]["baseline"]["infrastructure_errors"], 2)
        self.assertEqual(report["summary"]["arms"]["baseline"]["failures"], 0)
        self.cli("record", self.out, "--out", self.root / "history.json", script="bundle.py", expected=1)

    def reference_args(self, *, repetition=0):
        self.run_experiment()
        reference = self.root / "reference"
        self.cli("reference", self.out, "--arm", "baseline", "--repetition", repetition,
                 "--source-commit", "a" * 40, "--out", reference)
        args = list(self.run_args)
        args[args.index("--baseline")] = "--reference"
        args[args.index(str(self.root / "baseline"))] = str(reference)
        args[args.index("--out") + 1] = str(self.root / "reference-run")
        index = args.index("--prompts")
        del args[index:index + 2]
        return reference, args

    def test_reference_fresh_scores_and_only_candidate_authoring(self):
        import paired
        reference, args = self.reference_args()
        path = reference / "reference.json"
        value = json.loads(path.read_text())
        for item in value["prompts"]:
            item["original_score"] = 1
        value["fingerprint"] = paired.canonical({k: v for k, v in value.items() if k != "fingerprint"})
        path.write_text(json.dumps(value))
        before = len(self.recorded_calls())
        render_calls = self.root / "render-calls.jsonl"
        self.env["FAKE_RENDER_CALLS"] = str(render_calls)
        self.cli(*args, *self.pins)
        calls = self.recorded_calls()[before:]
        self.assertEqual(sum("--output-schema" not in call for call in calls), 2)
        self.assertEqual(sum("--output-schema" in call for call in calls), 4)
        renders = [json.loads(line) for line in render_calls.read_text().splitlines()]
        self.assertEqual(renders.count(["build"]), 4)
        self.assertEqual(renders.count(["check"]), 2)
        run = self.root / "reference-run"
        result = json.loads((run / "report.json").read_text())
        self.assertEqual(result["summary"]["arms"]["baseline"]["mean"], 12)
        self.assertEqual(result["summary"]["arms"]["baseline"]["successes"], 0)
        self.assertEqual(result["summary"]["arms"]["baseline"]["reference_uses"], 2)
        self.assertEqual(result["summary"]["paired_delta_mean"], 4)
        self.assertEqual(result["resources"]["groups"]["author"]["calls"], 2)
        self.assertEqual(result["resources"]["groups"]["reused_author"]["calls"], 0)
        self.assertFalse((run / "inputs/baseline").exists())
        self.assertFalse((run / "pairs/000-00/baseline/codex.json").exists())
        shutil.rmtree(reference)
        moved = self.root / "moved-reference-run"
        shutil.move(run, moved)
        self.cli("report", moved)
        self.cli("verify", moved)
        self.assertEqual(len(self.recorded_calls()), before + 6)

    def test_reference_tampering_and_prompt_mismatch_prevent_model_calls(self):
        import paired
        reference, args = self.reference_args()
        manifest_path = reference / "reference.json"
        original = json.loads(manifest_path.read_text())
        before = self.recorded_calls()
        image = reference / original["prompts"][0]["image"]
        original_bytes = image.read_bytes()
        image.write_bytes(b"tampered")
        self.cli(*args, *self.pins, expected=2)
        image.write_bytes(original_bytes)
        self.prompts.write_text(json.dumps({"wrong": "wrong prompt"}))
        self.cli(*args, *self.pins, "--prompts", self.prompts, expected=2)
        for defect in ("duplicate", "escape", "invalid_png"):
            value = json.loads(json.dumps(original))
            if defect == "duplicate":
                value["prompts"][1]["id"] = value["prompts"][0]["id"]
            elif defect == "escape":
                value["prompts"][0]["image"] = "../outside.png"
            else:
                image.write_bytes(b"invalid PNG")
                value["prompts"][0]["image_sha256"] = paired.digest(image)
            value["fingerprint"] = paired.canonical({k: v for k, v in value.items() if k != "fingerprint"})
            manifest_path.write_text(json.dumps(value))
            self.cli(*args, *self.pins, expected=2)
        self.assertEqual(self.recorded_calls(), before)

    def test_reference_resume_replay_and_history(self):
        reference, args = self.reference_args()
        self.cli(*args, *self.pins)
        run = self.root / "reference-run"
        pair = run / "pairs/000-00"
        baseline = (pair / "baseline/generation.json").read_bytes()
        (run / "seal.json").unlink()
        (pair / "pair.json").unlink()
        (pair / "checkpoint.json").unlink()
        shutil.rmtree(pair / "candidate")
        before = len(self.recorded_calls())
        self.cli("resume", run)
        self.assertEqual((pair / "baseline/generation.json").read_bytes(), baseline)
        calls = self.recorded_calls()[before:]
        self.assertEqual(sum("--output-schema" not in call for call in calls), 1)
        self.assertEqual(sum("--output-schema" in call for call in calls), 2)
        before = len(self.recorded_calls())
        self.cli("resume", run)
        self.assertEqual(len(self.recorded_calls()), before)
        replay = self.root / "reference-replay"
        args[args.index("--out") + 1] = str(replay)
        self.cli(*args, *self.pins, "--replay", run)
        self.assertEqual(len(self.recorded_calls()) - before, 4)
        self.assertTrue(all("--output-schema" in call for call in self.recorded_calls()[before:]))
        result = json.loads((replay / "report.json").read_text())
        self.assertEqual(result["summary"]["paired_delta_mean"], 4)
        self.assertEqual(result["resources"]["groups"]["reused_author"]["calls"], 2)
        records = []
        for index, source in enumerate((run, replay)):
            record = self.root / f"record-{index}.json"
            self.cli("record", source, "--out", record, script="bundle.py")
            records.append(record)
        history = json.loads(self.cli("history", *records, script="bundle.py").stdout)
        self.assertEqual(len(history["groups"]), 1)
        self.assertEqual(len(next(iter(history["groups"].values()))["records"]), 2)

    def test_fresh_replay_accepts_prior_manifest_without_reference_fields(self):
        import paired
        self.run_experiment()
        manifest_path = self.out / "experiment.json"
        manifest = json.loads(manifest_path.read_text())
        manifest.pop("reference")
        manifest["protocol"].pop("kind")
        manifest["protocol_key"] = paired.canonical(manifest["protocol"])
        manifest_path.write_text(json.dumps(manifest))
        frozen_path = self.out / "inputs-fingerprint.json"
        frozen = json.loads(frozen_path.read_text())
        frozen["manifest_sha256"] = paired.digest(manifest_path)
        frozen_path.write_text(json.dumps(frozen))
        (self.out / "seal.json").unlink()
        self.cli("seal", self.out, script="bundle.py")
        args = list(self.run_args)
        args[args.index("--out") + 1] = str(self.root / "legacy-replay")
        before = len(self.recorded_calls())
        self.cli(*args, *self.pins, "--replay", self.out)
        self.assertEqual(len(self.recorded_calls()) - before, 4)
        self.assertTrue(all("--output-schema" in call for call in self.recorded_calls()[before:]))

    def test_reference_replacement_splits_history_and_rejects_replay(self):
        reference, args = self.reference_args()
        self.cli(*args, *self.pins)
        first = self.root / "reference-run"
        second_reference = self.root / "second-reference"
        self.cli("reference", self.out, "--arm", "candidate", "--repetition", "0",
                 "--source-commit", "b" * 40, "--out", second_reference)
        args[args.index("--reference") + 1] = str(second_reference)
        args[args.index("--out") + 1] = str(self.root / "invalid-replay")
        before = len(self.recorded_calls())
        self.cli(*args, *self.pins, "--replay", first, expected=2)
        self.assertEqual(len(self.recorded_calls()), before)
        second = self.root / "second-reference-run"
        args[args.index("--out") + 1] = str(second)
        self.cli(*args, *self.pins)
        records = []
        for index, source in enumerate((first, second)):
            record = self.root / f"changed-record-{index}.json"
            self.cli("record", source, "--out", record, script="bundle.py")
            records.append(record)
        history = json.loads(self.cli("history", *records, script="bundle.py").stdout)
        self.assertEqual(len(history["groups"]), 2)
        self.cli(first, second, "--out", self.root / "combined.json", script="combine_paired.py", expected=2)

    def test_reference_export_is_explicit_immutable_and_complete(self):
        args = list(self.run_args)
        args[args.index("--repetitions") + 1] = "2"
        self.cli(*args, *self.pins)
        first = self.root / "first-reference"
        self.cli("reference", self.out, "--arm", "candidate", "--repetition", "1",
                 "--source-commit", "c" * 40, "--out", first)
        value = json.loads((first / "reference.json").read_text())
        self.assertEqual(value["source"]["repetition"], 1)
        self.assertEqual([item["original_score"] for item in value["prompts"]], [16, 16])
        self.cli("verify-reference", first)
        self.cli("reference", self.out, "--arm", "baseline", "--repetition", "0",
                 "--source-commit", "c" * 40, "--out", first, expected=2)
        self.cli("reference", self.out, "--arm", "candidate", "--repetition", "2",
                 "--source-commit", "c" * 40, "--out", self.root / "invalid", expected=2)
        self.assertFalse((self.root / "invalid").exists())
        self.cli("reference", self.out, "--arm", "candidate", "--repetition", "1",
                 "--source-commit", "c" * 40, "--out", self.out / "nested/reference", expected=2)
        self.assertFalse((self.out / "nested").exists())
        self.cli("verify", self.out)

    def test_reference_export_rejects_failed_delivery_without_substitution(self):
        self.env["FAKE_INVALID_ART"] = "1"
        self.run_experiment()
        result = json.loads((self.out / "report.json").read_text())
        self.assertTrue(result["summary"]["complete"])
        self.assertEqual(result["summary"]["arms"]["candidate"]["failures"], 2)
        destination = self.root / "failed-reference"
        self.cli("reference", self.out, "--arm", "candidate", "--repetition", "0",
                 "--source-commit", "d" * 40, "--out", destination, expected=2)
        self.assertFalse(destination.exists())

    def test_committed_reference_is_valid(self):
        self.cli("verify-reference", BENCH / "references/quality-v1")


if __name__ == "__main__":
    unittest.main()
