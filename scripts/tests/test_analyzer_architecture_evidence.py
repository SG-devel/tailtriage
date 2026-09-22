import json
import os
import shutil
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from scripts import analyzer_architecture_evidence as evidence


class ArchitectureEvidenceTests(unittest.TestCase):
    def setUp(self):
        self.temp = Path(tempfile.mkdtemp(prefix="architecture-test-", dir=evidence.TARGET))

    def tearDown(self):
        shutil.rmtree(self.temp, ignore_errors=True)

    def fake_binary(self, name="fake-tailtriage"):
        path = self.temp / name
        path.write_text(
            """#!/usr/bin/env python3
import json, pathlib, sys
if sys.argv[1:3] == ['import', 'tracing-spans-jsonl']:
    out = pathlib.Path(sys.argv[sys.argv.index('--output') + 1])
    out.write_text('{"requests":[{}]}\\n')
else:
    print(json.dumps({"request_count": 1, "warnings": [],
      "primary_suspect": {"kind": "insufficient_evidence", "score": 0, "confidence": "low"},
      "secondary_suspects": [], "route_breakdowns": [], "temporal_segments": []}))
""",
            encoding="utf-8",
        )
        path.chmod(0o755)
        return path

    def record(self, name="record"):
        output = self.temp / name
        evidence.record(output, str(self.fake_binary(name + "-binary")))
        return output

    # TT-TEST: support
    def test_plan_bytes_and_case_order_are_deterministic(self):
        first = evidence.canonical_bytes(evidence.build_plan())
        second = evidence.canonical_bytes(evidence.build_plan())
        self.assertEqual(first, second)
        ids = [case["id"] for case in json.loads(first)["cases"]]
        self.assertEqual(ids[:9], [f"analyzer-fixture:{Path(name).stem}" for name in evidence.FIXTURES])
        self.assertEqual(len(ids), 20)

    # TT-TEST: support
    def test_output_confinement_and_escape_rejection(self):
        self.assertEqual(evidence.confined_output(self.temp / "child"), (self.temp / "child").resolve())
        for bad in (evidence.TARGET, evidence.REPO / "docs" / "evidence", "target/../docs/evidence"):
            with self.assertRaises(evidence.EvidenceError):
                evidence.confined_output(bad)

    # TT-TEST: support
    def test_changed_and_missing_or_extra_input_fail_verification(self):
        output = self.record()
        inventory = evidence.read_json(output / "input-inventory.json")
        local = output / inventory[0]["evidence_path"]
        original = local.read_bytes()
        local.write_bytes(original + b"changed")
        with self.assertRaisesRegex(evidence.EvidenceError, "input bytes/hash changed"):
            evidence.validate_saved(output)
        local.write_bytes(original)
        (output / "inputs" / "extra").write_bytes(b"extra")
        with self.assertRaisesRegex(evidence.EvidenceError, "missing/extra input evidence"):
            evidence.validate_saved(output)
        (output / "inputs" / "extra").unlink()
        local.unlink()
        with self.assertRaisesRegex(evidence.EvidenceError, "missing/extra input evidence"):
            evidence.validate_saved(output)

    # TT-TEST: support
    def test_projection_ignores_json_object_key_order(self):
        left = b'{"warnings":[],"request_count":2,"unprojected":1}'
        right = b'{"unprojected":9,"request_count":2,"warnings":[]}'
        self.assertEqual(evidence.canonical_bytes(evidence.project_report(left)), evidence.canonical_bytes(evidence.project_report(right)))

    # TT-TEST: support
    def test_binary_and_raw_result_hash_drift_fail_verification(self):
        output = self.record()
        provenance = evidence.read_json(output / "provenance.json")
        binary = evidence.REPO / provenance["binary"]["path"]
        binary.write_bytes(binary.read_bytes() + b"changed")
        with self.assertRaisesRegex(evidence.EvidenceError, "binary bytes/hash changed"):
            evidence.validate_saved(output)
        output = self.record("raw-record")
        row = evidence.read_json(next((output / "cases").glob("*/result.json")))
        (output / row["stages"][0]["stdout_path"]).write_bytes(b"changed")
        with self.assertRaisesRegex(evidence.EvidenceError, "saved raw process bytes/hash changed"):
            evidence.validate_saved(output)

    # TT-TEST: support
    def test_aggregate_construction_is_deterministic(self):
        row = {"id": "x", "source_sha256": "a", "imported_run_sha256": None, "stages": [{"stage": "analyze", "command": ["x"], "exit_code": 0, "stdout_sha256": "b", "stderr_sha256": "c"}], "projection_sha256": "d"}
        first = evidence.aggregate_payload("p", "b", {"overrides": []}, [row])
        self.assertEqual(evidence.canonical_bytes(first), evidence.canonical_bytes(evidence.aggregate_payload("p", "b", {"overrides": []}, [row])))

    # TT-TEST: support
    def test_compare_rejects_incompatible_identity(self):
        left, right = self.record("left"), self.record("right")
        plan = evidence.read_json(right / "plan.json")
        plan["cases"][0]["source_sha256"] = "0" * 64
        (right / "plan.json").write_bytes(evidence.canonical_bytes(plan))
        with self.assertRaises(evidence.EvidenceError):
            evidence.compare(left, right, self.temp / "comparison")

    # TT-TEST: support
    def test_compare_identifies_changed_case_ids_deterministically(self):
        left, right = self.record("left"), self.record("right")
        right_results = []
        for case in evidence.read_json(right / "plan.json")["cases"]:
            result_path = right / "cases" / evidence.safe_case_id(case["id"]) / "result.json"
            row = evidence.read_json(result_path)
            if row["id"] == "analyzer-fixture:queue_saturation":
                projection_path = right / row["projection_path"]
                projection = evidence.read_json(projection_path)
                projection["report"]["request_count"] = 99
                projection_path.write_bytes(evidence.canonical_bytes(projection))
                row["projection_sha256"] = evidence.file_sha256(projection_path)
                evidence.write_json(result_path, row)
            right_results.append(row)
        provenance = evidence.read_json(right / "provenance.json")
        plan = evidence.read_json(right / "plan.json")
        aggregate = evidence.aggregate_payload(provenance["plan_sha256"], provenance["binary"]["sha256"], plan["global_analyzer_config"], right_results)
        evidence.write_json(right / "aggregate.json", aggregate)
        (right / "aggregate-sha256.txt").write_text(evidence.sha256(evidence.canonical_bytes(aggregate)) + "\n")
        report = evidence.compare(left, right, self.temp / "comparison")
        self.assertEqual(report["changed_case_ids"], ["analyzer-fixture:queue_saturation"])
        self.assertEqual(report["normalized_projection_changed_case_ids"], ["analyzer-fixture:queue_saturation"])

    # TT-TEST: support
    def test_built_and_supplied_modes_are_distinct(self):
        binary = self.fake_binary()
        self.assertEqual(evidence.prepare_binary(str(binary))[1], "supplied")
        with mock.patch("subprocess.run") as run:
            with mock.patch.object(Path, "is_file", return_value=True):
                self.assertEqual(evidence.prepare_binary(None)[1], "built")
            run.assert_called_once()

    # TT-TEST: support
    def test_strict_ambiguous_and_tracing_commands(self):
        case = {"artifact_type": "run_artifact", "artifact_policy": "strict", "analyzer_overrides": []}
        self.assertNotIn("--allow-ambiguous-artifact", evidence.command_description("analyze", case))
        case["artifact_policy"] = "allow_ambiguous"
        self.assertIn("--allow-ambiguous-artifact", evidence.command_description("analyze", case))
        case["artifact_type"] = "tracing_span_jsonl"
        self.assertEqual(evidence.command_description("import", case)[1:3], ["import", "tracing-spans-jsonl"])
        self.assertIn("$IMPORTED_RUN", evidence.command_description("analyze", case))

    # TT-TEST: support
    def test_numeric108_uses_independent_script_subprocess(self):
        numeric_dir = self.temp / "numeric"
        numeric_dir.mkdir()
        (numeric_dir / "aggregate-sha256.txt").write_text("abc\n")
        with mock.patch("subprocess.run") as run:
            run.return_value = mock.Mock(returncode=0, stdout=b"verified", stderr=b"")
            evidence.numeric108(self.temp / "link", numeric_dir, False)
        command = run.call_args.args[0]
        self.assertTrue(command[1].endswith("scripts/analyzer_numeric_sensitivity.py"))
        self.assertEqual(command[-1], "verify")
        self.assertEqual(evidence.read_json(self.temp / "link/numeric108-link.json")["verified_aggregate_sha256"], "abc")


if __name__ == "__main__":
    unittest.main()
