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

import measure_runtime_cost  # noqa: E402
import validate_runtime_cost_summary  # noqa: E402


class ValidateRuntimeCostSummaryTests(unittest.TestCase):
    def _summary(self) -> dict:
        abs_metrics = {}
        for mode in measure_runtime_cost.MODES:
            abs_metrics[mode] = {
                "throughput_rps": {"median": 100.0},
                "latency_p95_ms": {"median": 10.0},
                "run_requests": {"median": 100.0},
                "run_stages": {"median": 200.0},
                "run_queues": {"median": 100.0},
                "runtime_snapshots": {"median": 0.0},
                "effective_tokio_sampler_config_present_rounds": 0,
                "drop_path_signal_present_rounds": 0,
            }
        abs_metrics["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 10.0
        abs_metrics["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 1
        abs_metrics["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1
        return {
            "measured_rounds": 4,
            "measurement_quality": "noisy",
            "absolute_metrics": abs_metrics,
            "tracing_vs_native_ratios": {
                "tracing_light_vs_core_light_latency_p95": 1.0,
                "tracing_light_vs_core_light_throughput": 0.97,
                "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": 1.03,
                "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": 0.97,
                "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": 1.03,
                "tracing_light_drop_path_vs_core_light_drop_path_throughput": 0.97,
            },
        }

    def _write(self, summary: dict) -> tuple[Path, Path, tempfile.TemporaryDirectory]:
        tmp = tempfile.TemporaryDirectory()
        root = Path(tmp.name)
        raw = root / "runtime-cost-raw.jsonl"
        raw.write_text('{"mode":"baseline"}\n', encoding="utf-8")
        summ = root / "runtime-cost-summary.json"
        summ.write_text(json.dumps(summary), encoding="utf-8")
        return raw, summ, tmp

    def test_valid_fixture_passes(self) -> None:
        raw, summ, tmp = self._write(self._summary())
        with tmp:
            validate_runtime_cost_summary.validate(raw, summ)

    def test_missing_mode_fails(self) -> None:
        summary = self._summary()
        del summary["absolute_metrics"]["tracing_light"]
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)

    def test_missing_tracing_runtime_snapshots_fails(self) -> None:
        summary = self._summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 0
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)

    def test_missing_sampler_metadata_fails(self) -> None:
        summary = self._summary()
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 0
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)

    def test_missing_drop_path_signal_fails(self) -> None:
        summary = self._summary()
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 0
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)

    def test_nan_or_infinite_ratio_fails(self) -> None:
        for bad in (math.nan, math.inf):
            summary = self._summary()
            summary["tracing_vs_native_ratios"]["tracing_light_vs_core_light_latency_p95"] = bad
            raw, summ, tmp = self._write(summary)
            with tmp, self.assertRaises(SystemExit):
                validate_runtime_cost_summary.validate(raw, summ)

    def test_missing_required_ratio_key_fails(self) -> None:
        summary = self._summary()
        del summary["tracing_vs_native_ratios"]["tracing_light_drop_path_vs_core_light_drop_path_throughput"]
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)

    def test_fewer_measured_rounds_fails(self) -> None:
        summary = self._summary()
        summary["measured_rounds"] = 2
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)

    def test_insufficient_data_quality_fails(self) -> None:
        summary = self._summary()
        summary["measurement_quality"] = "insufficient_data"
        raw, summ, tmp = self._write(summary)
        with tmp, self.assertRaises(SystemExit):
            validate_runtime_cost_summary.validate(raw, summ)


if __name__ == "__main__":
    unittest.main()
