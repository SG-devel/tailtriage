#!/usr/bin/env python3
"""Tests for public-docs contract validation helpers."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import validate_docs_contracts  # noqa: E402


class ValidateDocsContractsTests(unittest.TestCase):

    def test_governance_strictness_contract_accepts_distinct_policies(self) -> None:
        spec_text = """# Spec

Schema contract:

- default Run artifact analysis is compatibility-oriented and warns on some ambiguous request-scoped attribution cases instead of failing
- strict Run artifact validation is opt-in through the analyzer strict-validation APIs and `tailtriage analyze --strict-artifact`
- tracing import `--strict` separately controls malformed or incomplete `tt.*` span handling during conversion; it does not replace strict Run artifact validation
- tracing completed-span JSONL import supports the stable wrapper format and the explicitly selected compatibility parser for supported pre-stable/internal record shapes
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            spec_path = Path(tmp_dir) / "SPEC.md"
            spec_path.write_text(spec_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "SPEC_PATH", spec_path):
                validate_docs_contracts.validate_governance_strictness_contract()

    def test_governance_strictness_contract_rejects_cli_import_conflation(self) -> None:
        spec_text = """# Spec

- default Run artifact analysis is compatibility-oriented and warns on some ambiguous request-scoped attribution cases instead of failing
- strict Run artifact validation is opt-in through the analyzer strict-validation APIs and `tailtriage analyze --strict-artifact`
- tracing import `--strict` separately controls malformed or incomplete `tt.*` span handling during conversion; it does not replace strict Run artifact validation
- tracing completed-span JSONL import supports the stable wrapper format and the explicitly selected compatibility parser for supported pre-stable/internal record shapes
- strict artifact validation is currently opt-in through strict analyzer validation APIs or CLI/import strict flags
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            spec_path = Path(tmp_dir) / "SPEC.md"
            spec_path.write_text(spec_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "SPEC_PATH", spec_path):
                with self.assertRaisesRegex(ValueError, r"conflates strict Run artifact validation"):
                    validate_docs_contracts.validate_governance_strictness_contract()

    def test_governance_pending_state_contract_accepts_unsealed_shutdown_wording(self) -> None:
        design_text = """# Notes

Not all live bookkeeping is bounded by capture limits today. Pending/unfinished request state can grow with admitted requests and remains until the corresponding request completion token finishes or the collector is dropped.

`shutdown()` currently inspects pending requests and records unfinished-request metadata, but it does not clear pending bookkeeping or seal the collector against later admissions or completion activity.

Pending-state tracking preserves lifecycle warnings but remains separate from the retained request, queue, stage, in-flight, and runtime vectors that capture limits bound.

Pending-state limits and unsealed shutdown behavior remain known current limitations rather than desired permanent contracts.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            design_path = Path(tmp_dir) / "DESIGN_NOTES.md"
            design_path.write_text(design_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "DESIGN_NOTES_PATH", design_path):
                validate_docs_contracts.validate_governance_pending_state_contract()

    def test_governance_pending_state_contract_rejects_shutdown_as_boundary(self) -> None:
        design_text = """# Notes

Not all live bookkeeping is bounded by capture limits today. Pending/unfinished request state can grow with admitted requests until those requests complete or the run shuts down.
Pending/unfinished request state can grow with admitted requests and remains until the corresponding request completion token finishes or the collector is dropped.
`shutdown()` currently inspects pending requests and records unfinished-request metadata, but it does not clear pending bookkeeping or seal the collector against later admissions or completion activity.
Pending-state tracking preserves lifecycle warnings but remains separate from the retained request, queue, stage, in-flight, and runtime vectors that capture limits bound.
Pending-state limits and unsealed shutdown behavior remain known current limitations rather than desired permanent contracts.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            design_path = Path(tmp_dir) / "DESIGN_NOTES.md"
            design_path.write_text(design_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "DESIGN_NOTES_PATH", design_path):
                with self.assertRaisesRegex(ValueError, r"shutdown clears pending request state"):
                    validate_docs_contracts.validate_governance_pending_state_contract()

    def test_run_end_policy_variants_include_expected_kinds(self) -> None:
        kinds = validate_docs_contracts.extract_run_end_policy_kinds_from_source()
        self.assertEqual(kinds, {"continue_after_limits_hit", "auto_seal_on_limits_hit"})


    def test_crate_rustdocs_include_readmes_contract(self) -> None:
        validate_docs_contracts.validate_crate_rustdocs_include_readmes()

    def test_crate_rustdocs_include_readmes_contract_fails_when_missing_include(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            rels = (
                "tailtriage/src/lib.rs",
                "tailtriage-core/src/lib.rs",
                "tailtriage-controller/src/lib.rs",
                "tailtriage-tokio/src/lib.rs",
                "tailtriage-axum/src/lib.rs",
                "tailtriage-analyzer/src/lib.rs",
                "tailtriage-cli/src/lib.rs",
                "tailtriage-tracing/src/lib.rs",
            )
            paths = []
            for rel in rels:
                path = repo_root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('#![doc = include_str!("../README.md")]\n', encoding="utf-8")
                paths.append(path)

            (repo_root / rels[0]).write_text('// missing include\n', encoding="utf-8")

            with (
                mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root),
                mock.patch.object(
                    validate_docs_contracts,
                    "RUSTDOC_INCLUDE_CRATE_LIBS",
                    tuple(paths),
                ),
            ):
                with self.assertRaisesRegex(ValueError, r"README directive"):
                    validate_docs_contracts.validate_crate_rustdocs_include_readmes()

    def test_markdown_examples_validate_against_contract(self) -> None:
        validate_docs_contracts.validate_readme_analyzer_example()
        validate_docs_contracts.validate_controller_readme_toml()

    def test_docs_index_contract(self) -> None:
        validate_docs_contracts.validate_docs_index_contract()

    def test_root_readme_docs_link(self) -> None:
        validate_docs_contracts.validate_root_readme_docs_link()

    def test_user_guide_contract(self) -> None:
        validate_docs_contracts.validate_user_guide_contract()

    def test_tracing_completed_jsonl_public_contract(self) -> None:
        validate_docs_contracts.validate_tracing_completed_jsonl_public_contract()

    def test_live_tracing_session_public_contract(self) -> None:
        validate_docs_contracts.validate_live_tracing_session_public_contract()

    def test_operations_guide_contract(self) -> None:
        validate_docs_contracts.validate_operations_guide_contract()

    def test_operations_guide_contract_fails_when_required_concepts_missing(self) -> None:
        incomplete = """# Production operations guide

