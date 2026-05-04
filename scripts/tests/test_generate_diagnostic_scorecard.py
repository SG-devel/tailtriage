import json
import tempfile
import unittest
from pathlib import Path

from scripts.generate_diagnostic_scorecard import (
    collect_environment,
    get_tailtriage_versions,
    manifest_and_artifact_hashes,
    render_failed_cases,
    render_scorecard,
    sha256_bytes,
)


class GenerateScorecardTests(unittest.TestCase):
    def test_sha256_stable(self):
        self.assertEqual(sha256_bytes(b"abc"), sha256_bytes(b"abc"))

    def test_versions_workspace_and_explicit(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "Cargo.toml").write_text(
                """
[workspace]
members = ["a", "b"]
[workspace.package]
version = "1.2.3"
""",
                encoding="utf-8",
            )
            (root / "a").mkdir()
            (root / "a/Cargo.toml").write_text('[package]\nname="tailtriage"\nversion={ workspace = true }\n', encoding="utf-8")
            (root / "b").mkdir()
            (root / "b/Cargo.toml").write_text('[package]\nname="tailtriage-core"\nversion="9.9.9"\n', encoding="utf-8")
            versions = get_tailtriage_versions(root)
            self.assertEqual(versions["workspace_package_version"], "1.2.3")
            self.assertEqual(versions["packages"]["tailtriage"], "1.2.3")
            self.assertEqual(versions["packages"]["tailtriage-core"], "9.9.9")

    def test_artifact_hash_changes(self):
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            (root / "a.json").write_text('{"x":1}', encoding="utf-8")
            (root / "manifest.json").write_text(json.dumps({"cases": [{"artifact": "a.json"}]}), encoding="utf-8")
            h1 = manifest_and_artifact_hashes(root / "manifest.json")[1]
            (root / "a.json").write_text('{"x":2}', encoding="utf-8")
            h2 = manifest_and_artifact_hashes(root / "manifest.json")[1]
            self.assertNotEqual(h1, h2)

    def test_render_contains_non_claims_and_metrics(self):
        env = {
            "generated_at_utc": "t",
            "snapshot_label": "s",
            "git": {"sha": "a", "tag": "v1", "describe": "d"},
            "tailtriage": {"workspace_package_version": "1", "packages": {"tailtriage": "1"}},
            "github_actions": {"run_id": None, "ref": None, "runner_os": None, "runner_arch": None, "image_version": None},
            "software": {"python": "3", "rustc": "r", "cargo": "c"},
            "hardware": {"cpu_model": "cpu", "logical_cores": 1, "memory_total_kib": 2},
            "inputs": {"manifest_sha256": "m", "referenced_artifacts_sha256": "a", "thresholds": {"min_top1": 0.75}},
        }
        metrics = {"failed_cases": [], "per_ground_truth_counts": {}, "confidence_bucket_accuracy": {}}
        for k in ["total_cases", "top1_accuracy", "top2_recall", "high_confidence_wrong_count", "required_evidence_pass_rate", "next_check_required_cases", "next_check_passed_cases", "next_check_pass_rate", "next_check_presence_rate", "confidence_ceiling_cases", "confidence_ceiling_passed_cases", "confidence_ceiling_pass_rate", "unexpected_warning_count", "missing_expected_warning_count"]:
            metrics[k] = 0
        text = render_scorecard(metrics, env)
        self.assertIn("failed_case_count", text)
        self.assertIn("not root-cause proof", text)

    def test_failed_case_rendering(self):
        self.assertEqual(render_failed_cases([]).strip(), "None")
        table = render_failed_cases([{"id": "a", "top1_ok": False, "top2_ok": True, "evidence_ok": True, "next_check_ok": True, "confidence_ceiling_ok": True}])
        self.assertIn("| a |", table)

    def test_environment_keys(self):
        repo = Path(__file__).resolve().parents[2]
        env = collect_environment(repo, repo / "validation/diagnostics/manifest.json", "x", {"min_top1": 0.75, "min_top2": 0.9, "max_high_confidence_wrong": 0})
        for key in ["schema_version", "generated_at_utc", "git", "tailtriage", "github_actions", "software", "hardware", "inputs"]:
            self.assertIn(key, env)


if __name__ == "__main__":
    unittest.main()
