import json
import sys
import tempfile
import types
import unittest
from pathlib import Path
from unittest.mock import patch

import human_review as review


class HumanReviewTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)
        self.run = self.root / "run"
        self.run.mkdir()
        (self.run / "seal.json").write_text('{}')
        for name in ("baseline", "candidate"):
            (self.run / f"{name}.png").write_bytes(name.encode())
        self.row = {"prompt_id": "test", "prompt": '<script>alert("x")</script>', "repetition": 0,
                    "generations": {a: {"status": "success", "image": f"{a}.png"} for a in ("baseline", "candidate")},
                    "judgment": {"preferences": ["candidate", "candidate"]}}
        self.result = {"summary": {"complete": True}, "pairs": [self.row], "protocol_errors": []}
        self.packet = self.root / "packet"

    def export(self):
        with patch.dict(sys.modules, {"bundle": types.SimpleNamespace(verify=lambda root: {}),
                                      "paired": types.SimpleNamespace(report=lambda root: self.result)}):
            return review.export_packet(self.run, self.packet)

    def answer(self, value, reviewer="human-1"):
        path = self.root / 'answers.json'
        review.write(path, {"reviewer": reviewer, "annotations": value})
        return path

    def test_blind_packet_and_real_annotation(self):
        self.assertEqual(self.export()["calibration_status"], "absent")
        page = (self.packet / "public/index.html").read_text()
        self.assertIn('&lt;script&gt;', page)
        self.assertNotIn('<script>', page)
        public = review.read(self.packet / 'public/packet.json')
        self.assertNotIn('baseline', json.dumps(public))
        self.assertNotIn('candidate', json.dumps(public))
        mapping = review.read(self.packet / 'private.json')['items']['item-0001']
        side = next(s for s in ('left', 'right') if mapping[s] == 'candidate')
        self.assertEqual((self.packet / 'public' / public['items'][0][side]).read_bytes(), b'candidate')
        result = review.import_answers(self.packet, self.answer([{"id": "item-0001", "preference": side}]))
        self.assertEqual(result['status'], 'observed')
        self.assertEqual(result['agreement'], 1.0)
        self.assertEqual(result['annotated_pairs'], 1)
        self.assertEqual(result['observed_position_counts'][side], 1)

    def test_empty_template_is_not_calibration(self):
        self.export()
        result = review.import_answers(self.packet, self.packet / 'public/answers.json')
        self.assertEqual(result['status'], 'absent')
        self.assertIsNone(result['agreement'])
        self.assertEqual(result['annotated_pairs'], 0)

    def test_disagreeing_machine_orders_are_not_consensus(self):
        self.row['judgment']['preferences'] = ['baseline', 'candidate']
        self.export()
        result = review.import_answers(self.packet, self.answer([{"id": "item-0001", "preference": "tie"}]))
        self.assertEqual(result['compared_pairs'], 0)
        self.assertIsNone(result['agreement'])
        self.assertEqual(result['machine_orientation_disagreements'], 1)

    def test_import_preserves_reviewers_and_is_idempotent(self):
        self.export()
        first = review.import_answers(self.packet, self.answer([{"id": "item-0001", "preference": "left"}], reviewer='alice'))
        paths = list((self.packet / 'calibrations').glob('*.json'))
        self.assertEqual(len(paths), 1)
        original = paths[0].read_bytes()
        self.assertEqual(review.import_answers(self.packet, self.answer([{"id": "item-0001", "preference": "left"}], reviewer='alice')), first)
        self.assertEqual(len(list((self.packet / 'calibrations').glob('*.json'))), 1)
        second = review.import_answers(self.packet, self.answer([{"id": "item-0001", "preference": "right"}], reviewer='bob'))
        self.assertEqual(len(list((self.packet / 'calibrations').glob('*.json'))), 2)
        self.assertEqual(paths[0].read_bytes(), original)
        self.assertNotEqual(first['answers_sha256'], second['answers_sha256'])
        self.assertEqual(first['packet_sha256'], second['packet_sha256'])
        self.assertEqual(len(first['packet_sha256']), 64)

    def test_modified_calibration_receipt_is_not_overwritten(self):
        self.export()
        answers = self.answer([{"id": "item-0001", "preference": "left"}])
        review.import_answers(self.packet, answers)
        path = next((self.packet / 'calibrations').glob('*.json'))
        path.write_text('{}')
        with self.assertRaisesRegex(ValueError, 'refusing to overwrite'):
            review.import_answers(self.packet, answers)
        self.assertEqual(path.read_text(), '{}')

    def test_invalid_annotations_rejected(self):
        self.export()
        for values in ([{"id": "missing", "preference": "left"}],
                       [{"id": "item-0001", "preference": "yes"}],
                       [{"id": "item-0001", "preference": []}],
                       [{"id": "item-0001", "preference": "left"}] * 2):
            with self.subTest(values=values), self.assertRaises(ValueError):
                review.import_answers(self.packet, self.answer(values))
        with self.assertRaisesRegex(ValueError, 'reviewer'):
            review.import_answers(self.packet, self.answer([{"id": "item-0001", "preference": "left"}], reviewer=''))

    def test_image_tampering_rejected(self):
        self.export()
        (self.packet / 'public/images/item-0001-left.png').write_bytes(b'changed')
        with self.assertRaisesRegex(ValueError, 'changed'):
            review.import_answers(self.packet, self.answer([]))

    def test_escaping_source_rejected(self):
        self.row['generations']['baseline']['image'] = '../outside.png'
        with self.assertRaisesRegex(ValueError, 'escapes'):
            self.export()

    def test_incomplete_run_rejected(self):
        self.result['summary']['complete'] = False
        with self.assertRaisesRegex(ValueError, 'complete'):
            self.export()

    def test_bundle_verification_failure_stops_export(self):
        def failed(root):
            raise ValueError('seal mismatch')
        with patch.dict(sys.modules, {'bundle': types.SimpleNamespace(verify=failed),
                                      'paired': types.SimpleNamespace(report=lambda root: self.result)}):
            with self.assertRaisesRegex(ValueError, 'seal mismatch'):
                review.export_packet(self.run, self.packet)
        self.assertFalse(self.packet.exists())

    def test_suite_keeps_original_regressions_and_has_24_distinct_prompts(self):
        suite = review.read(Path(__file__).parent / 'suites/quality-v2.json')
        original = review.read(Path(__file__).parent / 'quality-prompts.json')
        self.assertEqual(suite['suite_version'], 'quality-v2')
        self.assertEqual(len(suite['prompts']), 24)
        prompts = {row['id']: row for row in suite['prompts']}
        self.assertEqual(len(prompts), 24)
        for key, text in original.items():
            self.assertEqual(prompts[key]['prompt'], text)
            self.assertIn('regression', prompts[key]['tags'])
        self.assertEqual(sum('previous-holdout' in row['tags'] for row in prompts.values()), 6)


if __name__ == '__main__':
    unittest.main()