Minimal text that references VALIDATION.md, diagnostics.md, runtime-cost.md, and collector-limits.md,
but intentionally omits required operational concepts.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            operations_path = Path(tmp_dir) / "operations.md"
            operations_path.write_text(incomplete, encoding="utf-8")
            with mock.patch.object(validate_docs_contracts, "OPERATIONS_PATH", operations_path):
                with self.assertRaisesRegex(ValueError, r"missing required concept/token"):
                    validate_docs_contracts.validate_operations_guide_contract()

    def test_operations_guide_contract_passes_with_complete_content(self) -> None:
        complete = """# Production operations guide

This is a production operations guide with a recommended rollout path.
Use light first, escalate to investigation when needed.
Runtime sampling may help.
Artifact sizing and truncation behavior depend on capture limits.
If results are insufficient_evidence, inspect evidence_quality and controller choices.
These suspects are not proof of root cause and not universal production guarantees.

See VALIDATION.md, diagnostics.md, runtime-cost.md, and collector-limits.md.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            operations_path = Path(tmp_dir) / "operations.md"
            operations_path.write_text(complete, encoding="utf-8")
            with mock.patch.object(validate_docs_contracts, "OPERATIONS_PATH", operations_path):
                validate_docs_contracts.validate_operations_guide_contract()

    def test_user_guide_uses_controller_scoped_toml_shape(self) -> None:
        text = validate_docs_contracts.USER_GUIDE_PATH.read_text(encoding="utf-8")
        self.assertIn("[controller]", text)
        self.assertIn("[controller.activation]", text)
        self.assertIn("[controller.activation.sink]", text)
        self.assertNotIn("\n[activation]\n", text)
        self.assertNotIn("\n[activation.sink]\n", text)

    def test_diagnostics_contract_truthfulness(self) -> None:
        validate_docs_contracts.validate_diagnostics_contract_truthfulness()

    def test_analyzer_config_example_contract(self) -> None:
        validate_docs_contracts.validate_analyzer_config_example_contract()

    def test_analyzer_config_example_contract_rejects_missing_schema_version(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / "analyzer-config.toml"
            path.write_text("[analyzer]\n[analyzer.queueing]\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, r"schema_version = 1"):
                validate_docs_contracts.validate_analyzer_config_example_contract(config_path=path)

    def test_analyzer_config_example_contract_rejects_missing_analyzer_table(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / "analyzer-config.toml"
            path.write_text("schema_version = 1\n", encoding="utf-8")
            with self.assertRaisesRegex(ValueError, r"\[analyzer\]"):
                validate_docs_contracts.validate_analyzer_config_example_contract(config_path=path)

    def test_analyzer_config_example_contract_rejects_missing_group(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / "analyzer-config.toml"
            body = "[analyzer]\nschema_version = 1\n" + "\n".join(
                f"[analyzer.{g}]" for g in validate_docs_contracts.ANALYZER_GROUPS if g != "temporal"
            )
            path.write_text(body, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, r"temporal"):
                validate_docs_contracts.validate_analyzer_config_example_contract(config_path=path)

    def test_analyzer_config_example_contract_rejects_root_level_group(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / "analyzer-config.toml"
            body = "[analyzer]\nschema_version = 1\n" + "\n".join(
                f"[analyzer.{g}]" for g in validate_docs_contracts.ANALYZER_GROUPS
            ) + "\n[queueing]\ntrigger_permille = 100\n"
            path.write_text(body, encoding="utf-8")
            with self.assertRaisesRegex(ValueError, r"root-level analyzer groups"):
                validate_docs_contracts.validate_analyzer_config_example_contract(config_path=path)

    def test_extract_analyzer_paths_for_validation(self) -> None:
        text = """
