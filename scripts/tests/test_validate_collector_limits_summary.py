#!/usr/bin/env python3
"""Tests for collector-limits smoke summary structural validation."""

from __future__ import annotations

import unittest

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import validate_collector_limits_summary  # noqa: E402


class ValidateCollectorLimitsSummaryTests(unittest.TestCase):
    def test_accepts_summary_with_required_shape(self) -> None:
        summary = {
            "measurement_kind": "collector_limits",
            "profile": "smoke",
            "default_matrix": [
                {"case_id": "smoke_baseline_shape"},
                {"case_id": "smoke_sampler_dense"},
            ],
            "cases_by_mode": {},
            "collector_stress_signals": {},
            "collector_pressure_onset_markers": {},
            "measurement_quality": {},
        }

        validate_collector_limits_summary.validate_summary_shape(summary)

    def test_rejects_summary_missing_sampler_dense_case(self) -> None:
        summary = {
            "measurement_kind": "collector_limits",
            "profile": "smoke",
            "default_matrix": [{"case_id": "smoke_baseline_shape"}],
            "cases_by_mode": {},
            "collector_stress_signals": {},
            "collector_pressure_onset_markers": {},
            "measurement_quality": {},
        }

        with self.assertRaises(ValueError):
            validate_collector_limits_summary.validate_summary_shape(summary)


if __name__ == "__main__":
    unittest.main()
