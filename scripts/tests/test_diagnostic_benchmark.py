import copy
import json
import os
import tempfile
import unittest
from pathlib import Path

from scripts import diagnostic_benchmark as db

ALLOWED = sorted(db.ALLOWED_GROUND_TRUTH)

BASE_CASE = {
    "id": "case-1",
    "artifact": "case-1.json",
    "artifact_type": "analysis_report",
    "ground_truth": "application_queue_saturation",
    "required_top2": ["application_queue_saturation"],
    "acceptable_primary": ["application_queue_saturation"],
    "tags": ["queue"],
    "must_include_evidence": [],
    "must_include_next_checks": [],
    "expected_warnings": [],
    "allowed_warnings": [],
    "top1_required": False,
    "notes": "deterministic queue case",
}


def valid_report(*, primary_kind="application_queue_saturation", confidence="high", score=1.0, evidence=None, next_checks=None, secondary=None, warnings=None, confidence_notes=None, evidence_quality=None, route_breakdowns=None, temporal_segments=None):
    primary = {
        "kind": primary_kind,
        "confidence": confidence,
        "score": score,
        "evidence": evidence if evidence is not None else ["Queue wait dominates"],
    }
    if next_checks is not None:
        primary["next_checks"] = next_checks
    if confidence_notes is not None:
        primary["confidence_notes"] = confidence_notes
    return {
        "primary_suspect": primary,
        "secondary_suspects": secondary or [],
        "warnings": warnings or [],
        "evidence_quality": evidence_quality or {},
        "route_breakdowns": route_breakdowns or [],
        "temporal_segments": temporal_segments or [],
    }