Use `queueing.trigger_permille`, queueing.trigger_permille=400,
--analyzer-set queueing.trigger_permille=400, and `confidence.high_score_threshold`.
Ignore file names like docs/operations.md, foo.bar, and include queuing.trigger_permille too.
"""
        paths = validate_docs_contracts._extract_analyzer_paths_for_validation(text)
        self.assertIn("queueing.trigger_permille", paths)
        self.assertIn("confidence.high_score_threshold", paths)
        self.assertIn("queuing.trigger_permille", paths)
        self.assertNotIn("foo.bar", paths)
        self.assertNotIn("docs/operations.md", paths)

    def test_analyzer_tuning_docs_contracts_on_committed_docs(self) -> None:
        validate_docs_contracts.validate_analyzer_tuning_tokens_contract()
        validate_docs_contracts.validate_no_root_level_analyzer_toml_in_docs()
        validate_docs_contracts.validate_analyzer_override_paths_contract()

    def test_analyzer_no_root_level_docs_rejects_root_level_table_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            docs_dir = repo_root / "docs"
            docs_dir.mkdir(parents=True, exist_ok=True)
            path = docs_dir / "diagnostics.md"
            path.write_text("[queueing]\ntrigger_permille = 400\n", encoding="utf-8")
            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"invalid root-level TOML header"):
                    validate_docs_contracts.validate_no_root_level_analyzer_toml_in_docs(doc_paths=(path,))

    def test_analyzer_no_root_level_docs_allows_namespaced_table_header(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / "docs.md"
            path.write_text("[analyzer.queueing]\ntrigger_permille = 400\n", encoding="utf-8")
            validate_docs_contracts.validate_no_root_level_analyzer_toml_in_docs(doc_paths=(path,))

    def test_analyzer_override_paths_contract_rejects_invalid_paths(self) -> None:
        invalid_candidates = (
            "confidence.high_threshold",
            "route.max_routes",
            "temporal.windows",
            "queuing.trigger_permille",
        )
        for candidate in invalid_candidates:
            with self.subTest(candidate=candidate):
                with tempfile.TemporaryDirectory() as tmp_dir:
                    repo_root = Path(tmp_dir)
                    doc = repo_root / "docs.md"
                    doc.write_text(f"`{candidate}`\n", encoding="utf-8")
                    with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                        with self.assertRaisesRegex(ValueError, candidate):
                            validate_docs_contracts.validate_analyzer_override_paths_contract(
                                doc_paths=(doc,)
                            )

    def test_validation_ci_contract_checks_committed_workflow_and_docs(self) -> None:
        validate_docs_contracts.validate_diagnostic_benchmark_ci_contract()
        validate_docs_contracts.validate_validation_docs_ci_contract()

    def test_validation_ci_contract_fails_without_diagnostic_benchmark_command(self) -> None:
        workflow_text = """name: CI

jobs:
  verify:
    steps:
      - name: Checkout
        uses: actions/checkout@v4
      - name: Validate diagnostic benchmark helper unit tests
        run: python3 -m unittest scripts.tests.test_diagnostic_benchmark
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflow_path = Path(tmp_dir) / "ci.yml"
            workflow_path.write_text(workflow_text, encoding="utf-8")

            with self.assertRaisesRegex(ValueError, r"diagnostic_benchmark.py"):
                validate_docs_contracts.validate_diagnostic_benchmark_ci_contract(
                    workflow_path=workflow_path
                )

    def test_validation_ci_contract_fails_without_required_benchmark_args(self) -> None:
        workflow_text = """name: CI

jobs:
  verify:
    steps:
      - name: Validate deterministic diagnostics benchmark corpus
        run: >
          python3 scripts/diagnostic_benchmark.py
          --manifest validation/diagnostics/manifest.json
          --min-top1 0.75
          --max-high-confidence-wrong 0
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflow_path = Path(tmp_dir) / "ci.yml"
            workflow_path.write_text(workflow_text, encoding="utf-8")

            with self.assertRaisesRegex(ValueError, r"--min-top2 0.90"):
                validate_docs_contracts.validate_diagnostic_benchmark_ci_contract(
                    workflow_path=workflow_path
                )

    def test_validation_ci_contract_fails_when_benchmark_step_can_continue_on_error(self) -> None:
        workflow_text = """name: CI

