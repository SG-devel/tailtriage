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
    def test_run_end_policy_variants_include_expected_kinds(self) -> None:
        kinds = validate_docs_contracts.extract_run_end_policy_kinds_from_source()
        self.assertEqual(kinds, {"continue_after_limits_hit", "auto_seal_on_limits_hit"})

    def test_markdown_examples_validate_against_contract(self) -> None:
        validate_docs_contracts.validate_readme_analyzer_example()
        validate_docs_contracts.validate_controller_readme_toml()

    def test_docs_index_contract(self) -> None:
        validate_docs_contracts.validate_docs_index_contract()

    def test_root_readme_docs_link(self) -> None:
        validate_docs_contracts.validate_root_readme_docs_link()

    def test_user_guide_contract(self) -> None:
        validate_docs_contracts.validate_user_guide_contract()

    def test_user_guide_uses_controller_scoped_toml_shape(self) -> None:
        text = validate_docs_contracts.USER_GUIDE_PATH.read_text(encoding="utf-8")
        self.assertIn("[controller]", text)
        self.assertIn("[controller.activation]", text)
        self.assertIn("[controller.activation.sink]", text)
        self.assertNotIn("\n[activation]\n", text)
        self.assertNotIn("\n[activation.sink]\n", text)

    def test_diagnostics_contract_truthfulness(self) -> None:
        validate_docs_contracts.validate_diagnostics_contract_truthfulness()

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
tailtriage-analyzer is in-process for completed Run inputs, typed Report output,
render_text formatting, serde_json parsing support, AnalyzeOptions::default(),
not streaming capture, and tailtriage-cli for command-line artifact loading.
"""
        cli_text = """
tailtriage-cli loads saved run artifacts, performs schema validation, enforces
non-empty requests loader rules, uses tailtriage-analyzer, and provides command-line
text or json output. Rust in-process users should use tailtriage-analyzer directly.
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
            analyzer_readme = repo_root / "tailtriage-analyzer" / "README.md"
            for path in (docs_index, user_guide, diagnostics, architecture, cli_readme, analyzer_readme):
                path.parent.mkdir(parents=True, exist_ok=True)
            root_readme.write_text(clean_text, encoding="utf-8")
            docs_index.write_text(clean_text, encoding="utf-8")
            user_guide.write_text(clean_text, encoding="utf-8")
            diagnostics.write_text(clean_text, encoding="utf-8")
            architecture.write_text(clean_text, encoding="utf-8")
            cli_readme.write_text(clean_text, encoding="utf-8")
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


if __name__ == "__main__":
    unittest.main()
