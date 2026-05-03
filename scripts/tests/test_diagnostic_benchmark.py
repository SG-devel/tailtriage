import json, tempfile, unittest
from pathlib import Path

from scripts import diagnostic_benchmark as db


def write_json(path, obj):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(obj), encoding="utf-8")


class DiagnosticBenchmarkTests(unittest.TestCase):
    def setUp(self):
        self.td = tempfile.TemporaryDirectory()
        self.root = Path(self.td.name)

    def tearDown(self):
        self.td.cleanup()

    def _report(self, kind="application_queue_saturation", conf="high", score=90, evidence=None, warnings=None, secondary=None):
        return {
            "primary_suspect": {"kind": kind, "confidence": conf, "score": score, "evidence": evidence or ["Queue wait"], "next_checks": []},
            "secondary_suspects": secondary or [],
            "warnings": warnings or [],
        }

    def _case(self, id_, artifact, gt="application_queue_saturation"):
        return {"id": id_, "artifact": artifact, "artifact_type": "analysis_report", "ground_truth": gt, "acceptable_top2": [gt], "tags": [], "must_include_evidence": ["Queue"], "allowed_warnings": [], "notes": "independent rationale"}

    def test_missing_required_manifest_field_fails(self):
        with self.assertRaises(ValueError):
            db.validate_manifest([{"id": "x"}])

    def test_duplicate_ids_fail(self):
        c = self._case("dup", "a.json")
        with self.assertRaises(ValueError):
            db.validate_manifest([c, dict(c)])

    def test_unknown_ground_truth_fails(self):
        c = self._case("a", "a.json")
        c["ground_truth"] = "unknown"
        with self.assertRaises(ValueError):
            db.validate_manifest([c])

    def test_top2_must_include_ground_truth(self):
        c = self._case("a", "a.json")
        c["acceptable_top2"] = ["downstream_stage_dominates"]
        with self.assertRaises(ValueError):
            db.validate_manifest([c])

    def test_metrics_and_output_shape(self):
        write_json(self.root / "r1.json", self._report())
        write_json(self.root / "r2.json", self._report(kind="downstream_stage_dominates", conf="low", evidence=["Stage 'db'"], secondary=[{"kind": "application_queue_saturation", "evidence": ["Queue wait"]}]))
        manifest = [self._case("c1", "r1.json"), self._case("c2", "r2.json")]
        write_json(self.root / "manifest.json", manifest)
        m = db.run(self.root / "manifest.json", 0.0, 0.0)
        self.assertIn("top1_accuracy", m)
        self.assertIn("top2_recall", m)
        self.assertIn("confidence_bucket_accuracy", m)
        self.assertEqual(m["total_cases"], 2)

    def test_allowed_warning_substrings_and_unexpected_fail(self):
        write_json(self.root / "r.json", self._report(warnings=["expected hint", "bad warning"]))
        c = self._case("c", "r.json")
        c["allowed_warnings"] = ["expected"]
        write_json(self.root / "manifest.json", [c])
        m = db.run(self.root / "manifest.json", 0.0, 0.0)
        self.assertEqual(m["unexpected_warning_count"], 1)

    def test_high_confidence_wrong_and_threshold_failure(self):
        write_json(self.root / "r.json", self._report(kind="blocking_pool_pressure", conf="high"))
        c = self._case("c", "r.json", gt="application_queue_saturation")
        c["acceptable_top2"] = ["application_queue_saturation"]
        c["must_include_evidence"] = ["Queue"]
        write_json(self.root / "manifest.json", [c])
        m = db.run(self.root / "manifest.json", 0.75, 0.9)
        self.assertEqual(m["high_confidence_wrong_count"], 1)
        self.assertEqual(m["top1_accuracy"], 0.0)


if __name__ == "__main__":
    unittest.main()