jobs:
  verify:
    steps:
      - name: Validate deterministic diagnostics benchmark corpus
        continue-on-error: true
        run: |
          python3 scripts/diagnostic_benchmark.py \
            --manifest validation/diagnostics/manifest.json \
            --min-top1 0.75 \
            --min-top2 0.90 \
            --max-high-confidence-wrong 0
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflow_path = Path(tmp_dir) / "ci.yml"
            workflow_path.write_text(workflow_text, encoding="utf-8")

            with self.assertRaisesRegex(ValueError, r"continue-on-error"):
                validate_docs_contracts.validate_diagnostic_benchmark_ci_contract(
                    workflow_path=workflow_path
                )

    def test_validation_docs_contract_fails_on_stale_normal_pr_ci_wording(self) -> None:
        doc_text = """# Validation

Deterministic corpus: no in normal PR CI.
Durable scorecards come from `.github/workflows/validation-snapshot.yml`.
Normal CI does not publish durable diagnostic scorecards.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            doc_path = Path(tmp_dir) / "VALIDATION.md"
            doc_path.write_text(doc_text, encoding="utf-8")

            with self.assertRaisesRegex(ValueError, r"no in normal pr ci"):
                validate_docs_contracts.validate_validation_docs_ci_contract(
                    doc_paths=(doc_path,)
                )

    def test_validation_docs_contract_requires_snapshot_workflow_scorecard_source(self) -> None:
        doc_text = """# Validation

Durable scorecards come from normal CI artifacts.
Normal CI does not publish durable diagnostic scorecards.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            doc_path = Path(tmp_dir) / "VALIDATION.md"
            doc_path.write_text(doc_text, encoding="utf-8")

            with self.assertRaisesRegex(ValueError, r"validation-snapshot.yml"):
                validate_docs_contracts.validate_validation_docs_ci_contract(
                    doc_paths=(doc_path,)
                )


    def test_cli_not_presented_as_library_analyzer_api_contract(self) -> None:
        validate_docs_contracts.validate_cli_not_presented_as_library_analyzer_api()

    def test_analyzer_cli_docs_split_contract(self) -> None:
        validate_docs_contracts.validate_analyzer_cli_docs_split_contract()

    def test_capture_readmes_analyzer_cli_wording_contract(self) -> None:
        validate_docs_contracts.validate_capture_readmes_analyzer_cli_wording_contract()

    def test_capture_readme_wording_rejects_cli_only_stale_phrase(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            paths = []
            for rel in (
                "tailtriage/README.md",
                "tailtriage-core/README.md",
                "tailtriage-controller/README.md",
                "tailtriage-tokio/README.md",
                "tailtriage-axum/README.md",
            ):
                path = repo_root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(
                    "Use tailtriage-analyzer in-process and tailtriage-cli for saved artifacts.",
                    encoding="utf-8",
                )
                paths.append(path)

            paths[0].write_text(
                "Analysis is still done by `tailtriage-cli` for this crate.\n"
                "Also references tailtriage-analyzer and tailtriage-cli.",
                encoding="utf-8",
            )

            with (
                mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root),
                mock.patch.object(
                    validate_docs_contracts,
                    "CAPTURE_INTEGRATION_README_PATHS",
                    tuple(paths),
                ),
            ):
                with self.assertRaisesRegex(ValueError, r"stale CLI-only analyzer wording"):
                    validate_docs_contracts.validate_capture_readmes_analyzer_cli_wording_contract()

    def test_capture_readme_requires_analyzer_and_cli_mentions(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            paths = []
            for rel in (
                "tailtriage/README.md",
                "tailtriage-core/README.md",
                "tailtriage-controller/README.md",
                "tailtriage-tokio/README.md",
                "tailtriage-axum/README.md",
            ):
                path = repo_root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(
                    "Use tailtriage-analyzer in-process and tailtriage-cli for saved artifacts.",
                    encoding="utf-8",
                )
                paths.append(path)
            paths[1].write_text("Use tailtriage-cli for saved artifacts only.", encoding="utf-8")

            with (
                mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root),
                mock.patch.object(
                    validate_docs_contracts,
                    "CAPTURE_INTEGRATION_README_PATHS",
                    tuple(paths),
                ),
            ):
                with self.assertRaisesRegex(ValueError, r"must mention tailtriage-analyzer"):
                    validate_docs_contracts.validate_capture_readmes_analyzer_cli_wording_contract()

    def test_cli_readme_positive_when_cli_invokes_analyzer(self) -> None:
        analyzer_text = """
tailtriage-analyzer is in-process analysis for completed Run values and returns a typed Report.
Use analyze_run(run, AnalyzeOptions::default()) for the standard entry point.
Use render_text(&report), render_json(&report), and render_json_pretty(&report) for Report rendering.
Use analyze_run_json(run, AnalyzeOptions::default()) and analyze_run_json_pretty(run, AnalyzeOptions::default()) for helpers.
This crate is not streaming / not live streaming, and tailtriage-cli owns artifact loading.
## How to interpret a report
primary_suspect secondary_suspects evidence[] next_checks[] score confidence evidence_quality route_breakdowns temporal_segments Report JSON Run artifact JSON
"""
        cli_text = """
tailtriage-cli loads saved run artifacts from disk, performs schema validation,
enforces a non-empty requests loader rule, and uses tailtriage-analyzer.
It provides command-line text/json output and emits Report JSON as output.
Rust in-process users should use tailtriage-analyzer directly.
Run artifact JSON is CLI input; Report JSON is CLI/analyzer output.
CLI does not consume Report JSON as input.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            cli_readme = repo_root / "tailtriage-cli" / "README.md"
            analyzer_readme.parent.mkdir(parents=True, exist_ok=True)
            cli_readme.parent.mkdir(parents=True, exist_ok=True)
            analyzer_readme.write_text(analyzer_text, encoding="utf-8")
            cli_readme.write_text(cli_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                validate_docs_contracts.validate_analyzer_cli_docs_split_contract()

    def test_analyzer_readme_validation_fails_when_json_renderer_tokens_missing(self) -> None:
        analyzer_text = """
