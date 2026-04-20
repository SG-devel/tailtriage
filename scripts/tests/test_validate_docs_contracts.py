#!/usr/bin/env python3
"""Tests for public-docs contract validation helpers."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import validate_docs_contracts  # noqa: E402


class ValidateDocsContractsTests(unittest.TestCase):
    def test_run_end_policy_variants_include_expected_kinds(self) -> None:
        kinds = validate_docs_contracts.extract_run_end_policy_kinds_from_source()
        self.assertEqual(
            kinds,
            {"continue_after_limits_hit", "auto_seal_on_limits_hit"},
        )

    def test_markdown_examples_validate_against_contract(self) -> None:
        validate_docs_contracts.validate_readme_analyzer_example()
        validate_docs_contracts.validate_controller_readme_toml()


if __name__ == "__main__":
    unittest.main()