class DiagnosticBenchmarkTests(unittest.TestCase):
    def make_case(self, **updates):
        case = copy.deepcopy(BASE_CASE)
        case.update(updates)
        return case

    def make_manifest(self, *cases, schema_version=1):
        return {"schema_version": schema_version, "cases": list(cases)}

    def write_json(self, root, relative_path, payload):
        path = Path(root) / relative_path
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(json.dumps(payload), encoding="utf-8")
        return path

    def run_single_case(self, case, report, *, min_top1=0.0, min_top2=0.0, max_high_confidence_wrong=99):
        with tempfile.TemporaryDirectory() as td:
            self.write_json(td, case["artifact"], report)
            manifest_path = self.write_json(td, "manifest.json", self.make_manifest(case))
            return db.run(str(manifest_path), min_top1, min_top2, max_high_confidence_wrong)

    # Manifest validation tests
    def test_manifest_schema_version_required(self):
        with self.assertRaisesRegex(ValueError, "schema_version"):
            db.validate_manifest(self.make_manifest(self.make_case(), schema_version=0))

    def test_committed_manifest_has_required_schema_version(self):
        manifest_path = Path(__file__).resolve().parents[2] / "validation" / "diagnostics" / "manifest.json"
        manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
        self.assertEqual(manifest.get("schema_version"), 1)
        db.validate_manifest(manifest)

    def test_manifest_duplicate_ids_fail(self):
        c1 = self.make_case(id="dup", artifact="a.json")
        c2 = self.make_case(id="dup", artifact="b.json")
        with self.assertRaisesRegex(ValueError, "duplicate case id"):
            db.validate_manifest(self.make_manifest(c1, c2))

    def test_manifest_non_empty_id_and_artifact_required(self):
        with self.assertRaisesRegex(ValueError, "id"):
            db.validate_manifest(self.make_manifest(self.make_case(id="")))
        with self.assertRaisesRegex(ValueError, "artifact"):
            db.validate_manifest(self.make_manifest(self.make_case(artifact="")))

    def test_manifest_artifact_type_must_be_allowed(self):
        bad = self.make_case(artifact_type="anything_else")
        with self.assertRaisesRegex(ValueError, "artifact_type"):
            db.validate_manifest(self.make_manifest(bad))

    def test_manifest_ground_truth_and_required_top2_rules(self):
        with self.assertRaisesRegex(ValueError, "unknown ground_truth"):
            db.validate_manifest(self.make_manifest(self.make_case(ground_truth="not_a_kind")))
        with self.assertRaisesRegex(ValueError, "required_top2 must be a non-empty list"):
            db.validate_manifest(self.make_manifest(self.make_case(required_top2=[])))
        with self.assertRaisesRegex(ValueError, "required_top2 contains unknown diagnosis kind"):
            db.validate_manifest(self.make_manifest(self.make_case(required_top2=["unknown"])))
        with self.assertRaisesRegex(ValueError, "required_top2 must include ground_truth"):
            db.validate_manifest(self.make_manifest(self.make_case(required_top2=["blocking_pool_pressure"])))

    def test_manifest_acceptable_primary_rules(self):
        with self.assertRaisesRegex(ValueError, "acceptable_primary must be a non-empty list"):
            db.validate_manifest(self.make_manifest(self.make_case(acceptable_primary=[])))
        with self.assertRaisesRegex(ValueError, "acceptable_primary contains unknown diagnosis kind"):
            db.validate_manifest(self.make_manifest(self.make_case(acceptable_primary=["bad_kind"])))
        with self.assertRaisesRegex(ValueError, "acceptable_primary must include ground_truth"):
            db.validate_manifest(self.make_manifest(self.make_case(acceptable_primary=["blocking_pool_pressure"])))

    def test_manifest_list_and_scalar_field_shapes(self):
        with self.assertRaisesRegex(ValueError, "tags"):
            db.validate_manifest(self.make_manifest(self.make_case(tags=[""])))
        with self.assertRaisesRegex(ValueError, "must_include_evidence"):
            db.validate_manifest(self.make_manifest(self.make_case(must_include_evidence=[1])))
        with self.assertRaisesRegex(ValueError, "must_include_next_checks"):
            db.validate_manifest(self.make_manifest(self.make_case(must_include_next_checks=[1])))
        with self.assertRaisesRegex(ValueError, "expected_warnings"):
            db.validate_manifest(self.make_manifest(self.make_case(expected_warnings=[1])))
        with self.assertRaisesRegex(ValueError, "allowed_warnings"):
            db.validate_manifest(self.make_manifest(self.make_case(allowed_warnings=[1])))

    def test_manifest_wildcard_warnings_and_remaining_fields(self):
        with self.assertRaisesRegex(ValueError, "wildcard"):
            db.validate_manifest(self.make_manifest(self.make_case(expected_warnings=["*"])))
        with self.assertRaisesRegex(ValueError, "wildcard"):
            db.validate_manifest(self.make_manifest(self.make_case(allowed_warnings=["*"])))
        with self.assertRaisesRegex(ValueError, "top1_required"):
            db.validate_manifest(self.make_manifest(self.make_case(top1_required="yes")))
        with self.assertRaisesRegex(ValueError, "notes"):
            db.validate_manifest(self.make_manifest(self.make_case(notes="")))
    def test_manifest_max_primary_confidence_rules(self):
        db.validate_manifest(self.make_manifest(self.make_case()))
        for allowed in ["low", "medium", "high"]:
            db.validate_manifest(self.make_manifest(self.make_case(max_primary_confidence=allowed)))
        with self.assertRaisesRegex(ValueError, "max_primary_confidence must be one of"):
            db.validate_manifest(self.make_manifest(self.make_case(max_primary_confidence="very_high")))
        with self.assertRaisesRegex(ValueError, "max_primary_confidence must be one of"):
            db.validate_manifest(self.make_manifest(self.make_case(max_primary_confidence="extreme")))
        with self.assertRaisesRegex(ValueError, "max_primary_confidence must be a string"):
            db.validate_manifest(self.make_manifest(self.make_case(max_primary_confidence=1)))

    # Report validation tests
    def test_report_missing_primary_fails(self):
        with self.assertRaisesRegex(ValueError, "primary_suspect"):
            db.extract({"secondary_suspects": [], "warnings": []})

    def test_report_primary_field_validation(self):
        with self.assertRaisesRegex(ValueError, "primary_suspect.kind"):
            db.extract({"primary_suspect": {"kind": "bad", "confidence": "high", "evidence": []}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "confidence"):
            db.extract({"primary_suspect": {"kind": ALLOWED[0], "confidence": "bad", "evidence": []}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "confidence"):
            db.extract({"primary_suspect": {"kind": ALLOWED[0], "confidence": "very_high", "evidence": []}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "score"):
            db.extract({"primary_suspect": {"kind": ALLOWED[0], "confidence": "high", "score": "x", "evidence": []}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "evidence"):
            db.extract({"primary_suspect": {"kind": ALLOWED[0], "confidence": "high", "evidence": "x"}, "secondary_suspects": [], "warnings": []})
        with self.assertRaisesRegex(ValueError, "next_checks"):
            db.extract({"primary_suspect": {"kind": ALLOWED[0], "confidence": "high", "evidence": [], "next_checks": "x"}, "secondary_suspects": [], "warnings": []})

    def test_report_secondary_and_warnings_validation(self):
        primary = {"kind": ALLOWED[0], "confidence": "high", "evidence": []}
        with self.assertRaisesRegex(ValueError, "secondary_suspects.kind"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [{"kind": "bad"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "secondary_suspects.confidence"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [{"confidence": "bad"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "secondary_suspects.confidence"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [{"confidence": "very_high"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "secondary_suspects.score"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [{"score": "bad"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "secondary_suspects.evidence"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [{"evidence": "bad"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "secondary_suspects.next_checks"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [{"next_checks": "bad"}], "warnings": []})
        with self.assertRaisesRegex(ValueError, "warnings"):
            db.extract({"primary_suspect": primary, "secondary_suspects": [], "warnings": [1]})

    def test_analysis_report_requires_primary_score_but_synthetic_may_omit(self):
        case = self.make_case(artifact_type="analysis_report")
        no_score_report = valid_report(score=None)
        del no_score_report["primary_suspect"]["score"]
        with self.assertRaisesRegex(ValueError, "analysis_report requires"):
            self.run_single_case(case, no_score_report)

        synthetic = self.make_case(artifact_type="synthetic_analysis_report")
        metrics, failures = self.run_single_case(synthetic, no_score_report)
        self.assertEqual(metrics["total_cases"], 1)
        self.assertFalse(failures)

    # Metric semantics tests
    def test_top1_required_wrong_primary_fails(self):
        case = self.make_case(top1_required=True)
        metrics, failures = self.run_single_case(case, valid_report(primary_kind="blocking_pool_pressure"))
        self.assertTrue(failures)
        self.assertEqual(len(metrics["failed_cases"]), 1)
        self.assertFalse(metrics["failed_cases"][0]["top1_ok"])

    def test_required_top2_missing_fails_even_if_primary_acceptable(self):
        case = self.make_case(
            required_top2=["application_queue_saturation"],
            acceptable_primary=["application_queue_saturation", "blocking_pool_pressure"],
        )
        metrics, failures = self.run_single_case(case, valid_report(primary_kind="blocking_pool_pressure"))
        self.assertTrue(failures)
        self.assertFalse(metrics["failed_cases"][0]["top2_ok"])

    def test_acceptable_alternate_primary_with_ground_truth_secondary(self):
        case = self.make_case(acceptable_primary=["application_queue_saturation", "blocking_pool_pressure"])
        secondary = [{"kind": "application_queue_saturation", "evidence": ["backup"]}]
        metrics, failures = self.run_single_case(case, valid_report(primary_kind="blocking_pool_pressure", secondary=secondary))
        self.assertFalse(failures)
        self.assertEqual(metrics["high_confidence_wrong_count"], 0)

    def test_high_confidence_unacceptable_primary_counts_even_with_gt_secondary(self):
        case = self.make_case(acceptable_primary=["application_queue_saturation"])
        secondary = [{"kind": "application_queue_saturation", "evidence": ["backup"]}]
        metrics, failures = self.run_single_case(case, valid_report(primary_kind="blocking_pool_pressure", secondary=secondary), max_high_confidence_wrong=0)
        self.assertEqual(metrics["high_confidence_wrong_count"], 1)
        self.assertTrue(failures)

    def test_warning_and_evidence_semantics(self):
        case = self.make_case(must_include_evidence=["secondary evidence"], expected_warnings=["expected warn"], allowed_warnings=["optional warn"])
        secondary = [{"kind": "blocking_pool_pressure", "evidence": ["secondary evidence"]}]
        good_report = valid_report(secondary=secondary, warnings=["expected warn", "optional warn"])
        metrics, failures = self.run_single_case(case, good_report)
        self.assertFalse(failures)
        self.assertEqual(metrics["unexpected_warning_count"], 0)

        missing_expected = valid_report(secondary=secondary, warnings=[])
        metrics, failures = self.run_single_case(case, missing_expected)
        self.assertEqual(metrics["missing_expected_warning_count"], 1)
        self.assertTrue(failures)

        unexpected_warning = valid_report(secondary=secondary, warnings=["expected warn", "not allowed"])
        metrics, failures = self.run_single_case(case, unexpected_warning)
        self.assertEqual(metrics["unexpected_warning_count"], 1)
        self.assertTrue(failures)

    def test_next_check_metrics_and_required_substrings(self):
        case_no_requirements = self.make_case()
        report_with_next_checks = valid_report(next_checks=["inspect queue depth"], secondary=[{"kind": "blocking_pool_pressure", "next_checks": ["check blocking pool"]}])
        metrics, failures = self.run_single_case(case_no_requirements, report_with_next_checks)
        self.assertFalse(failures)
        self.assertIsNone(metrics["next_check_pass_rate"])
        self.assertEqual(metrics["next_check_presence_rate"], 1.0)

        required_case = self.make_case(must_include_next_checks=["queue depth"])
        metrics, failures = self.run_single_case(required_case, report_with_next_checks)
        self.assertFalse(failures)
        self.assertEqual(metrics["next_check_required_cases"], 1)
        self.assertEqual(metrics["next_check_passed_cases"], 1)

        missing_report = valid_report(next_checks=["check runtime metrics"])
        metrics, failures = self.run_single_case(required_case, missing_report)
        self.assertTrue(failures)
        self.assertEqual(metrics["next_check_passed_cases"], 0)
        self.assertFalse(metrics["failed_cases"][0]["next_check_ok"])

    def test_failed_cases_include_useful_fields(self):
        case = self.make_case(top1_required=True, expected_warnings=["must appear"])
        metrics, _ = self.run_single_case(case, valid_report(primary_kind="blocking_pool_pressure", warnings=[]))
        self.assertEqual(len(metrics["failed_cases"]), 1)
        row = metrics["failed_cases"][0]
        for field in ["id", "top1_ok", "top2_ok", "evidence_ok", "next_check_ok", "confidence_ceiling_ok", "max_primary_confidence", "primary_confidence", "unexpected_warnings", "missing_expected_warnings", "top1_required"]:
            self.assertIn(field, row)
    def test_confidence_ceiling_semantics_and_metrics(self):
        base_case = self.make_case(max_primary_confidence="medium")
        metrics, failures = self.run_single_case(base_case, valid_report(confidence="medium"))
        self.assertFalse(failures)
        self.assertEqual(metrics["confidence_ceiling_cases"], 1)
        self.assertEqual(metrics["confidence_ceiling_passed_cases"], 1)
        self.assertEqual(metrics["confidence_ceiling_pass_rate"], 1.0)

        metrics, failures = self.run_single_case(base_case, valid_report(confidence="low"))
        self.assertFalse(failures)
        self.assertEqual(metrics["confidence_ceiling_passed_cases"], 1)

        metrics, failures = self.run_single_case(base_case, valid_report(confidence="high"))
        self.assertTrue(failures)
        self.assertEqual(metrics["confidence_ceiling_passed_cases"], 0)
        row = metrics["failed_cases"][0]
        self.assertFalse(row["confidence_ceiling_ok"])
        self.assertEqual(row["max_primary_confidence"], "medium")
        self.assertEqual(row["primary_confidence"], "high")


    def test_optional_manifest_field_validation(self):
        with self.assertRaisesRegex(ValueError, "expected_evidence_quality"):
            db.validate_manifest(self.make_manifest(self.make_case(expected_evidence_quality="bad")))
        with self.assertRaisesRegex(ValueError, "unknown signal family"):
            db.validate_manifest(self.make_manifest(self.make_case(expected_signal_statuses={"bad":"present"})))
        with self.assertRaisesRegex(ValueError, "unknown signal status"):
            db.validate_manifest(self.make_manifest(self.make_case(expected_signal_statuses={"queues":"bad"})))
        with self.assertRaisesRegex(ValueError, "must_include_confidence_notes"):
            db.validate_manifest(self.make_manifest(self.make_case(must_include_confidence_notes="x")))

    def test_optional_checks_pass_and_fail(self):
        case = self.make_case(expected_evidence_quality="strong", expected_signal_statuses={"queues":"present"}, must_include_confidence_notes=["queue"], expected_route_breakdowns="non_empty", expected_temporal_segments="non_empty", must_include_route_warning=["route caveat"], must_include_temporal_warning=["overlap"], expected_top_level_warnings=["top warning"], allowed_warnings=["top warning"])
        report = valid_report(confidence_notes=["Queue confidence note"], warnings=["top warning"], evidence_quality={"quality":"strong","queues":"present"}, route_breakdowns=[{"warnings":["route caveat"]}], temporal_segments=[{"warnings":["overlap"]}])
        metrics, failures = self.run_single_case(case, report)
        self.assertFalse(failures)
        self.assertEqual(metrics["evidence_quality_check_passed_cases"], 1)

        bad = valid_report(confidence_notes=["other"], warnings=[], evidence_quality={"quality":"weak","queues":"missing"}, route_breakdowns=[], temporal_segments=[])
        metrics, failures = self.run_single_case(case, bad)
        self.assertTrue(failures)
        row = metrics["failed_cases"][0]
        self.assertFalse(row["evidence_quality_ok"])
        self.assertFalse(row["signal_status_ok"])
        self.assertFalse(row["confidence_note_ok"])
        self.assertFalse(row["route_breakdown_ok"])
        self.assertFalse(row["temporal_segment_ok"])
        self.assertFalse(row["route_warning_ok"])
        self.assertFalse(row["temporal_warning_ok"])
        self.assertEqual(row["missing_expected_top_level_warnings"], ["top warning"])

    def test_existing_cases_without_optional_fields_still_pass(self):
        case = self.make_case()
        metrics, failures = self.run_single_case(case, valid_report())
        self.assertFalse(failures)
        self.assertEqual(metrics["evidence_quality_check_cases"], 0)
    # Threshold and output/path tests
    def test_threshold_failures(self):
        case = self.make_case()
        report = valid_report(primary_kind="blocking_pool_pressure")
        _, failures = self.run_single_case(case, report, min_top1=1.0)
        self.assertTrue(any("top1_accuracy" in f for f in failures))
        _, failures = self.run_single_case(case, report, min_top2=1.0)
        self.assertTrue(any("top2_recall" in f for f in failures))
        _, failures = self.run_single_case(case, report, max_high_confidence_wrong=0)
        self.assertTrue(any("high_confidence_wrong_count" in f for f in failures))

    def test_metrics_shape_and_absolute_manifest_path(self):
        with tempfile.TemporaryDirectory() as td:
            case = self.make_case()
            self.write_json(td, case["artifact"], valid_report())
            manifest = self.write_json(td, "manifest.json", self.make_manifest(case))
            cwd = os.getcwd()
            os.chdir("/")
            try:
                metrics, failures = db.run(str(manifest.resolve()), 0.0, 0.0, 99)
            finally:
                os.chdir(cwd)

            self.assertFalse(failures)
            expected_keys = {
                "total_cases", "top1_accuracy", "top2_recall", "high_confidence_wrong_count",
                "per_ground_truth_counts", "confusion_matrix", "confidence_bucket_accuracy",
                "required_evidence_pass_rate", "next_check_required_cases", "next_check_passed_cases",
                "next_check_presence_rate", "next_check_pass_rate", "confidence_ceiling_cases",
                "confidence_ceiling_passed_cases", "confidence_ceiling_pass_rate", "unexpected_warning_count",
                "missing_expected_warning_count", "evidence_quality_check_cases", "evidence_quality_check_passed_cases",
                "signal_status_check_cases", "signal_status_check_passed_cases", "confidence_note_check_cases",
                "confidence_note_check_passed_cases", "route_breakdown_check_cases", "route_breakdown_check_passed_cases",
                "temporal_segment_check_cases", "temporal_segment_check_passed_cases", "failed_cases",
            }
            self.assertEqual(set(metrics.keys()), expected_keys)


if __name__ == "__main__":
    unittest.main()