tailtriage-analyzer is in-process analysis for completed Run values with typed Report output.
Use analyze_run(run, AnalyzeOptions::default()) and render_text(&report).
Use analyze_run_json(run, AnalyzeOptions::default()) and analyze_run_json_pretty(run, AnalyzeOptions::default()).
This crate is not streaming and references tailtriage-cli for artifact loading.
"""
        cli_text = """
tailtriage-cli loads saved run artifacts from disk, performs schema validation,
enforces non-empty requests loader rules, uses tailtriage-analyzer, and provides command-line text/json output.
Rust in-process users should use tailtriage-analyzer.
Run artifact JSON is input; Report JSON is output; CLI does not consume Report JSON as input.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            cli_readme = repo_root / "tailtriage-cli" / "README.md"
            analyzer_readme.parent.mkdir(parents=True, exist_ok=True)
            cli_readme.parent.mkdir(parents=True, exist_ok=True)
            analyzer_readme.write_text(analyzer_text, encoding="utf-8")
            cli_readme.write_text(cli_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"render_json"):
                    validate_docs_contracts.validate_analyzer_cli_docs_split_contract()

    def test_cli_readme_validation_fails_without_report_vs_run_artifact_distinction(self) -> None:
        analyzer_text = """
tailtriage-analyzer is in-process analysis for completed Run values and returns a typed Report.
Use analyze_run(run, AnalyzeOptions::default()) and render_text(&report).
Use render_json(&report), render_json_pretty(&report), analyze_run_json(run, AnalyzeOptions::default()),
and analyze_run_json_pretty(run, AnalyzeOptions::default()).
This crate is not streaming / not live streaming and references tailtriage-cli.
## How to interpret a report
primary_suspect secondary_suspects evidence[] next_checks[] score confidence evidence_quality route_breakdowns temporal_segments Report JSON Run artifact JSON
"""
        cli_text = """
tailtriage-cli loads saved run artifacts from disk, performs schema validation,
enforces non-empty requests loader rules, uses tailtriage-analyzer, and provides command-line text/json output.
Rust in-process users should use tailtriage-analyzer.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            cli_readme = repo_root / "tailtriage-cli" / "README.md"
            analyzer_readme.parent.mkdir(parents=True, exist_ok=True)
            cli_readme.parent.mkdir(parents=True, exist_ok=True)
            analyzer_readme.write_text(analyzer_text, encoding="utf-8")
            cli_readme.write_text(cli_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"report vs run artifact json distinction"):
                    validate_docs_contracts.validate_analyzer_cli_docs_split_contract()


    def test_analyzer_readme_contract_fails_when_repo_relative_docs_link_present(self) -> None:
        analyzer_text = """
tailtriage-analyzer is in-process analysis for completed Run values and returns a typed Report.
Use analyze_run(run, AnalyzeOptions::default()) for the standard entry point.
Use render_text(&report), render_json(&report), and render_json_pretty(&report) for Report rendering.
Use analyze_run_json(run, AnalyzeOptions::default()) and analyze_run_json_pretty(run, AnalyzeOptions::default()).
This crate is not streaming and references tailtriage-cli.
## How to interpret a report
primary_suspect secondary_suspects evidence[] next_checks[] score confidence evidence_quality route_breakdowns temporal_segments Report JSON Run artifact JSON
See ../docs/diagnostics.md
"""
        cli_text = """
