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
    subprocess.run(["ramose", "build"], cwd=cwd, check=True)
print(json.dumps({"type": "turn.completed", "usage": {"input_tokens": 20, "output_tokens": 10}}))
'''
FAKE_RENDERER = r'''
import sys
from pathlib import Path
from PIL import Image
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


if __name__ == "__main__":
    unittest.main()
