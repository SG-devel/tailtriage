import json
import os
import tempfile
import unittest
from pathlib import Path

from scripts import diagnostic_benchmark as db

BASE_CASE = {
    "id": "case-1",
    "artifact": "a.json",
    "artifact_type": "analysis_report",
    "ground_truth": "application_queue_saturation",
    "required_top2": ["application_queue_saturation"],
    "acceptable_primary": ["application_queue_saturation"],
    "tags": ["t"],
    "must_include_evidence": [],
    "must_include_next_checks": [],
    "expected_warnings": [],
    "allowed_warnings": [],
    "top1_required": False,
    "notes": "note",
}


def valid_report(kind="application_queue_saturation"):
    return {"primary_suspect": {"kind": kind, "confidence": "high", "score": 1.0, "evidence": ["ev"]}, "secondary_suspects": [], "warnings": []}


class DiagnosticBenchmarkTests(unittest.TestCase):
    def _write(self, root, rel, payload):
        p = Path(root) / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_text(json.dumps(payload), encoding="utf-8")
        return p

    def _manifest(self, case):
        return {"schema_version": 1, "cases": [case]}

    def test_manifest_validation_core_errors(self):
        with self.assertRaisesRegex(ValueError, "schema_version"):
            db.validate_manifest({"cases": [dict(BASE_CASE)]})
        bad = dict(BASE_CASE)
        bad["id"] = ""
        with self.assertRaisesRegex(ValueError, "id"):
            db.validate_manifest(self._manifest(bad))
        dup = [dict(BASE_CASE), dict(BASE_CASE)]
        with self.assertRaisesRegex(ValueError, "duplicate"):
            db.validate_manifest({"schema_version": 1, "cases": dup})

    def test_manifest_required_top2_and_acceptable_primary_rules(self):
        bad = dict(BASE_CASE); bad["ground_truth"] = "unknown"
        with self.assertRaisesRegex(ValueError, "ground_truth"):
            db.validate_manifest(self._manifest(bad))
        bad = dict(BASE_CASE); bad["required_top2"] = ["unknown"]
        with self.assertRaisesRegex(ValueError, "required_top2"):
            db.validate_manifest(self._manifest(bad))
        bad = dict(BASE_CASE); bad["required_top2"] = ["blocking_pool_pressure"]
        with self.assertRaisesRegex(ValueError, "required_top2.*ground_truth"):
            db.validate_manifest(self._manifest(bad))
        bad = dict(BASE_CASE); bad["acceptable_primary"] = ["unknown"]
        with self.assertRaisesRegex(ValueError, "acceptable_primary"):
            db.validate_manifest(self._manifest(bad))
        bad = dict(BASE_CASE); bad["acceptable_primary"] = ["blocking_pool_pressure"]
        with self.assertRaisesRegex(ValueError, "acceptable_primary.*ground_truth"):
            db.validate_manifest(self._manifest(bad))

    def test_report_validation_errors(self):
        with self.assertRaisesRegex(ValueError, "primary_suspect"):
            db.extract({"secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "kind"):
            db.extract({"primary_suspect": {"kind": "bad", "confidence": "high", "evidence": []}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "confidence"):
            db.extract({"primary_suspect": {"kind": "application_queue_saturation", "confidence": "bad", "evidence": []}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "evidence"):
            db.extract({"primary_suspect": {"kind": "application_queue_saturation", "confidence": "high", "evidence": "x"}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "secondary_suspects.kind"):
            db.extract({"primary_suspect": {"kind": "application_queue_saturation", "confidence": "high", "evidence": []}, "secondary_suspects": [{"kind": "bad"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "warnings"):
            db.extract({"primary_suspect": {"kind": "application_queue_saturation", "confidence": "high", "evidence": []}, "secondary_suspects": [], "warnings": [1]})

    def test_metric_semantics_split(self):
        with tempfile.TemporaryDirectory() as td:
            case = dict(BASE_CASE)
            case["acceptable_primary"] = ["application_queue_saturation", "blocking_pool_pressure"]
            self._write(td, "a.json", valid_report("blocking_pool_pressure"))
            m = self._write(td, "manifest.json", self._manifest(case))
            metrics, failures = db.run(str(m), 0, 0, 1)
            self.assertEqual(metrics["high_confidence_wrong_count"], 0)
            self.assertTrue(failures)

    def test_absolute_manifest_path_from_different_cwd(self):
        with tempfile.TemporaryDirectory() as td:
            self._write(td, "a.json", valid_report())
            m = self._write(td, "manifest.json", self._manifest(dict(BASE_CASE)))
            cwd = os.getcwd()
            os.chdir("/")
            try:
                metrics, failures = db.run(str(m.resolve()), 0, 0, 1)
            finally:
                os.chdir(cwd)
            self.assertEqual(metrics["total_cases"], 1)
            self.assertFalse(failures)


if __name__ == "__main__":
    unittest.main()