tailtriage-cli loads saved run artifacts from disk, performs schema validation,
enforces non-empty requests loader rules, uses tailtriage-analyzer, and provides command-line text/json output.
Rust in-process users should use tailtriage-analyzer.
Run artifact JSON is input; Report JSON is output; CLI does not consume Report JSON as input.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            cli_readme = repo_root / "tailtriage-cli" / "README.md"
            analyzer_readme.parent.mkdir(parents=True, exist_ok=True)
            cli_readme.parent.mkdir(parents=True, exist_ok=True)
            analyzer_readme.write_text(analyzer_text, encoding="utf-8")
            cli_readme.write_text(cli_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"must not link to ../docs/"):
                    validate_docs_contracts.validate_analyzer_cli_docs_split_contract()

    def test_analyzer_readme_contract_fails_when_interpret_heading_missing(self) -> None:
        analyzer_text = """
tailtriage-analyzer is in-process analysis for completed Run values and returns a typed Report.
Use analyze_run(run, AnalyzeOptions::default()) for the standard entry point.
Use render_text(&report), render_json(&report), and render_json_pretty(&report) for Report rendering.
Use analyze_run_json(run, AnalyzeOptions::default()) and analyze_run_json_pretty(run, AnalyzeOptions::default()).
This crate is not streaming and references tailtriage-cli.
primary_suspect secondary_suspects evidence[] next_checks[] score confidence evidence_quality route_breakdowns temporal_segments Report JSON Run artifact JSON
"""
        cli_text = """
tailtriage-cli loads saved run artifacts from disk, performs schema validation,
enforces non-empty requests loader rules, uses tailtriage-analyzer, and provides command-line text/json output.
Rust in-process users should use tailtriage-analyzer.
Run artifact JSON is input; Report JSON is output; CLI does not consume Report JSON as input.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            cli_readme = repo_root / "tailtriage-cli" / "README.md"
            analyzer_readme.parent.mkdir(parents=True, exist_ok=True)
            cli_readme.parent.mkdir(parents=True, exist_ok=True)
            analyzer_readme.write_text(analyzer_text, encoding="utf-8")
            cli_readme.write_text(cli_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"How to interpret a report"):
                    validate_docs_contracts.validate_analyzer_cli_docs_split_contract()

    def test_architecture_contract(self) -> None:
        validate_docs_contracts.validate_architecture_contract()

    def test_docs_no_history_framing(self) -> None:
        validate_docs_contracts.validate_docs_no_history_framing()

    def test_user_facing_wording_has_no_facade_term(self) -> None:
        validate_docs_contracts.validate_no_user_facing_facade_wording()

    def test_user_facing_wording_validation_fails_when_facade_present(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            temp_path = Path(tmp_dir) / "README.md"
            temp_path.write_text("start with the facade crate", encoding="utf-8")

            with mock.patch.object(
                validate_docs_contracts,
                "USER_FACING_TERMINOLOGY_PATHS",
                (temp_path,),
            ):
                with self.assertRaisesRegex(ValueError, r"stale facade wording"):
                    validate_docs_contracts.validate_no_user_facing_facade_wording()


    def test_cli_library_analyzer_api_contract_fails_on_banned_token(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            temp_path = Path(tmp_dir) / "README.md"
            temp_path.write_text("use tailtriage_cli::analyze::{analyze_run, render_text};", encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "README_PATH", temp_path):
                with self.assertRaisesRegex(ValueError, r"tailtriage_cli::analyze"):
                    validate_docs_contracts.validate_cli_not_presented_as_library_analyzer_api()

    def test_analyzer_readme_migration_note_allows_old_token_only_in_migration_block(self) -> None:
        readme_text = """# tailtriage-analyzer

## Migration note

```rust
use tailtriage_cli::analyze::{analyze_run, render_text};
```
"""
        stripped = validate_docs_contracts._strip_allowed_analyzer_migration_note(readme_text)
        self.assertNotIn("tailtriage_cli::analyze", stripped)

    def test_analyzer_readme_contract_fails_on_old_token_outside_migration_note(self) -> None:
        analyzer_text = """# tailtriage-analyzer

Use `tailtriage_cli::analyze` in this section.

## Migration note

```rust
use tailtriage_cli::analyze::{analyze_run, render_text};
```
"""
        clean_text = "# tailtriage docs"
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            root_readme = repo_root / "README.md"
            docs_index = repo_root / "docs" / "README.md"
            user_guide = repo_root / "docs" / "user-guide.md"
            diagnostics = repo_root / "docs" / "diagnostics.md"
            architecture = repo_root / "docs" / "architecture.md"
            cli_readme = repo_root / "tailtriage-cli" / "README.md"
            tracing_readme = repo_root / "tailtriage-tracing" / "README.md"
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            for path in (docs_index, user_guide, diagnostics, architecture, cli_readme, tracing_readme, analyzer_readme):
                path.parent.mkdir(parents=True, exist_ok=True)
            root_readme.write_text(clean_text, encoding="utf-8")
            docs_index.write_text(clean_text, encoding="utf-8")
            user_guide.write_text(clean_text, encoding="utf-8")
            diagnostics.write_text(clean_text, encoding="utf-8")
            architecture.write_text(clean_text, encoding="utf-8")
            cli_readme.write_text(clean_text, encoding="utf-8")
            tracing_readme.write_text(clean_text, encoding="utf-8")
            analyzer_readme.write_text(analyzer_text, encoding="utf-8")

            with (
                mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root),
                mock.patch.object(validate_docs_contracts, "README_PATH", root_readme),
                mock.patch.object(validate_docs_contracts, "DOCS_INDEX_PATH", docs_index),
                mock.patch.object(validate_docs_contracts, "USER_GUIDE_PATH", user_guide),
                mock.patch.object(validate_docs_contracts, "DIAGNOSTICS_PATH", diagnostics),
                mock.patch.object(validate_docs_contracts, "ARCHITECTURE_PATH", architecture),
            ):
                with self.assertRaisesRegex(ValueError, r"tailtriage_cli::analyze"):
                    validate_docs_contracts.validate_cli_not_presented_as_library_analyzer_api()

    def test_controller_readme_does_not_use_misleading_dependency_example_flow(self) -> None:
        readme_text = validate_docs_contracts.CONTROLLER_README_PATH.read_text(encoding="utf-8")
        self.assertFalse(validate_docs_contracts.is_misleading_controller_example_flow(readme_text))

    def test_sampler_forge_method_detector_flags_public_methods(self) -> None:
        source = """
