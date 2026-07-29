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
             "artifact_type": "run_artifact", "artifact": "corpus/run.json"},
            {"id": "trace", "validation_class": "analyzer_execution",
             "artifact_type": "tracing_span_jsonl", "artifact": "corpus/trace.jsonl"},
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

    def test_valid_inventory_passes(self):
        result = self.command()
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn("match the integrity lock", result.stdout)

    def test_refresh_is_byte_deterministic(self):
        before = (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes()
        self.assertEqual(self.command("--refresh").returncode, 0)
        self.assertEqual(before, (self.diagnostics / "analyzer-fixtures.lock.json").read_bytes())

    def test_refresh_writes_only_lock_file(self):
        before = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        self.command("--refresh")
        after = {p.relative_to(self.root): p.read_bytes() for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)
        self.assertEqual(before[Path("validation/diagnostics/corpus/run.json")], self.run_path.read_bytes())

    def test_check_mode_is_read_only(self):
        before = {p.relative_to(self.root): (p.stat().st_mtime_ns, p.read_bytes())
                  for p in self.root.rglob("*") if p.is_file()}
        self.command()
        after = {p.relative_to(self.root): (p.stat().st_mtime_ns, p.read_bytes())
                 for p in self.root.rglob("*") if p.is_file()}
        self.assertEqual(before, after)

    def test_changed_bytes_are_detected(self):
        self.run_path.write_bytes(self.run_bytes(outcome="error"))
        self.assert_failure("sha256 mismatch for run")

    def test_byte_length_drift_is_detected(self):
        lock = self.lock()
        lock["fixtures"][0]["byte_length"] += 1
        self.write_lock(lock)
        self.assert_failure("byte length mismatch")

    def test_missing_artifact_is_detected(self):
        self.run_path.unlink()
        self.assert_failure("artifact is not a regular file")

    def test_missing_lock_entry_is_detected(self):
        lock = self.lock()
        lock["fixtures"] = [entry for entry in lock["fixtures"] if entry["case_id"] != "run"]
        self.write_lock(lock)
        self.assert_failure("missing lock entry: run")

    def test_unexpected_lock_entry_is_detected(self):
        lock = self.lock()
        lock["fixtures"].append({"case_id": "old"})
        self.write_lock(lock)
        self.assert_failure("unexpected lock entry: old")

    def test_artifact_type_mismatch_is_detected(self):
        lock = self.lock()
        lock["fixtures"][0]["artifact_type"] = "tracing_span_jsonl"
        self.write_lock(lock)
        self.assert_failure("artifact type mismatch")

    def test_artifact_path_mismatch_is_detected(self):
        lock = self.lock()
        lock["fixtures"][0]["artifact"] = "corpus/other.json"
        self.write_lock(lock)
        self.assert_failure("artifact path mismatch")

    def test_duplicate_manifest_case_id_is_rejected(self):
        cases = self.cases()
        cases[1]["id"] = "run"
        self.write_manifest(cases)
        self.assert_failure("duplicate manifest case ID: run")

    def test_duplicate_manifest_artifact_path_is_rejected(self):
        cases = self.cases()
        cases[1]["artifact"] = "corpus/run.json"
        self.write_manifest(cases)
        self.assert_failure("duplicate manifest artifact path: corpus/run.json")

    def test_report_contract_is_excluded(self):
        self.assertEqual({entry["case_id"] for entry in self.lock()["fixtures"]}, {"run", "trace"})

    def test_absolute_artifact_path_is_rejected(self):
        cases = self.cases()
        cases[0]["artifact"] = str(self.run_path)
        self.write_manifest(cases)
        self.assert_failure("absolute artifact path rejected")

    def test_parent_path_escape_is_rejected(self):
        cases = self.cases()
        cases[0]["artifact"] = "../outside.json"
        self.write_manifest(cases)
        self.assert_failure("artifact path escapes diagnostics root")

    def test_symlink_escape_is_rejected(self):
        outside = self.root / "outside.json"
        outside.write_bytes(self.run_bytes())
        (self.diagnostics / "corpus" / "escape.json").symlink_to(outside)
        cases = self.cases()
        cases[0]["artifact"] = "corpus/escape.json"
        self.write_manifest(cases)
        self.assert_failure("artifact path escapes diagnostics root")

    def test_invalid_utf8_is_rejected(self):
        self.run_path.write_bytes(b"\xff\n")
        self.assert_failure("invalid UTF-8")

    def test_crlf_is_rejected(self):
        self.run_path.write_bytes(self.run_bytes().replace(b"\n", b"\r\n"))
        self.assert_failure("CR byte found")

    def test_missing_final_lf_is_rejected(self):
        self.run_path.write_bytes(self.run_bytes().rstrip(b"\n"))
        self.assert_failure("missing final LF")

    def test_blank_jsonl_line_is_rejected(self):
        self.trace_path.write_bytes(self.trace_bytes() + b"\n")
        self.assert_failure("blank line after final content")

    def test_malformed_run_json_is_rejected(self):
        self.run_path.write_bytes(b"{no}\n")
        self.assert_failure("malformed Run JSON")

    def test_malformed_tracing_jsonl_is_rejected(self):
        self.trace_path.write_bytes(b"{no}\n")
        self.assert_failure("malformed tracing JSONL")

    def test_wrong_tracing_format_marker_is_rejected(self):
        record = {"format": "wrong", "span": {"fields": {}}}
        self.trace_path.write_text(json.dumps(record) + "\n")
        self.assert_failure("wrong tracing format marker")

    def test_run_shape_drift_is_detected(self):
        lock = self.lock()
        entry = next(item for item in lock["fixtures"] if item["case_id"] == "run")
        entry["shape"]["request_count"] = 9
        self.write_lock(lock)
        self.assert_failure("shape mismatch for run at request_count")

    def test_tracing_shape_drift_is_detected(self):
        lock = self.lock()
        entry = next(item for item in lock["fixtures"] if item["case_id"] == "trace")
        entry["shape"]["request_span_count"] = 9
        self.write_lock(lock)
        self.assert_failure("shape mismatch for trace at request_span_count")

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
