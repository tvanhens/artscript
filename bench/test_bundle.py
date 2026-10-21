import copy
import json
from pathlib import Path
import shutil
import tempfile
import unittest
from unittest.mock import patch

import bundle


class BundleTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.base = Path(self.temp.name)
        self.root = self.base / "experiment"
        self.root.mkdir()
        protocol = {"model": "codex-test", "judge_model": "codex-test", "effort": "high",
                    "judge_effort": "high", "suite_sha256": "suite"}
        self.experiment = {"schema_version": 2, "mode": "strict", "protocol": protocol,
                           "protocol_key": bundle.fingerprint(protocol),
                           "provenance": {"baseline": {"SKILL.md": "old"},
                                          "candidate": {"SKILL.md": "new"},
                                          "binaries": {"baseline": "binary", "candidate": "binary"}}}
        self.write("experiment.json", self.experiment)
        self.write("pairs/000-00/pair.json", {"value": 5})

    def write(self, name, value):
        path = self.root / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(value))

    def result(self):
        return {"experiment": self.experiment, "protocol_errors": [],
                "summary": {"complete": True, "paired_delta_mean": 0.5}, "per_prompt": {}}

    def test_copy_is_portable_and_report_is_derived(self):
        original = bundle.seal(self.root)
        moved = self.base / "moved"
        shutil.copytree(self.root, moved)
        (moved / "report.json").write_text("regenerated")
        with bundle.lock(moved):
            self.assertEqual(bundle.verify(moved), original)

    def test_missing_changed_and_unexpected_receipts_fail(self):
        for mutation in ("missing", "changed", "unexpected"):
            with self.subTest(mutation=mutation):
                target = self.base / mutation
                shutil.copytree(self.root, target)
                bundle.seal(target)
                receipt = target / "pairs/000-00/pair.json"
                if mutation == "missing":
                    receipt.unlink()
                elif mutation == "changed":
                    receipt.write_text("changed")
                else:
                    (target / "extra.json").write_text("extra")
                with self.assertRaisesRegex(ValueError, "bundle integrity failed"):
                    bundle.verify(target)
                with self.assertRaises(ValueError):
                    bundle.seal(target)

    def test_nested_report_is_not_excluded(self):
        self.write("pairs/report.json", {})
        bundle.seal(self.root)
        self.write("pairs/report.json", {"changed": True})
        with self.assertRaisesRegex(ValueError, "changed"):
            bundle.verify(self.root)

    def test_symlinks_even_excluded_paths_are_rejected(self):
        bundle.seal(self.root)
        (self.root / "report.json").symlink_to(self.root / "experiment.json")
        with self.assertRaisesRegex(ValueError, "symlink"):
            bundle.verify(self.root)

    def test_inventory_cannot_escape_bundle(self):
        sealed = bundle.seal(self.root)
        sealed["files"]["../outside"] = {"sha256": "x", "size": 1}
        sealed["fingerprint"] = bundle.fingerprint({k: v for k, v in sealed.items() if k != "fingerprint"})
        self.write("seal.json", sealed)
        with self.assertRaisesRegex(ValueError, "invalid bundle inventory path"):
            bundle.verify(self.root)

    def test_protocol_mutation_prevents_sealing(self):
        self.experiment["protocol"]["model"] = "changed"
        self.write("experiment.json", self.experiment)
        with self.assertRaisesRegex(ValueError, "protocol fingerprint mismatch"):
            bundle.seal(self.root)

    def test_lock_excludes_concurrent_writer_and_can_be_reused(self):
        with bundle.lock(self.root):
            with self.assertRaisesRegex(ValueError, "locked"):
                with bundle.lock(self.root):
                    self.fail("second writer acquired lock")
        with bundle.lock(self.root):
            self.assertTrue((self.root / ".lock").exists())

    def test_history_is_immutable_and_tampering_is_rejected(self):
        bundle.seal(self.root)
        dest = self.base / "history/result.json"
        with patch("paired.report", return_value=self.result()):
            bundle.history_record(self.root, dest)
            with self.assertRaises(FileExistsError):
                bundle.history_record(self.root, dest)
        result = bundle.history([dest])
        self.assertEqual(next(iter(result["groups"].values()))["records"][0]["summary"]["paired_delta_mean"], 0.5)
        value = json.loads(dest.read_text())
        value["summary"]["paired_delta_mean"] = 19
        dest.write_text(json.dumps(value))
        with self.assertRaisesRegex(ValueError, "fingerprint mismatch"):
            bundle.history([dest])

    def test_history_retains_resources_and_orders_dates_with_legacy_fallback(self):
        paths = []
        for index, (field, date) in enumerate((("created_at_utc", "2026-10-22T01:00:00+00:00"),
                                               ("created_at", "2026-10-20T01:00:00+00:00"),
                                               ("created_at_utc", "2026-10-20T01:00:00+00:00"))):
            self.experiment.pop("created_at_utc", None)
            self.experiment.pop("created_at", None)
            self.experiment[field] = date
            self.experiment["provenance"]["candidate"] = {"SKILL.md": str(index)}
            self.write("experiment.json", self.experiment)
            (self.root / "seal.json").unlink(missing_ok=True)
            bundle.seal(self.root)
            result = self.result()
            result["resources"] = {"groups": {"judge": {"calls": index + 2}}, "cost_usd": None}
            dest = self.base / f"record-{index}.json"
            with patch("paired.report", return_value=result):
                record = bundle.history_record(self.root, dest)
            self.assertEqual(record["resources"]["groups"]["judge"]["calls"], index + 2)
            paths.append(dest)
        history = bundle.history([paths[0], paths[2], paths[1]])
        records = next(iter(history["groups"].values()))["records"]
        self.assertEqual([record["candidate"]["SKILL.md"] for record in records], ["2", "1", "0"])
        self.assertEqual([record["resources"]["groups"]["judge"]["calls"] for record in records], [4, 3, 2])
        self.assertEqual(records[1]["created_at_utc"], "2026-10-20T01:00:00+00:00")

    def test_history_refuses_exploratory_and_incomplete_results(self):
        bundle.seal(self.root)
        for change in ("mode", "complete"):
            result = copy.deepcopy(self.result())
            if change == "mode":
                result["experiment"]["mode"] = "exploratory"
            else:
                result["summary"]["complete"] = False
            with patch("paired.report", return_value=result):
                with self.assertRaises(ValueError):
                    bundle.history_record(self.root, self.base / f"{change}.json")

    def test_history_groups_reference_and_protocol_but_not_treatment(self):
        first = bundle.compatibility(self.experiment)
        other = copy.deepcopy(self.experiment)
        other["provenance"]["candidate"] = {"SKILL.md": "third"}
        other["provenance"]["binaries"]["candidate"] = "new binary"
        self.assertEqual(bundle.compatibility(other), first)
        other["provenance"]["binaries"]["baseline"] = "different reference"
        self.assertNotEqual(bundle.compatibility(other), first)
        other = copy.deepcopy(self.experiment)
        other["protocol"]["suite_sha256"] = "new suite"
        other["protocol_key"] = bundle.fingerprint(other["protocol"])
        self.assertNotEqual(bundle.compatibility(other), first)

    def test_history_output_must_not_modify_bundle(self):
        bundle.seal(self.root)
        with self.assertRaisesRegex(ValueError, "outside"):
            bundle.history_record(self.root, self.root / "history.json")
        bundle.verify(self.root)


if __name__ == "__main__":
    unittest.main()