impl Tailtriage {
    pub fn register_tokio_runtime_sampler(&self) {}
    pub fn runtime_sampler_stats(&self) {}
    pub(crate) fn register_tokio_runtime_sampler_internal(&self) {}
}
"""
        self.assertEqual(
            validate_docs_contracts.find_public_sampler_forge_methods(source),
            ["register_tokio_runtime_sampler", "runtime_sampler_stats"],
        )

    def test_sampler_integration_boundary_contract_validates(self) -> None:
        validate_docs_contracts.validate_sampler_integration_boundary()

    def test_controller_readme_toml_validation_allows_equivalent_headings(self) -> None:
        readme_text = """# tailtriage-controller

## Config file (TOML)

With TOML config loaded, service_name and initially_enabled fall back to builder values when omitted.
Activation template settings come from TOML when config is loaded.
Omitted optional activation subfields use TOML contract defaults.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "investigation"
strict_lifecycle = true

[controller.activation.capture_limits_override]
max_requests = 100
max_stages = 200
max_queues = 200
max_inflight_snapshots = 200
max_runtime_snapshots = 100

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 50

[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
```

## TOML field reference

service_name initially_enabled mode strict_lifecycle capture_limits_override max_requests max_stages max_queues max_inflight_snapshots max_runtime_snapshots enabled_for_armed_runs mode_override interval_ms run_end_policy continue_after_limits_hit auto_seal_on_limits_hit
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme_path = Path(tmp_dir) / "README.md"
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "CONTROLLER_README_PATH", readme_path):
                validate_docs_contracts.validate_controller_readme_toml()

    def test_controller_readme_toml_validation_fails_without_field_reference_section(self) -> None:
        readme_text = """# tailtriage-controller

## Config file (TOML)

With TOML config loaded, service_name and initially_enabled fall back to builder values when omitted.
Activation template settings come from TOML when config is loaded.
Omitted optional activation subfields use TOML contract defaults.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "light"

[controller.activation.capture_limits_override]
max_requests = 100

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "light"
interval_ms = 250
max_runtime_snapshots = 50

[controller.activation.run_end_policy]
kind = "continue_after_limits_hit"
```
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme_path = Path(tmp_dir) / "README.md"
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "CONTROLLER_README_PATH", readme_path):
                with self.assertRaisesRegex(
                    ValueError, r"TOML field reference"
                ):
                    validate_docs_contracts.validate_controller_readme_toml()

    def test_controller_readme_toml_validation_fails_when_important_tokens_missing(self) -> None:
        readme_text = """# tailtriage-controller

## Config file (TOML)

With TOML config loaded, service_name and initially_enabled fall back to builder values when omitted.
Activation template settings come from TOML when config is loaded.
Omitted optional activation subfields use TOML contract defaults.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "investigation"

[controller.activation.capture_limits_override]
max_requests = 100
max_stages = 200
max_queues = 200
max_inflight_snapshots = 200
max_runtime_snapshots = 100

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 50

[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
```

## TOML field reference

service_name initially_enabled mode strict_lifecycle capture_limits_override
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme_path = Path(tmp_dir) / "README.md"
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "CONTROLLER_README_PATH", readme_path):
                with self.assertRaisesRegex(
                    ValueError, r"missing token"
                ):
                    validate_docs_contracts.validate_controller_readme_toml()

    def test_controller_readme_toml_validation_fails_when_expanded_example_missing_sections(self) -> None:
        readme_text = """# tailtriage-controller

## Config file (TOML)

With TOML config loaded, service_name and initially_enabled fall back to builder values when omitted.
Activation template settings come from TOML when config is loaded.
Omitted optional activation subfields use TOML contract defaults.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "investigation"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

## TOML field reference

