import hashlib
import json
from pathlib import Path
import shutil
import subprocess
import sys
import tempfile
import unittest


SOURCE = Path(__file__).parents[1] / "check_diagnostic_fixture_integrity.py"


class FixtureIntegrityTest(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        (self.root / "scripts").mkdir()
        shutil.copy2(SOURCE, self.root / "scripts" / SOURCE.name)
        self.diagnostics = self.root / "validation" / "diagnostics"
        (self.diagnostics / "corpus").mkdir(parents=True)
        self.run_path = self.diagnostics / "corpus" / "run.json"
        self.trace_path = self.diagnostics / "corpus" / "trace.jsonl"
        self.run_path.write_bytes(self.run_bytes())
        self.trace_path.write_bytes(self.trace_bytes())
        self.write_manifest()
        result = self.command("--refresh")
        self.assertEqual(result.returncode, 0, result.stderr)

    def tearDown(self):
        self.temp.cleanup()

    @staticmethod
    def run_bytes(request_id="r1", outcome="ok"):
        value = {
            "requests": [{"request_id": request_id, "outcome": outcome,
                          "started_at_run_us": 1, "finished_at_run_us": 2}],
            "stages": [{"completed": False, "started_at_run_us": 1,
                        "finished_at_run_us": 2}],
            "queues": [{"waited_from_run_us": 1, "waited_until_run_us": 2}],
            "inflight": [], "runtime_snapshots": [],
        }
        return (json.dumps(value, separators=(",", ":")) + "\n").encode()

    @staticmethod
    def trace_bytes(request_id="r1", extra=False):
        records = [{
            "format": "tailtriage.tracing-span.v1",
            "span": {"fields": {"tt.kind": "request", "tt.request_id": request_id}},
        }]
        if extra:
            records.append({"format": "tailtriage.tracing-span.v1",
                            "span": {"fields": {"tt.kind": "queue"}}})
        return ("\n".join(json.dumps(record) for record in records) + "\n").encode()

    def cases(self):
        return [
            {"id": "run", "validation_class": "analyzer_execution",
             "artifact_type": "run_artifact", "artifact": "corpus/run.json",
             "accuracy_eligible": True, "observation_id": "run"},
            {"id": "trace", "validation_class": "analyzer_execution",
             "artifact_type": "tracing_span_jsonl", "artifact": "corpus/trace.jsonl",
             "accuracy_eligible": True, "observation_id": "trace"},
            {"id": "report", "validation_class": "report_contract",
             "artifact_type": "synthetic_analysis_report", "artifact": "ignored.json"},
        ]

    def write_manifest(self, cases=None):
        value = {"schema_version": 2, "cases": self.cases() if cases is None else cases}
        (self.diagnostics / "manifest.json").write_text(json.dumps(value) + "\n")

    def command(self, *args):
        return subprocess.run(
            [sys.executable, "scripts/check_diagnostic_fixture_integrity.py", *args],
            cwd=self.root, text=True, capture_output=True, check=False,
        )

    def lock(self):
        return json.loads((self.diagnostics / "analyzer-fixtures.lock.json").read_text())

    def write_lock(self, value):
        (self.diagnostics / "analyzer-fixtures.lock.json").write_text(
            json.dumps(value, indent=2) + "\n"
        )

    def assert_failure(self, phrase, *args):
        result = self.command(*args)
        self.assertNotEqual(result.returncode, 0)
        self.assertIn(phrase, result.stderr)
        return result

    def assert_validation_failure(self, phrase, *args):
        result = self.assert_failure(phrase, *args)
        self.assertNotIn("Traceback", result.stderr)
        self.assertNotIn("TypeError", result.stderr)
        return result

    # TT-TEST: F04 primary
    def test_valid_inventory_passes(self):
        result = self.command()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("match the integrity lock", result.stdout)

    # TT-TEST: F04 primary
    def test_valid_lock_format_passes(self):
        self.assertEqual(self.lock()["format"], "tailtriage.analyzer-fixture-lock.v1")
        self.assertEqual(self.command().returncode, 0)

    # TT-TEST: F04 primary
    def test_missing_lock_format_is_rejected(self):
        lock = self.lock()
        del lock["format"]
        self.write_lock(lock)
        self.assert_failure("missing lock field: format")

    # TT-TEST: F04 primary
    def test_non_string_lock_format_is_rejected(self):
        lock = self.lock()
        lock["format"] = 1
        self.write_lock(lock)
        self.assert_failure("lock format must be a string")

    # TT-TEST: F04 primary
    def test_unsupported_lock_format_is_rejected(self):
        lock = self.lock()
        lock["format"] = "tailtriage.analyzer-fixture-lock.v2"
        self.write_lock(lock)
        self.assert_failure("unsupported lock format")

    # TT-TEST: F04 primary
    def test_legacy_schema_version_fields_are_rejected(self):
        lock = self.lock()
        lock.update(schema_version=1, manifest_schema_version=2)
        self.write_lock(lock)
        result = self.assert_failure("unknown lock field: schema_version")
        self.assertIn("unknown lock field: manifest_schema_version", result.stderr)

    # TT-TEST: F04 primary
    def test_refresh_writes_only_new_format_marker(self):
        lock = self.lock()
        lock.update(schema_version=1, manifest_schema_version=2)
        self.write_lock(lock)
        self.assertEqual(self.command("--refresh").returncode, 0)
        self.assertEqual(set(self.lock()), {"format", "fixtures"})

    # TT-TEST: F04 primary
    def test_refresh_is_byte_deterministic(self):
        before = (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes()
        self.assertEqual(self.command("--refresh").returncode, 0)
        self.assertEqual(before, (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes())

    # TT-TEST: F04 primary
    def test_refresh_writes_only_lock_file(self):
        before = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        self.command("--refresh")
        after = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)
        self.assertEqual(before[Path("validation/diagnostics/corpus/run.json")], self.run_path.read_bytes())

    # TT-TEST: F04 primary
    def test_check_mode_is_read_only(self):
        before = {p.relative_to(self.root): (p.stat().st_mtime_ns, p.read_bytes())
                  for p in self.root.rglob("*") if p.is_file()}
        self.command()
        after = {p.relative_to(self.root): (p.stat().st_mtime_ns, p.read_bytes())
                 for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)

    def mutate_entry(self, mutation):
        lock = self.lock()
        mutation(lock["fixtures"][0])
        self.write_lock(lock)

    # TT-TEST: F04 primary
    def test_non_object_lock_entry_is_rejected(self):
        for value in (None, "fixture", 42, []):
            with self.subTest(value=value):
                lock = self.lock()
                lock["fixtures"].append(value)
                self.write_lock(lock)
                self.assert_failure("lock fixture 2 must be an object")
                self.command("--refresh")

    # TT-TEST: F04 primary
    def test_missing_lock_entry_field_is_rejected(self):
        self.mutate_entry(lambda entry: entry.pop("artifact"))
        self.assert_failure("lock fixture 0 missing field: artifact")

    # TT-TEST: F04 primary
    def test_unknown_lock_entry_field_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(extra=True))
        self.assert_failure("lock fixture 0 unknown field: extra")

    # TT-TEST: F04 primary
    def test_empty_lock_case_id_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(case_id=""))
        self.assert_failure("case_id must be a non-empty string")

    # TT-TEST: F04 primary
    def test_invalid_lock_artifact_type_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(artifact_type="report"))
        self.assert_failure("artifact_type must be run_artifact or tracing_span_jsonl")

    # TT-TEST: F04 primary
    def test_unhashable_lock_artifact_type_is_rejected(self):
        for value in ([], {}):
            with self.subTest(value=value):
                self.mutate_entry(lambda entry, value=value: entry.update(artifact_type=value))
                self.assert_validation_failure(
                    "artifact_type must be run_artifact or tracing_span_jsonl"
                )
                self.command("--refresh")

    # TT-TEST: F04 primary
    def test_empty_lock_artifact_path_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(artifact=""))
        self.assert_failure("artifact must be a non-empty string")

    # TT-TEST: F04 primary
    def test_invalid_sha256_type_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(sha256=42))
        self.assert_failure("sha256 must be a string")

    # TT-TEST: F04 primary
    def test_invalid_sha256_length_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(sha256="a" * 63))
        self.assert_failure("sha256 must be exactly 64 lowercase hexadecimal characters")

    # TT-TEST: F04 primary
    def test_uppercase_sha256_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(sha256="A" * 64))
        self.assert_failure("sha256 must be exactly 64 lowercase hexadecimal characters")

    # TT-TEST: F04 primary
    def test_non_hex_sha256_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(sha256="g" * 64))
        self.assert_failure("sha256 must be exactly 64 lowercase hexadecimal characters")

    # TT-TEST: F04 primary
    def test_negative_byte_length_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(byte_length=-1))
        self.assert_failure("byte_length must be a non-negative integer")

    # TT-TEST: F04 primary
    def test_boolean_byte_length_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(byte_length=True))
        self.assert_failure("byte_length must be a non-negative integer")

    # TT-TEST: F04 primary
    def test_non_object_shape_is_rejected(self):
        self.mutate_entry(lambda entry: entry.update(shape=[]))
        self.assert_failure("shape must be an object")

    # TT-TEST: F04 primary
    def test_multiple_malformed_entries_are_stably_sorted(self):
        lock = self.lock()
        lock["fixtures"] = [None, {"case_id": ""}]
        self.write_lock(lock)
        lines = self.command().stderr.splitlines()
        self.assertEqual(lines, sorted(lines))
        self.assertIn("lock fixture 0 must be an object", lines)
        self.assertIn("lock fixture 1 missing field: artifact", lines)

    # TT-TEST: F04 primary
    def test_changed_bytes_are_detected(self):
        self.run_path.write_bytes(self.run_bytes(outcome="error"))
        self.assert_failure("sha256 mismatch for run")

    # TT-TEST: F04 primary
    def test_byte_length_drift_is_detected(self):
        lock = self.lock()
        lock["fixtures"][0]["byte_length"] += 1
        self.write_lock(lock)
        self.assert_failure("byte length mismatch")

    # TT-TEST: F04 primary
    def test_missing_artifact_is_detected(self):
        self.run_path.unlink()
        self.assert_failure("artifact is not a regular file")

    # TT-TEST: F04 primary
    def test_missing_lock_entry_is_detected(self):
        lock = self.lock()
        lock["fixtures"] = [entry for entry in lock["fixtures"] if entry["case_id"] != "run"]
        self.write_lock(lock)
        self.assert_failure("missing lock entry: run")

    # TT-TEST: F04 primary
    def test_unexpected_lock_entry_is_detected(self):
        lock = self.lock()
        old = dict(lock["fixtures"][0])
        old["case_id"] = "old"
        lock["fixtures"].append(old)
        self.write_lock(lock)
        self.assert_failure("unexpected lock entry: old")

    # TT-TEST: F04 primary
    def test_artifact_type_mismatch_is_detected(self):
        lock = self.lock()
        lock["fixtures"][0]["artifact_type"] = "tracing_span_jsonl"
        self.write_lock(lock)
        self.assert_failure("artifact type mismatch")

    # TT-TEST: F04 primary
    def test_artifact_path_mismatch_is_detected(self):
        lock = self.lock()
        lock["fixtures"][0]["artifact"] = "corpus/other.json"
        self.write_lock(lock)
        self.assert_failure("artifact path mismatch")

    # TT-TEST: F04 primary
    def test_duplicate_manifest_case_id_is_rejected(self):
        cases = self.cases()
        cases[1]["id"] = "run"
        self.write_manifest(cases)
        self.assert_failure("duplicate manifest case ID: run")

    # TT-TEST: F04 primary
    def test_duplicate_manifest_artifact_path_is_rejected(self):
        cases = self.cases()
        cases[1]["artifact"] = "corpus/run.json"
        self.write_manifest(cases)
        self.assert_failure("duplicate manifest artifact path: corpus/run.json")

    # TT-TEST: F04 primary
    def test_unhashable_manifest_fields_are_rejected(self):
        fields = {
            "id": "non-empty case ID required",
            "artifact_type": "invalid artifact type",
            "artifact": "non-empty artifact path required",
            "observation_id": "non-empty observation_id required",
        }
        for field, phrase in fields.items():
            for value in ([], {}):
                for args in ((), ("--refresh",)):
                    with self.subTest(field=field, value=value, args=args):
                        cases = self.cases()
                        cases[0][field] = value
                        self.write_manifest(cases)
                        self.assert_validation_failure(phrase, *args)

    # TT-TEST: F04 primary
    def test_combined_unhashable_manifest_values_report_all_errors(self):
        cases = self.cases()
        cases[0].update(id=[], artifact_type={}, artifact=[], observation_id={})
        self.write_manifest(cases)
        expected = {
            "invalid artifact type for case 0",
            "non-empty artifact path required for case 0",
            "non-empty case ID required for analyzer case 0",
            "non-empty observation_id required for accuracy-eligible analyzer case 0",
        }
        lock_before = (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes()
        for args in ((), ("--refresh",)):
            with self.subTest(args=args):
                result = self.assert_validation_failure("invalid artifact type", *args)
                lines = result.stderr.splitlines()
                self.assertEqual(lines, sorted(lines))
                self.assertTrue(expected.issubset(lines))
        self.assertEqual(
            lock_before,
            (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes(),
        )

    def duplicate_run_cases(self, first_observation="one", second_observation="two",
                            accuracy_eligible=True):
        duplicate = self.diagnostics / "corpus" / "run-copy.json"
        duplicate.write_bytes(self.run_path.read_bytes())
        return [
            {"id": "a", "validation_class": "analyzer_execution",
             "artifact_type": "run_artifact", "artifact": "corpus/run.json",
             "accuracy_eligible": accuracy_eligible, "observation_id": first_observation},
            {"id": "b", "validation_class": "analyzer_execution",
             "artifact_type": "run_artifact", "artifact": "corpus/run-copy.json",
             "accuracy_eligible": accuracy_eligible, "observation_id": second_observation},
        ]

    # TT-TEST: F04 primary
    def test_distinct_accuracy_observations_cannot_share_bytes(self):
        self.write_manifest(self.duplicate_run_cases())
        self.assert_failure("identical analyzer artifact bytes are assigned to distinct accuracy observations")

    # TT-TEST: F04 primary
    def test_equivalent_cases_with_same_observation_id_may_share_bytes(self):
        self.write_manifest(self.duplicate_run_cases(second_observation="one"))
        self.assertEqual(self.command("--refresh").returncode, 0)
        self.assertEqual(self.command().returncode, 0)

    # TT-TEST: F04 primary
    def test_non_accuracy_cases_may_share_bytes(self):
        self.write_manifest(self.duplicate_run_cases(accuracy_eligible=False))
        self.assertEqual(self.command("--refresh").returncode, 0)
        self.assertEqual(self.command().returncode, 0)

    # TT-TEST: F04 primary
    def test_accuracy_case_requires_observation_id(self):
        cases = self.cases()
        cases[0].pop("observation_id")
        self.write_manifest(cases)
        self.assert_failure("non-empty observation_id required for accuracy-eligible analyzer run")

    # TT-TEST: F04 primary
    def test_duplicate_diagnostic_is_deterministic(self):
        cases = self.duplicate_run_cases()
        cases.reverse()
        self.write_manifest(cases)
        result = self.command()
        digest = hashlib.sha256(self.run_path.read_bytes()).hexdigest()
        expected = ("identical analyzer artifact bytes are assigned to distinct accuracy "
                    f"observations: {digest}: a/one, b/two")
        self.assertIn(expected, result.stderr.splitlines())

    # TT-TEST: F04 primary
    def test_report_contract_is_excluded(self):
        self.assertEqual({entry["case_id"] for entry in self.lock()["fixtures"]}, {"run", "trace"})

    # TT-TEST: F04 primary
    def test_absolute_artifact_path_is_rejected(self):
        cases = self.cases()
        cases[0]["artifact"] = str(self.run_path)
        self.write_manifest(cases)
        self.assert_failure("absolute artifact path rejected")

    # TT-TEST: F04 primary
    def test_parent_path_escape_is_rejected(self):
        cases = self.cases()
        cases[0]["artifact"] = "../outside.json"
        self.write_manifest(cases)
        self.assert_failure("artifact path escapes diagnostics root")

    # TT-TEST: F04 primary
    def test_symlink_escape_is_rejected(self):
        outside = self.root / "outside.json"
        outside.write_bytes(self.run_bytes())
        (self.diagnostics / "corpus" / "escape.json").symlink_to(outside)
        cases = self.cases()
        cases[0]["artifact"] = "corpus/escape.json"
        self.write_manifest(cases)
        self.assert_failure("artifact path escapes diagnostics root")

    # TT-TEST: F04 primary
    def test_invalid_artifact_path_is_rejected_without_crashing(self):
        cases = self.cases()
        cases[0]["artifact"] = "corpus/\0invalid.json"
        self.write_manifest(cases)
        lock_before = (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes()
        for args in ((), ("--refresh",)):
            with self.subTest(args=args):
                result = self.assert_validation_failure("invalid artifact path for run", *args)
                for exception in ("ValueError", "OSError", "RuntimeError"):
                    self.assertNotIn(exception, result.stderr)
        self.assertEqual(
            lock_before,
            (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes(),
        )

    # TT-TEST: F04 primary
    def test_invalid_utf8_is_rejected(self):
        self.run_path.write_bytes(b"\xff\n")
        self.assert_failure("invalid UTF-8")

    # TT-TEST: F04 primary
    def test_crlf_is_rejected(self):
        self.run_path.write_bytes(self.run_bytes().replace(b"\n", b"\r\n"))
        self.assert_failure("CR byte found")

    # TT-TEST: F04 primary
    def test_missing_final_lf_is_rejected(self):
        self.run_path.write_bytes(self.run_bytes().rstrip(b"\n"))
        self.assert_failure("missing final LF")

    # TT-TEST: F04 primary
    def test_blank_jsonl_line_is_rejected(self):
        self.trace_path.write_bytes(self.trace_bytes() + b"\n")
        self.assert_failure("blank line after final content")

    # TT-TEST: F04 primary
    def test_malformed_run_json_is_rejected(self):
        self.run_path.write_bytes(b"{no}\n")
        self.assert_failure("malformed Run JSON")

    # TT-TEST: F04 primary
    def test_malformed_tracing_jsonl_is_rejected(self):
        self.trace_path.write_bytes(b"{no}\n")
        self.assert_failure("malformed tracing JSONL")

    # TT-TEST: F04 primary
    def test_wrong_tracing_format_marker_is_rejected(self):
        record = {"format": "wrong", "span": {"fields": {}}}
        self.trace_path.write_text(json.dumps(record) + "\n")
        self.assert_failure("wrong tracing format marker")

    # TT-TEST: F04 primary
    def test_non_string_tracing_kind_is_rejected_without_crashing(self):
        for kind in ([], {}, 42, True):
            for args in ((), ("--refresh",)):
                with self.subTest(kind=kind, args=args):
                    record = {
                        "format": "tailtriage.tracing-span.v1",
                        "span": {"fields": {"tt.kind": kind}},
                    }
                    self.trace_path.write_text(json.dumps(record) + "\n")
                    lock_before = (
                        self.diagnostics / "analyzer-fixtures.lock.json"
                    ).read_bytes()
                    result = self.assert_validation_failure(
                        "span tt.kind must be a string on line 1", *args
                    )
                    self.assertEqual(
                        lock_before,
                        (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes(),
                    )

    # TT-TEST: F04 primary
    def test_missing_and_unknown_tracing_kinds_are_other(self):
        records = [
            {"format": "tailtriage.tracing-span.v1", "span": {"fields": {}}},
            {
                "format": "tailtriage.tracing-span.v1",
                "span": {"fields": {"tt.kind": "custom"}},
            },
        ]
        self.trace_path.write_text(
            "\n".join(json.dumps(record) for record in records) + "\n"
        )
        self.assertEqual(self.command("--refresh").returncode, 0)
        entry = next(
            item for item in self.lock()["fixtures"] if item["case_id"] == "trace"
        )
        self.assertEqual(entry["shape"]["other_span_count"], 2)
        self.assertEqual(self.command().returncode, 0)

    # TT-TEST: F04 primary
    def test_run_shape_drift_is_detected(self):
        lock = self.lock()
        entry = next(item for item in lock["fixtures"] if item["case_id"] == "run")
        entry["shape"]["request_count"] = 9
        self.write_lock(lock)
        self.assert_failure("shape mismatch for run at request_count")

    # TT-TEST: F04 primary
    def test_tracing_shape_drift_is_detected(self):
        lock = self.lock()
        entry = next(item for item in lock["fixtures"] if item["case_id"] == "trace")
        entry["shape"]["request_span_count"] = 9
        self.write_lock(lock)
        self.assert_failure("shape mismatch for trace at request_span_count")

    # TT-TEST: F04 primary
    def test_all_failures_are_reported_in_stable_order(self):
        lock = self.lock()
        for entry in lock["fixtures"]:
            entry["sha256"] = "0" * 64
            entry["byte_length"] = 0
        self.write_lock(lock)
        result = self.command()
        lines = result.stderr.splitlines()
        self.assertEqual(lines, sorted(lines))
        self.assertEqual(4, len(lines))
        self.assertTrue(any("sha256 mismatch for run" in line for line in lines))
        self.assertTrue(any("sha256 mismatch for trace" in line for line in lines))


if __name__ == "__main__":
    unittest.main()
