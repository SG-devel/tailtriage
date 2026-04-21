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

    def test_root_readme_docs_map_parity(self) -> None:
        validate_docs_contracts.validate_root_readme_docs_map_parity()

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

    def test_architecture_contract(self) -> None:
        validate_docs_contracts.validate_architecture_contract()

    def test_docs_no_history_framing(self) -> None:
        validate_docs_contracts.validate_docs_no_history_framing()

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

    def test_controller_readme_toml_validation_requires_current_anchor(self) -> None:
        readme_text = """# tailtriage-controller

## Config file (TOML)

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


if __name__ == "__main__":
    unittest.main()