service_name initially_enabled mode strict_lifecycle capture_limits_override max_requests max_stages max_queues max_inflight_snapshots max_runtime_snapshots enabled_for_armed_runs mode_override interval_ms run_end_policy continue_after_limits_hit auto_seal_on_limits_hit
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme_path = Path(tmp_dir) / "README.md"
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "CONTROLLER_README_PATH", readme_path):
                with self.assertRaisesRegex(
                    ValueError, r"capture_limits_override"
                ):
                    validate_docs_contracts.validate_controller_readme_toml()

    def test_controller_readme_toml_validation_accepts_semantic_precedence_wording(self) -> None:
        readme_text = """# tailtriage-controller

## Config behavior

If omitted in TOML, `service_name` uses the builder value.
If omitted in TOML, `initially_enabled` uses the builder value.
Activation template settings are TOML-owned.
Any omitted optional activation fields use contract defaults.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "investigation"

[controller.activation.capture_limits_override]
max_requests = 100
max_stages = 200
max_queues = 200
max_inflight_snapshots = 200
max_runtime_snapshots = 100

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 50

[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
```

## TOML field reference

service_name initially_enabled mode strict_lifecycle capture_limits_override max_requests max_stages max_queues max_inflight_snapshots max_runtime_snapshots enabled_for_armed_runs mode_override interval_ms run_end_policy continue_after_limits_hit auto_seal_on_limits_hit
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme_path = Path(tmp_dir) / "README.md"
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "CONTROLLER_README_PATH", readme_path):
                validate_docs_contracts.validate_controller_readme_toml()

    def test_controller_readme_toml_validation_fails_without_precedence_semantics(self) -> None:
        readme_text = """# tailtriage-controller

## Config behavior

TOML controls config for this crate.

```toml
[controller]
service_name = "checkout-service"

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
```

```toml
[controller]
service_name = "checkout-service"
initially_enabled = false

[controller.activation]
mode = "investigation"

[controller.activation.capture_limits_override]
max_requests = 100
max_stages = 200
max_queues = 200
max_inflight_snapshots = 200
max_runtime_snapshots = 100

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"

[controller.activation.runtime_sampler]
enabled_for_armed_runs = true
mode_override = "investigation"
interval_ms = 250
max_runtime_snapshots = 50

[controller.activation.run_end_policy]
kind = "auto_seal_on_limits_hit"
```

## TOML field reference

service_name initially_enabled mode strict_lifecycle capture_limits_override max_requests max_stages max_queues max_inflight_snapshots max_runtime_snapshots enabled_for_armed_runs mode_override interval_ms run_end_policy continue_after_limits_hit auto_seal_on_limits_hit
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme_path = Path(tmp_dir) / "README.md"
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "CONTROLLER_README_PATH", readme_path):
                with self.assertRaisesRegex(ValueError, r"precedence guidance missing semantic rule"):
                    validate_docs_contracts.validate_controller_readme_toml()

    def test_validate_docs_index_contract_checks_paths_not_link_labels(self) -> None:
        docs_index = """# Documentation index

- [Guide](user-guide.md)
- [Diag](diagnostics.md)
- [Controller crate](../tailtriage-controller/README.md)
- [Sampler crate](../tailtriage-tokio/README.md)
"""

        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)

            # Files that should be required by repo_markdown_files().
            for rel in (
                "docs/README.md",
                "docs/user-guide.md",
                "docs/diagnostics.md",
                "tailtriage-controller/README.md",
                "tailtriage-tokio/README.md",
            ):
                path = repo_root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text(f"# {rel}\n", encoding="utf-8")

            docs_index_path = repo_root / "docs" / "README.md"
            docs_index_path.write_text(docs_index, encoding="utf-8")

            with (
                mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root),
                mock.patch.object(validate_docs_contracts, "DOCS_INDEX_PATH", docs_index_path),
                mock.patch.object(
                    validate_docs_contracts,
                    "DOCS_INDEX_EXCLUDED_MARKDOWN",
                    {"docs/README.md"},
                ),
            ):
                validate_docs_contracts.validate_docs_index_contract()


    def test_tracing_readme_migration_section_contract_rejects_duplicate_sentence(self) -> None:
        readme_text = """# README

For both `TracingSession` and `TracingSession`, use the builder.

## Live tracing session migration

Use `TracingSession` as the sole current live entry point for capture-to-Run workflows.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            readme_path = repo_root / "tailtriage-tracing" / "README.md"
            readme_path.parent.mkdir(parents=True)
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"duplicated TracingSession"):
                    validate_docs_contracts.validate_tracing_readme_migration_section_contract()

    def test_tracing_readme_migration_section_contract_requires_one_heading(self) -> None:
        readme_text = """# README

## Live tracing session migration

Use `TracingSession` as the sole current live entry point.

## Live tracing session migration

Duplicate.
"""
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            readme_path = repo_root / "tailtriage-tracing" / "README.md"
            readme_path.parent.mkdir(parents=True)
            readme_path.write_text(readme_text, encoding="utf-8")

            with mock.patch.object(validate_docs_contracts, "REPO_ROOT", repo_root):
                with self.assertRaisesRegex(ValueError, r"exactly one"):
                    validate_docs_contracts.validate_tracing_readme_migration_section_contract()

if __name__ == "__main__":
    unittest.main()
