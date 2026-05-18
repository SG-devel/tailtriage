#!/usr/bin/env python3

from __future__ import annotations

import json
import math
import tempfile
import unittest
from pathlib import Path

import sys

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import validate_runtime_cost_summary  # noqa: E402


class ValidateRuntimeCostSummaryTests(unittest.TestCase):
    def _summary(self) -> dict:
        modes = {m: {
            "run_requests": {"median": 10},
            "run_stages": {"median": 10},
            "run_queues": {"median": 10},
            "runtime_snapshots": {"median": 0},
            "effective_tokio_sampler_config_present_rounds": 0,
            "drop_path_signal_present_rounds": 0,
        } for m in validate_runtime_cost_summary.EXPECTED_MODES}
        modes["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 2
        modes["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 1
        modes["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1
        ratios = {k: 1.1 for k in validate_runtime_cost_summary.REQUIRED_RATIO_KEYS}
        return {"absolute_metrics": modes, "tracing_vs_native_ratios": ratios}

    def _write_and_validate(self, summary: dict) -> None:
        with tempfile.TemporaryDirectory() as td:
            root = Path(td)
            raw = root / "runtime-cost-raw.jsonl"
            out = root / "runtime-cost-summary.json"
            raw.write_text('{"ok":true}\n', encoding="utf-8")
            out.write_text(json.dumps(summary), encoding="utf-8")
            validate_runtime_cost_summary.validate(raw, out)

    def test_valid_fixture_passes(self) -> None:
        self._write_and_validate(self._summary())

    def test_missing_mode_fails(self) -> None:
        summary = self._summary()
        del summary["absolute_metrics"]["tracing_light"]
        with self.assertRaises(SystemExit):
            self._write_and_validate(summary)

    def test_missing_tracing_runtime_snapshots_fails(self) -> None:
        summary = self._summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 0
        with self.assertRaises(SystemExit):
            self._write_and_validate(summary)

    def test_missing_sampler_metadata_fails(self) -> None:
        summary = self._summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 0
        with self.assertRaises(SystemExit):
            self._write_and_validate(summary)

    def test_missing_drop_path_signal_fails(self) -> None:
        summary = self._summary()
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 0
        with self.assertRaises(SystemExit):
            self._write_and_validate(summary)

    def test_nan_or_infinite_ratio_fails(self) -> None:
        summary = self._summary()
        summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_latency_p95"] = math.inf
        with self.assertRaises(SystemExit):
            self._write_and_validate(summary)

    def test_missing_required_ratio_key_fails(self) -> None:
        summary = self._summary()
        del summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_throughput"]
        with self.assertRaises(SystemExit):
            self._write_and_validate(summary)


if __name__ == "__main__":
    unittest.main()
