import unittest
import hashlib
import json
import tempfile
from pathlib import Path
from unittest.mock import patch
from PIL import Image
from paired import generation, AXES, RUBRIC, Experiment, evaluate_pair, summarize, validate_score, judge, report, save


def row(base, candidate, base_status="success", candidate_status="success", judgment="complete"):
    return {"generations": {"baseline": {"status": base_status}, "candidate": {"status": candidate_status}},
            "judgment": {"status": judgment, "scores": {"baseline": base, "candidate": candidate},
                         "preferences": ["candidate" if candidate is not None and base is not None and candidate > base else "tie"],
                         "orientation_disagreement": False}}


class PairedReportTests(unittest.TestCase):
    def test_failed_generation_stays_in_denominator(self):
        result = summarize([row(12, 16), row(0, 8, base_status="failed")], 2)
        self.assertTrue(result["complete"])
        self.assertEqual(result["arms"]["baseline"]["mean"], 6)
        self.assertEqual(result["arms"]["baseline"]["failures"], 1)
        self.assertEqual(result["arms"]["candidate"]["mean"], 12)
        self.assertEqual(result["paired_deltas"], [4, 8])
        self.assertEqual(result["paired_delta_mean"], 6)
        self.assertEqual(result["paired_delta_ci95"], [4, 8])

    def test_missing_judge_does_not_become_zero_or_disappear(self):
        result = summarize([row(12, 16), row(None, None, judgment="error")], 2)
        self.assertFalse(result["complete"])
        self.assertEqual(result["judge_errors"], 1)
        self.assertIsNone(result["paired_delta_mean"])
        self.assertNotIn("mean", result["arms"]["baseline"])

    def test_unscheduled_missing_receipt_is_incomplete(self):
        result = summarize([row(12, 16)], 2)
        self.assertFalse(result["complete"])
        self.assertEqual(result["scheduled_pairs"], 2)
        self.assertEqual(result["received_pairs"], 1)

    def test_aggregation_literal_values(self):
        result = summarize([row(4, 8), row(12, 16), row(8, 12)], 3)
        self.assertEqual(result["arms"]["baseline"]["mean"], 8)
        self.assertEqual(result["arms"]["candidate"]["median"], 12)
        self.assertEqual(result["arms"]["candidate"]["range"], [8, 16])
        self.assertEqual(result["paired_delta_ci95"], [4, 4])
        self.assertEqual(result["preference_votes"], {"baseline": 0, "candidate": 3, "tie": 0})

    def test_failed_arm_gets_zero_even_if_it_left_an_image(self):
        exp = Experiment((), 1, None, "judge", "ramose", "base", "candidate")
        generations = {"baseline": {"status": "failed", "image": "broken.png"},
                       "candidate": {"status": "success", "image": "good.png"}}
        with patch("paired.judge", return_value=dict.fromkeys(AXES, 12) | {"notes": "Good."}):
            result = evaluate_pair(exp, "fox", generations, Path("unused"))
        self.assertEqual(result["scores"], {"baseline": 0, "candidate": 12})
        self.assertEqual(result["preferences"], ["candidate"])

    def test_reversal_maps_scores_back_to_arm(self):
        exp = Experiment((), 1, None, "judge", "ramose", "base", "candidate")
        generations = {arm: {"status": "success", "image": arm + ".png"}
                       for arm in ("baseline", "candidate")}
        low = dict.fromkeys(AXES, 8) | {"notes": "Flat."}
        high = dict.fromkeys(AXES, 16) | {"notes": "Polished."}
        verdicts = [{"left": low, "right": high, "preference": "right"},
                    {"left": high, "right": low, "preference": "left"}]
        with patch("paired.judge", side_effect=verdicts):
            result = evaluate_pair(exp, "fox", generations, Path("unused"))
        self.assertEqual(result["scores"], {"baseline": 8, "candidate": 16})
        self.assertEqual(result["preferences"], ["candidate", "candidate"])
        self.assertFalse(result["orientation_disagreement"])

    def test_reversal_judge_failure_invalidates_pair(self):
        exp = Experiment((), 1, None, "judge", "ramose", "base", "candidate")
        generations = {arm: {"status": "success", "image": arm + ".png"}
                       for arm in ("baseline", "candidate")}
        score = dict.fromkeys(AXES, 12) | {"notes": "Good."}
        with patch("paired.judge", side_effect=[{"left": score, "right": score, "preference": "tie"}, ValueError("timeout")]):
            result = evaluate_pair(exp, "fox", generations, Path("unused"))
        self.assertEqual(result["status"], "error")
        self.assertIsNone(result["scores"])

    def test_judge_retry_keeps_distinct_receipts(self):
        score = dict.fromkeys(AXES, 12) | {"notes": "Good."}
        with patch("paired.judge_once", side_effect=[ValueError("schema"), score]) as call:
            result = judge("fox", [Path("image.png")], "judge", Path("judge.json"))
        self.assertEqual(result, score)
        self.assertEqual([c.args[3].name for c in call.call_args_list],
                         ["judge-attempt1.json", "judge-attempt2.json"])

    def test_changed_protocol_makes_report_incomplete(self):
        with tempfile.TemporaryDirectory() as tmp:
            out = Path(tmp)
            save(out / "experiment.json", {"prompts": [["fox", "draw fox"]], "repetitions": 1,
                 "schedule": [{"prompt_id": "fox", "repetition": 0}], "harness_sha256": "changed",
                 "rubric_sha256": hashlib.sha256(RUBRIC.encode()).hexdigest()})
            save(out / "pairs/000-00/pair.json", row(12, 16) | {"prompt_id": "fox", "repetition": 0})
            result = report(out)
        self.assertFalse(result["summary"]["complete"])
        self.assertIsNone(result["summary"]["paired_delta_mean"])
        self.assertEqual(result["protocol_errors"], ["harness changed since experiment was frozen"])
        self.assertEqual(result["per_prompt"]["fox"]["arms"]["candidate"]["mean"], 16)

    def test_clean_rebuild_cannot_reuse_delivered_png(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp)
            skill = root / "skill"
            skill.mkdir()
            (skill / "SKILL.md").write_text("Frozen skill contents")
            exp = Experiment((), 1, None, "judge", "ramose", str(skill), str(skill))
            workdirs = []

            def directory(prefix):
                path = root / f"{prefix}{len(workdirs)}"
                path.mkdir()
                workdirs.append(path)
                return str(path)

            def run(args, cwd, receipt, env=None):
                events = []
                if args[0] == "codex":
                    (cwd / "ramose.toml").write_text('[project]\nout = "assets"\n')
                    (cwd / "assets").mkdir()
                    Image.new("RGBA", (4, 4), "red").save(cwd / "assets/art.png")
                    events = [{"type": "item.completed", "item": {"type": "command_execution",
                               "exit_code": 0, "command": "cat .agents/skills/ramose/SKILL.md",
                               "aggregated_output": "Frozen skill contents"}}, {"type": "turn.completed"}]
                else:
                    self.assertNotEqual(cwd, workdirs[0])
                    self.assertFalse((cwd / "assets/art.png").exists())
                return {"returncode": 0, "timeout": False, "stdout": "\n".join(map(json.dumps, events))}

            with patch("paired.tempfile.mkdtemp", side_effect=directory), patch("paired.command", side_effect=run):
                result = generation(exp, "baseline", "fox", root / "result")
            self.assertEqual(result["status"], "failed")
            self.assertIn("expected exactly one output PNG, found 0", result["error"])
            self.assertTrue((root / "result/project/assets/art.png").exists())
            self.assertTrue((workdirs[0] / "assets/art.png").exists())
            self.assertFalse((root / "result/validation/assets/art.png").exists())

    def test_rebuild_accepts_project_under_symlinked_parent(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            real = root / "real"
            real.mkdir()
            alias = root / "alias"
            alias.symlink_to(real, target_is_directory=True)
            skill = root / "skill"
            skill.mkdir()
            (skill / "SKILL.md").write_text("Frozen skill contents")
            exp = Experiment((), 1, None, "judge", "ramose", str(skill), str(skill))
            allocated = []

            def directory(prefix):
                path = alias / f"{prefix}{len(allocated)}"
                path.mkdir()
                allocated.append(path)
                return str(path)

            def run(args, cwd, receipt, env=None):
                events = []
                if args[0] == "codex":
                    (cwd / "ramose.toml").write_text('[project]\nout = "assets"\n')
                    events = [{"type": "item.completed", "item": {"type": "command_execution",
                               "exit_code": 0, "command": "cat .agents/skills/ramose/SKILL.md",
                               "aggregated_output": "Frozen skill contents"}}, {"type": "turn.completed"}]
                if args[0] == "codex" or args[1] == "build":
                    (cwd / "assets").mkdir(exist_ok=True)
                    Image.new("RGBA", (4, 4), "red").save(cwd / "assets/art.png")
                return {"returncode": 0, "timeout": False, "stdout": "\n".join(map(json.dumps, events))}

            with patch("paired.tempfile.mkdtemp", side_effect=directory), patch("paired.command", side_effect=run):
                result = generation(exp, "baseline", "fox", root / "result")
            self.assertEqual(result["status"], "success", result["error"])
            self.assertEqual((root / "result/original.png").read_bytes(), (root / "result/rebuilt.png").read_bytes())

    def test_explicit_skill_delivery_needs_no_cat_event(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = Path(tmp).resolve()
            skill = root / "skill"
            skill.mkdir()
            (skill / "SKILL.md").write_text("Frozen skill contents")
            project, validation = root / "studio", root / "validation"
            project.mkdir()
            validation.mkdir()
            exp = Experiment((), 1, None, "judge", "ramose", str(skill), str(skill))

            def run(args, cwd, receipt, env=None):
                if args[0] == "codex":
                    self.assertEqual(args[-1], f"[$ramose]({project}/.agents/skills/ramose/SKILL.md) fox")
                    (cwd / "ramose.toml").write_text('[project]\nout = "assets"\n')
                if args[0] == "codex" or args[1] == "build":
                    (cwd / "assets").mkdir(exist_ok=True)
                    Image.new("RGBA", (4, 4), "red").save(cwd / "assets/art.png")
                return {"returncode": 0, "timeout": False, "stdout": '{"type":"turn.completed"}'}

            with patch("paired.tempfile.mkdtemp", side_effect=[str(project), str(validation)]), patch("paired.command", side_effect=run):
                generated = generation(exp, "baseline", "fox", root / "result")
            self.assertEqual(generated["status"], "success", generated["error"])
            self.assertFalse(generated["skill_read_observed"])
            self.assertIsNone(generated["error"])

    def test_strict_score_validation(self):
        valid = dict.fromkeys(AXES, 12) | {"notes": "Readable. Flat lighting."}
        self.assertEqual(validate_score(valid), valid)
        for invalid in (True, False, 0, 21, 12.0, "12", None):
            with self.subTest(invalid=invalid), self.assertRaises(ValueError):
                validate_score(valid | {"wow": invalid})
        with self.assertRaises(ValueError):
            validate_score({"fidelity": 12})
        with self.assertRaises(ValueError):
            validate_score(dict.fromkeys(AXES, 12))


if __name__ == "__main__":
    unittest.main()
