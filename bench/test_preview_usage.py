import json
from pathlib import Path
import subprocess
import sys
import tempfile
import unittest

from PIL import Image


class PreviewUsageTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.bundle = Path(self.temp.name)
        self.author = self.bundle / "pairs/000-00/candidate"
        (self.author / "project").mkdir(parents=True)
        self.receipt = {"source": "art.ramo", "target": "art.png", "target_line": 1,
                        "exit_code": 0, "diagnostics": [], "backend": "cpu", "times": [0],
                        "size": [8, 8], "paths": ["/tmp/author/preview.png"],
                        "crop_artboard": [0, 0, 8, 8], "crop_pixels": [0, 0, 8, 8],
                        "parameters": [], "sampling": "even samples exclude loop end; explicit times include it"}

    def run_report(self, events, fallback=False):
        process = {"command": ["codex", "exec", "-C", "/tmp/author"],
                   "stdout": "\n".join(json.dumps(e) for e in events)}
        (self.author / "codex.json").write_text(json.dumps(process))
        if not fallback:
            (self.author / "events.json").write_text(json.dumps(events))
        result = subprocess.run([sys.executable, str(Path(__file__).with_name("preview_usage.py")), str(self.bundle)],
                                text=True, capture_output=True, check=True)
        return json.loads(result.stdout)["authors"][0]

    def event(self, command, output="", exit_code=0, event_type="item.completed", item_id="item_1"):
        return {"type": event_type, "item": {"id": item_id, "type": "command_execution",
                "command": command, "aggregated_output": output, "exit_code": exit_code}}

    def test_preview_receipt_and_retained_png_are_distinct_from_viewing(self):
        Image.new("RGBA", (8, 8)).save(self.author / "project/preview.png")
        command = "/bin/zsh -lc 'ramose preview art.ramo --out preview.png'"
        row = self.run_report([self.event(command, event_type="item.started"),
                               self.event(command, json.dumps(self.receipt, indent=2)),
                               {"type": "turn.completed"}])
        self.assertEqual((row["preview_command_events"], row["valid_receipts"], row["retained_pngs"]), (1, 1, 1))
        self.assertEqual(row["viewer_observation"], "unknown")
        self.assertTrue(row["turn_completed"])

    def test_documentation_and_view_image_shell_text_are_not_calls(self):
        row = self.run_report([self.event("cat .agents/skills/ramose/SKILL.md", "ramose preview art.ramo\n" + json.dumps(self.receipt)),
                               self.event("echo 'ramose preview art.ramo; view_image preview.png'", item_id="item_2"),
                               self.event("echo ';' ramose preview", item_id="item_3"),
                               {"type": "item.completed", "item": {"type": "agent_message", "text": "I ran ramose preview and view_image."}}])
        self.assertEqual((row["preview_command_events"], row["valid_receipts"], row["retained_pngs"]), (0, 0, 0))
        self.assertEqual(row["viewer_observation"], "unknown")

    def test_failed_attempt_has_no_receipt(self):
        row = self.run_report([self.event("ramose preview missing.ramo --out preview.png", "file not found", 1)], fallback=True)
        self.assertEqual((row["preview_command_events"], row["valid_receipts"]), (1, 0))
        self.assertEqual(row["calls"][0]["exit_code"], 1)
        self.assertFalse(row["turn_completed"])

    def test_diagnostic_exit_receipt_counts_with_missing_artifact(self):
        for exit_code in (2, 3):
            with self.subTest(exit_code=exit_code):
                self.receipt["exit_code"] = exit_code
                row = self.run_report([self.event("cd /tmp/author && /bin/ramose preview art.ramo --out preview.png", json.dumps(self.receipt), exit_code)])
                self.assertEqual((row["preview_command_events"], row["valid_receipts"], row["retained_pngs"]), (1, 1, 0))
                self.assertEqual(row["calls"][0]["receipts"][0]["exit_code"], exit_code)
                self.assertFalse(row["calls"][0]["receipts"][0]["artifacts"][0]["exists"])

    def test_npx_fallback_and_compound_invocations_count_one_command_event(self):
        row = self.run_report([self.event("npx --yes ramose preview art.ramo --out a.png && npx --yes ramose preview art.ramo --out b.png",
                                          json.dumps(self.receipt) + "\n" + json.dumps(self.receipt))])
        self.assertEqual((row["preview_command_events"], row["valid_receipts"]), (1, 2))

    def test_heredoc_examples_are_not_observed_commands(self):
        row = self.run_report([self.event("cat <<'EOF'\nExample; ramose preview art.ramo --out preview.png\nEOF",
                                          json.dumps(self.receipt))])
        self.assertEqual((row["preview_command_events"], row["valid_receipts"]), (0, 0))

    def test_newline_separates_executed_commands(self):
        row = self.run_report([self.event("echo done\nramose preview art.ramo --out preview.png",
                                          json.dumps(self.receipt))])
        self.assertEqual((row["preview_command_events"], row["valid_receipts"]), (1, 1))

    def test_partial_author_has_unknown_observation(self):
        row = self.run_report([])
        self.assertEqual(row["preview_command_events"], 0)
        self.assertFalse(row["events_available"])
        self.assertEqual(row["viewer_observation"], "unknown")

    def test_invalid_png_and_unrelated_json_do_not_verify_image(self):
        (self.author / "project/preview.png").write_text("not a PNG")
        row = self.run_report([self.event("ramose preview art.ramo --out preview.png", '{"paths": ["fake.png"]}\n' + json.dumps(self.receipt))])
        self.assertEqual((row["valid_receipts"], row["retained_pngs"]), (1, 0))
        artifact = row["calls"][0]["receipts"][0]["artifacts"][0]
        self.assertTrue(artifact["exists"])
        self.assertFalse(artifact["valid_png"])


if __name__ == "__main__":
    unittest.main()
