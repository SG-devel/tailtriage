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
        evidence.TARGET.mkdir(parents=True, exist_ok=True)
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
        config = {"config": None, "overrides": []}
        self.assertNotIn("--allow-ambiguous-artifact", evidence.command_description("analyze", case, config))
        case["artifact_policy"] = "allow_ambiguous"
        self.assertIn("--allow-ambiguous-artifact", evidence.command_description("analyze", case, config))
        case["artifact_type"] = "tracing_span_jsonl"
        self.assertEqual(evidence.command_description("import", case, config)[1:3], ["import", "tracing-spans-jsonl"])
        self.assertIn("$IMPORTED_RUN", evidence.command_description("analyze", case, config))

    # TT-TEST: support
    def test_analyzer_configuration_is_ordered_and_analyze_only(self):
        config = {"config": {"evidence_path": "config/analyzer-config", "sha256": "a" * 64}, "overrides": ["scoring.queue.weight=2", "scoring.queue.weight=3"]}
        case = {"artifact_type": "tracing_span_jsonl", "artifact_policy": "strict", "analyzer_overrides": []}
        imported = evidence.command_description("import", case, config)
        analyzed = evidence.command_description("analyze", case, config)
        self.assertNotIn("--analyzer-config", imported)
        self.assertNotIn("--analyzer-set", imported)
        self.assertEqual(analyzed[-6:], ["--analyzer-config", "$ANALYZER_CONFIG", "--analyzer-set", "scoring.queue.weight=2", "--analyzer-set", "scoring.queue.weight=3"])
        local_config = self.temp / "evidence/config/analyzer-config"
        actual = evidence.actual_command(analyzed, Path("/bin/tailtriage"), Path("input"), Path("imported"), local_config)
        self.assertEqual(actual[actual.index("--analyzer-config") + 1], str(local_config))

    # TT-TEST: support
    def test_config_evidence_and_override_drift_fail_verification(self):
        config = self.temp / "settings.toml"
        config.write_bytes(b"[scoring.queue]\nweight = 2\n")
        output = self.temp / "configured"
        evidence.record(output, str(self.fake_binary()), config, ["scoring.queue.weight=3", "scoring.queue.weight=4"])
        provenance = evidence.read_json(output / "provenance.json")
        recorded = provenance["global_analyzer_config"]
        self.assertEqual(provenance["analyzer_config_source"]["source_path"], evidence.repository_path(config))
        self.assertEqual((output / recorded["config"]["evidence_path"]).read_bytes(), config.read_bytes())
        self.assertEqual(recorded["config"]["sha256"], evidence.file_sha256(config))
        self.assertEqual(recorded["overrides"], ["scoring.queue.weight=3", "scoring.queue.weight=4"])
        (output / recorded["config"]["evidence_path"]).write_bytes(b"changed")
        with self.assertRaisesRegex(evidence.EvidenceError, "saved analyzer config bytes/hash changed"):
            evidence.validate_saved(output)

        (output / recorded["config"]["evidence_path"]).write_bytes(b"[scoring.queue]\nweight = 2\n")
        config.write_bytes(b"[scoring.queue]\nweight = 9\n")
        with self.assertRaisesRegex(evidence.EvidenceError, "source analyzer config bytes/hash changed"):
            evidence.validate_saved(output)
        config.write_bytes(b"[scoring.queue]\nweight = 2\n")

        output = self.temp / "override-drift"
        evidence.record(output, str(self.fake_binary("other-binary")), config, ["scoring.queue.weight=3", "scoring.queue.weight=4"])
        plan = evidence.read_json(output / "plan.json")
        plan["global_analyzer_config"]["overrides"].reverse()
        (output / "plan.json").write_bytes(evidence.canonical_bytes(plan))
        with self.assertRaisesRegex(evidence.EvidenceError, "analyzer configuration provenance changed"):
            evidence.validate_saved(output)

    # TT-TEST: support
    def test_compare_allows_and_exposes_different_global_configuration(self):
        binary = self.fake_binary()
        left, right = self.temp / "left-config", self.temp / "right-config"
        evidence.record(left, str(binary), analyzer_overrides=["scoring.queue.weight=2"])
        evidence.record(right, str(binary), analyzer_overrides=["scoring.queue.weight=3"])
        report = evidence.compare(left, right, self.temp / "config-comparison")
        self.assertTrue(report["compatible_plan_input_identity"])
        self.assertEqual(report["left"]["global_analyzer_config"]["overrides"], ["scoring.queue.weight=2"])
        self.assertEqual(report["right"]["global_analyzer_config"]["overrides"], ["scoring.queue.weight=3"])
        self.assertNotEqual(evidence.read_json(left / "aggregate.json")["global_analyzer_config"], evidence.read_json(right / "aggregate.json")["global_analyzer_config"])

    # TT-TEST: support
    def test_config_identity_is_independent_of_source_path(self):
        binary = self.fake_binary()
        first_source_dir = Path(tempfile.mkdtemp(prefix="architecture-config-a-"))
        second_source_dir = Path(tempfile.mkdtemp(prefix="architecture-config-b-"))
        self.addCleanup(shutil.rmtree, first_source_dir, True)
        self.addCleanup(shutil.rmtree, second_source_dir, True)
        first_source = first_source_dir / "settings.toml"
        second_source = second_source_dir / "settings.toml"
        config_bytes = b"[queueing]\ntrigger_permille = 450\n"
        first_source.write_bytes(config_bytes)
        second_source.write_bytes(config_bytes)
        first = self.temp / "path-a"
        second = self.temp / "path-b"
        overrides = ["queueing.trigger_permille=451", "queueing.trigger_permille=452"]
        evidence.record(first, str(binary), first_source, overrides)
        evidence.record(second, str(binary), second_source, overrides)

        first_plan_bytes = (first / "plan.json").read_bytes()
        second_plan_bytes = (second / "plan.json").read_bytes()
        self.assertEqual(first_plan_bytes, second_plan_bytes)
        self.assertEqual(evidence.sha256(first_plan_bytes), evidence.sha256(second_plan_bytes))
        first_aggregate = evidence.read_json(first / "aggregate.json")
        second_aggregate = evidence.read_json(second / "aggregate.json")
        self.assertEqual(first_aggregate["global_analyzer_config"], second_aggregate["global_analyzer_config"])
        self.assertEqual((first / "aggregate-sha256.txt").read_bytes(), (second / "aggregate-sha256.txt").read_bytes())
        first_provenance = evidence.read_json(first / "provenance.json")
        second_provenance = evidence.read_json(second / "provenance.json")
        self.assertNotEqual(first_provenance["analyzer_config_source"]["source_path"], second_provenance["analyzer_config_source"]["source_path"])
        report = evidence.compare(first, second, self.temp / "path-comparison")
        self.assertEqual(report["changed_case_count"], 0)
        self.assertNotEqual(report["left"]["analyzer_config_source"], report["right"]["analyzer_config_source"])

    # TT-TEST: support
    def test_config_bytes_and_ordered_overrides_participate_in_identity(self):
        first = self.temp / "first.toml"
        second = self.temp / "second.toml"
        first.write_bytes(b"one")
        second.write_bytes(b"two")
        first_identity = {
            "config": {"evidence_path": "config/analyzer-config", "sha256": evidence.file_sha256(first)},
            "overrides": ["a=1", "b=2"],
        }
        second_identity = {
            "config": {"evidence_path": "config/analyzer-config", "sha256": evidence.file_sha256(second)},
            "overrides": ["a=1", "b=2"],
        }
        self.assertNotEqual(first_identity, second_identity)
        self.assertNotEqual(evidence.sha256(evidence.canonical_bytes(evidence.build_plan(global_analyzer_config=first_identity))), evidence.sha256(evidence.canonical_bytes(evidence.build_plan(global_analyzer_config=second_identity))))
        reordered = {"config": first_identity["config"], "overrides": ["b=2", "a=1"]}
        changed = {"config": first_identity["config"], "overrides": ["a=1", "b=3"]}
        self.assertNotEqual(first_identity, reordered)
        self.assertNotEqual(first_identity, changed)
        first_plan_hash = evidence.sha256(evidence.canonical_bytes(evidence.build_plan(global_analyzer_config=first_identity)))
        self.assertNotEqual(first_plan_hash, evidence.sha256(evidence.canonical_bytes(evidence.build_plan(global_analyzer_config=reordered))))
        self.assertNotEqual(first_plan_hash, evidence.sha256(evidence.canonical_bytes(evidence.build_plan(global_analyzer_config=changed))))

    # TT-TEST: support
    def test_record_captures_start_state_before_binary_preparation(self):
        order = []

        def git_value(*args):
            order.append(("git", args))
            return "head\n" if args[0] == "rev-parse" else " M already-dirty\n"

        def prepare(_supplied):
            order.append(("prepare",))
            raise evidence.EvidenceError("stop after ordering proof")

        with mock.patch.object(evidence, "git_value", side_effect=git_value), mock.patch.object(evidence, "prepare_binary", side_effect=prepare):
            with self.assertRaisesRegex(evidence.EvidenceError, "ordering proof"):
                evidence.record(self.temp / "ordering")
        self.assertEqual(order, [("git", ("rev-parse", "HEAD")), ("git", ("status", "--short")), ("prepare",)])

    # TT-TEST: support
    def test_field_differences_distinguishes_missing_keys_from_null(self):
        self.assertEqual(evidence.field_differences({}, {"field": None}), ["field"])
        self.assertEqual(evidence.field_differences({}, {"field": 1}), ["field"])
        self.assertEqual(evidence.field_differences({"field": None}, {"field": None}), [])
        self.assertEqual(evidence.field_differences({"outer": {}}, {"outer": {"field": None}}), ["outer.field"])

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

    # TT-TEST: support
    def test_architecture_manifest_inventory_hashes_and_fingerprints(self):
        manifest = evidence.load_architecture_manifest()
        self.assertEqual(manifest["format"], evidence.ARCHITECTURE_FORMAT)
        sentinels = {item["id"] for item in evidence.architecture_cases("visible_sentinel")}
        locked = {item["id"] for item in evidence.architecture_cases("locked_challenge")}
        self.assertEqual(sentinels, {"relation-typed-neutral-name", "relation-name-token-only-negative-control", "completed-plus-partial-extension", "locally-limited-ambiguity-peer", "trivial-downstream-fallback", "two-strong-independent-candidates", "weak-unrelated-noise", "same-family-representation-uniqueness", "route-small-n", "temporal-small-n"})
        self.assertEqual(locked, {"sparse-extreme-queue", "sparse-extreme-executor", "sparse-extreme-downstream", "same-magnitude-queue-sparse", "same-magnitude-queue-mature", "cross-family-queue-vs-executor", "cross-family-blocking-vs-downstream", "independent-ambiguity", "related-group-plus-independent-third", "scoped-nearest-rank-route-temporal"})
        self.assertEqual(evidence.definition_fingerprint("visible_sentinel"), evidence.definition_fingerprint("visible_sentinel"))
        self.assertEqual(evidence.definition_fingerprint("locked_challenge"), evidence.definition_fingerprint("locked_challenge"))

    # TT-TEST: support
    def test_suite_plans_are_explicit_and_disjoint(self):
        base = {case["source_path"] for case in evidence.build_plan()["cases"]}
        visible = evidence.build_architecture_plan("visible_sentinel")
        locked = evidence.build_architecture_plan("locked_challenge")
        self.assertTrue(all(case["source_class"] == "visible_sentinel" for case in visible["cases"]))
        self.assertTrue(base.isdisjoint(case["source_path"] for case in visible["cases"] + locked["cases"]))

    # TT-TEST: support
    def test_definition_check_never_prepares_binary(self):
        with mock.patch.object(evidence, "prepare_binary") as prepare:
            result = evidence.check_architecture_definitions()
        prepare.assert_not_called()
        self.assertEqual(result["locked_challenge"]["case_count"], 10)

    # TT-TEST: support
    def test_locked_execution_guard_precedes_output_and_binary(self):
        output = self.temp / "must-not-exist"
        with mock.patch.object(evidence, "prepare_binary") as prepare:
            self.assertEqual(evidence.main(["run-locked-challenges", "--output", str(output)]), 1)
        prepare.assert_not_called()
        self.assertFalse(output.exists())

    # TT-TEST: support
    def test_visible_record_reuses_binary_config_and_provenance(self):
        config = self.temp / "sentinel.toml"
        config.write_text("[queueing]\ntrigger_permille = 450\n")
        output = self.temp / "sentinels"
        evidence.record(output, str(self.fake_binary()), config, ["queueing.trigger_permille=451"], "visible_sentinel")
        provenance = evidence.read_json(output / "provenance.json")
        self.assertEqual(provenance["binary"]["mode"], "supplied")
        self.assertEqual(provenance["global_analyzer_config"]["overrides"], ["queueing.trigger_permille=451"])
        self.assertEqual(evidence.read_json(output / "plan.json")["suite"], "visible_sentinel")

    # TT-TEST: support
    def test_locked_forbidden_fields_and_duplicate_bytes_are_rejected(self):
        manifest = evidence.load_architecture_manifest()
        root = self.temp / "definitions"
        shutil.copytree(evidence.REPO / evidence.ARCHITECTURE_MANIFEST.parent, root)
        copied = evidence.read_json(root / "manifest.json")
        locked = next(item for item in copied["cases"] if item["suite"] == "locked_challenge")
        locked["expected_score"] = 1
        evidence.write_json(root / "manifest.json", copied)
        with self.assertRaisesRegex(evidence.EvidenceError, "forbidden"):
            evidence.load_architecture_manifest(root / "manifest.json")
        locked.pop("expected_score")
        sentinel = next(item for item in copied["cases"] if item["suite"] == "visible_sentinel")
        (root / locked["artifact"]).write_bytes((root / sentinel["artifact"]).read_bytes())
        locked["sha256"] = sentinel["sha256"]
        evidence.write_json(root / "manifest.json", copied)
        with self.assertRaisesRegex(evidence.EvidenceError, "duplicate locked input bytes"):
            evidence.load_architecture_manifest(root / "manifest.json")

    # TT-TEST: support
    def test_visible_assertions_do_not_require_exact_scores(self):
        for item in evidence.architecture_cases("visible_sentinel"):
            self.assertNotIn("expected_score", item)
            self.assertIn("analysis_succeeds", item["assertions"])


if __name__ == "__main__":
    unittest.main()
