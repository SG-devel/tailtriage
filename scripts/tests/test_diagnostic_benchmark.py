import json
import tempfile
import unittest
from pathlib import Path

from scripts import diagnostic_benchmark as db


class DiagnosticBenchmarkTests(unittest.TestCase):
    def _write(self, root: Path, rel: str, payload):
        p = root / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(payload), encoding="utf-8")
        return p

    def test_manifest_validation_errors(self):
        with self.assertRaises(ValueError):
            db.validate_manifest({"cases": [{"id": "x"}]})

    def test_duplicate_ids_and_unknown_ground_truth(self):
        base = {
            "artifact": "a.json", "artifact_type": "analysis_report", "acceptable_top2": ["application_queue_saturation"],
            "tags": [], "must_include_evidence": [], "expected_warnings": [], "allowed_warnings": [], "top1_required": False, "notes": "n"
        }
        with self.assertRaises(ValueError):
            db.validate_manifest({"cases": [{**base, "id": "a", "ground_truth": "application_queue_saturation"}, {**base, "id": "a", "ground_truth": "application_queue_saturation"}]})
        with self.assertRaises(ValueError):
            db.validate_manifest({"cases": [{**base, "id": "b", "ground_truth": "not_real"}]})

    def test_acceptable_top2_must_include_ground_truth(self):
        with self.assertRaises(ValueError):
            db.validate_manifest({"schema_version": 1, "cases": [{"id": "1", "artifact": "a.json", "artifact_type": "analysis_report", "ground_truth": "insufficient_evidence", "acceptable_top2": ["application_queue_saturation"], "tags": [], "must_include_evidence": [], "expected_warnings": [], "allowed_warnings": [], "top1_required": False, "notes": "x"}]})

    def test_schema_version_required(self):
        with self.assertRaises(ValueError):
            db.validate_manifest({"cases": []})

    def test_metrics_evidence_warnings_buckets_and_threshold_failure(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self._write(root, "ok.json", {"primary_suspect": {"kind": "application_queue_saturation", "confidence": "high", "score": 90, "evidence": ["Queue wait high"]}, "secondary_suspects": [{"kind": "blocking_pool_pressure", "evidence": ["blocking"]}], "warnings": []})
            self._write(root, "bad.json", {"primary_suspect": {"kind": "blocking_pool_pressure", "confidence": "high", "score": 85, "evidence": ["Blocking queue depth"]}, "secondary_suspects": [{"kind": "application_queue_saturation", "evidence": ["Queue wait"]}], "warnings": ["runtime signal missing"]})
            manifest = {"schema_version": 1, "cases": [
                {"id": "c1", "artifact": "ok.json", "artifact_type": "analysis_report", "ground_truth": "application_queue_saturation", "acceptable_top2": ["application_queue_saturation"], "tags": [], "must_include_evidence": ["Queue wait"], "expected_warnings": [], "allowed_warnings": [], "top1_required": True, "notes": "n"},
                {"id": "c2", "artifact": "bad.json", "artifact_type": "analysis_report", "ground_truth": "application_queue_saturation", "acceptable_top2": ["application_queue_saturation", "blocking_pool_pressure"], "tags": [], "must_include_evidence": ["Queue wait"], "expected_warnings": ["runtime signal missing"], "allowed_warnings": [], "top1_required": False, "notes": "n"}
            ]}
            mpath = self._write(root, "manifest.json", manifest)
            metrics, failures = db.run(str(mpath), 0.9, 1.0, 0)
            self.assertEqual(metrics["total_cases"], 2)
            self.assertAlmostEqual(metrics["top1_accuracy"], 0.5)
            self.assertAlmostEqual(metrics["top2_recall"], 1.0)
            self.assertEqual(metrics["high_confidence_wrong_count"], 0)
            self.assertIn("high", metrics["confidence_bucket_accuracy"])
            self.assertEqual(metrics["unexpected_warning_count"], 0)
            self.assertTrue(failures)

    def test_json_output_shape_stable(self):
        required = {"total_cases", "top1_accuracy", "top2_recall", "high_confidence_wrong_count", "per_ground_truth_counts", "confusion_matrix", "confidence_bucket_accuracy", "required_evidence_pass_rate", "unexpected_warning_count", "missing_expected_warning_count", "failed_cases"}
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self._write(root, "a.json", {"primary_suspect": {"kind": "insufficient_evidence", "confidence": "low", "score": 50, "evidence": ["Not enough"]}, "secondary_suspects": [], "warnings": []})
            mpath = self._write(root, "manifest.json", {"schema_version": 1, "cases": [{"id": "x", "artifact": "a.json", "artifact_type": "analysis_report", "ground_truth": "insufficient_evidence", "acceptable_top2": ["insufficient_evidence"], "tags": [], "must_include_evidence": ["Not enough"], "expected_warnings": [], "allowed_warnings": [], "top1_required": False, "notes": "n"}]})
            metrics, failures = db.run(str(mpath), 0.0, 0.0, 1)
            self.assertFalse(failures)
            self.assertTrue(required.issubset(set(metrics.keys())))

    def test_top2_miss_and_missing_expected_warning_fail_case(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self._write(root, "a.json", {"primary_suspect": {"kind": "blocking_pool_pressure", "confidence": "medium", "evidence": ["x"]}, "secondary_suspects": [{"kind": "executor_pressure_suspected", "evidence": []}], "warnings": []})
            manifest = {"schema_version": 1, "cases": [{"id": "x", "artifact": "a.json", "artifact_type": "analysis_report", "ground_truth": "application_queue_saturation", "acceptable_top2": ["application_queue_saturation"], "tags": [], "must_include_evidence": [], "expected_warnings": ["runtime signal missing"], "allowed_warnings": [], "top1_required": False, "notes": "n"}]}
            mpath = self._write(root, "manifest.json", manifest)
            _metrics, failures = db.run(str(mpath), 0.0, 0.0, 10)
            self.assertTrue(failures)

    def test_top1_required_and_high_confidence_wrong_gate(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self._write(root, "a.json", {"primary_suspect": {"kind": "blocking_pool_pressure", "confidence": "high", "evidence": ["x"]}, "secondary_suspects": [{"kind": "application_queue_saturation", "evidence": ["Queue wait"]}], "warnings": []})
            manifest = {"schema_version": 1, "cases": [{"id": "x", "artifact": "a.json", "artifact_type": "analysis_report", "ground_truth": "application_queue_saturation", "acceptable_top2": ["application_queue_saturation", "blocking_pool_pressure"], "tags": [], "must_include_evidence": [], "expected_warnings": [], "allowed_warnings": [], "top1_required": True, "notes": "n"}]}
            mpath = self._write(root, "manifest.json", manifest)
            _metrics, failures = db.run(str(mpath), 0.0, 0.0, 0)
            self.assertFalse(any("high_confidence_wrong_count" in f for f in failures))
            self.assertTrue(failures)

    def test_allowed_warnings_and_missing_expected_warning_count(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self._write(root, "a.json", {"primary_suspect": {"kind": "application_queue_saturation", "confidence": "low", "evidence": ["Queue wait"]}, "secondary_suspects": [], "warnings": ["allowed optional noise"]})
            manifest = {"schema_version": 1, "cases": [{"id": "x", "artifact": "a.json", "artifact_type": "analysis_report", "ground_truth": "application_queue_saturation", "acceptable_top2": ["application_queue_saturation"], "tags": [], "must_include_evidence": [], "expected_warnings": ["required runtime warning"], "allowed_warnings": ["allowed optional noise"], "top1_required": True, "notes": "n"}]}
            mpath = self._write(root, "manifest.json", manifest)
            metrics, failures = db.run(str(mpath), 0.0, 0.0, 99)
            self.assertEqual(metrics["unexpected_warning_count"], 0)
            self.assertEqual(metrics["missing_expected_warning_count"], 1)
            self.assertTrue(failures)

    def test_malformed_report_shape_fails_clearly(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            self._write(root, "bad.json", {"secondary_suspects": [], "warnings": []})
            manifest = {"schema_version": 1, "cases": [{"id": "x", "artifact": "bad.json", "artifact_type": "analysis_report", "ground_truth": "application_queue_saturation", "acceptable_top2": ["application_queue_saturation"], "tags": [], "must_include_evidence": [], "expected_warnings": [], "allowed_warnings": [], "top1_required": True, "notes": "n"}]}
            mpath = self._write(root, "manifest.json", manifest)
            with self.assertRaisesRegex(ValueError, "primary_suspect"):
                db.run(str(mpath), 0.0, 0.0, 0)


if __name__ == "__main__":
    unittest.main()
