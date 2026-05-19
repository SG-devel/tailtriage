#!/usr/bin/env python3
"""Smoke coverage for runtime-cost summary schema and attribution sections."""

from __future__ import annotations

import json
import tempfile
import unittest
from pathlib import Path

import sys

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import measure_runtime_cost  # noqa: E402


class RuntimeCostSummaryTests(unittest.TestCase):
    def _base_row(self, mode: str, round_idx: int, latency_p95_ms: float = 2.0, throughput_rps: float = 1000.0) -> dict:
        row = {
            "mode": mode,
            "requests": 100,
            "concurrency": 10,
            "work_ms": 1,
            "throughput_rps": throughput_rps,
            "latency_p50_ms": 1.0,
            "latency_p95_ms": latency_p95_ms,
            "latency_p99_ms": 3.0,
            "artifact_finalize_ms": 0.5,
            "analyze_ms": 0.5,
            "report_render_ms": 0.5,
            "run_requests": 100,
            "run_stages": 100,
            "run_queues": 100,
            "runtime_snapshots": 10 if mode == "tracing_light_tokio_sampler" else 0,
            "effective_tokio_sampler_config_present": mode == "tracing_light_tokio_sampler",
            "inflight_supported": mode.startswith("core_") or mode == "baked_in_no_request_context",
            "drop_path_signal_present": mode.endswith("drop_path"),
            "lifecycle_warning_count": 0,
            "artifact_path": None,
            "round": round_idx,
            "phase": "measured",
            "is_warmup": False,
        }
        if mode != "baseline":
            row["truncation"] = {
                "dropped_requests": 0,
                "dropped_stages": 0,
                "dropped_queues": 0,
                "dropped_inflight_snapshots": 0,
                "dropped_runtime_snapshots": 0,
                "limits_reached": False,
            }
        if mode == "tracing_light_drop_path":
            row["truncation"] = {
                "dropped_requests": 4,
                "dropped_stages": 4,
                "dropped_queues": 4,
                "dropped_inflight_snapshots": 4,
                "dropped_runtime_snapshots": 1,
                "limits_reached": True,
            }
        return row

    def test_mode_matrix_preserves_unsaturated_saturated_and_sampler_scenarios(self) -> None:
        self.assertEqual(
            measure_runtime_cost.UNSATURATED_CORE_MODES,
            ("core_light", "core_investigation"),
        )
        self.assertEqual(
            measure_runtime_cost.SATURATED_DROP_PATH_MODES,
            ("core_light_drop_path", "core_investigation_drop_path"),
        )
        self.assertEqual(
            measure_runtime_cost.TOKIO_SAMPLER_MODES,
            ("core_light_tokio_sampler", "core_investigation_tokio_sampler"),
        )
        self.assertEqual(
            measure_runtime_cost.MODES,
            (
                "baseline",
                "baked_in_no_request_context",
                "core_light",
                "core_investigation",
                "core_light_tokio_sampler",
                "core_investigation_tokio_sampler",
                "core_light_drop_path",
                "core_investigation_drop_path",
                "tracing_light",
                "tracing_light_tokio_sampler",
                "tracing_light_drop_path",
            ),
        )

    def test_safe_ratio_handles_zero_denominator(self) -> None:
        self.assertIsNone(measure_runtime_cost.safe_ratio(10.0, 0.0))
        self.assertEqual(measure_runtime_cost.safe_ratio(10.0, 2.0), 5.0)

    def test_summary_includes_required_overhead_headings_and_drop_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            raw_path = Path(tmp) / "runtime-cost-raw.jsonl"
            summary_path = Path(tmp) / "runtime-cost-summary.json"

            rows = [self._base_row(mode, round_idx) for round_idx in range(4) for mode in measure_runtime_cost.MODES]

            raw_path.write_text("\n".join(json.dumps(row) for row in rows) + "\n", encoding="utf-8")
            summary = measure_runtime_cost.summarize(raw_path, summary_path)

            self.assertIn("Core mode overhead", summary["delta_vs_baseline_pct"])
            self.assertIn("Tokio mode overhead", summary["delta_vs_baseline_pct"])
            self.assertIn("Post-limit / drop-path overhead", summary["delta_vs_baseline_pct"])
            self.assertIn("baked_in_no_request_context", summary["delta_vs_baseline_pct"]["Baked-in overhead"])
            self.assertEqual(
                set(summary["delta_vs_baseline_pct"]["Core mode overhead"]),
                set(measure_runtime_cost.UNSATURATED_CORE_MODES),
            )
            self.assertEqual(
                set(summary["delta_vs_baseline_pct"]["Tokio mode overhead"]),
                set(measure_runtime_cost.TOKIO_SAMPLER_MODES),
            )
            self.assertEqual(
                set(summary["delta_vs_baseline_pct"]["Post-limit / drop-path overhead"]),
                set(measure_runtime_cost.SATURATED_DROP_PATH_MODES),
            )
            self.assertIn(
                "Incremental runtime sampler overhead",
                summary["incremental_runtime_sampler_overhead_pct"],
            )

            expected_ratio_keys = {
                "core_light_vs_baseline_latency_p95",
                "tracing_light_vs_baseline_latency_p95",
                "tracing_light_vs_core_light_latency_p95",
                "core_light_tokio_sampler_vs_core_light_latency_p95",
                "tracing_light_tokio_sampler_vs_tracing_light_latency_p95",
                "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95",
                "tracing_light_drop_path_vs_core_light_drop_path_latency_p95",
                "tracing_light_vs_core_light_throughput",
                "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput",
                "tracing_light_drop_path_vs_core_light_drop_path_throughput",
                "tracing_finalize_vs_native_finalize",
                "tracing_analyze_vs_native_analyze",
                "tracing_render_vs_native_render",
            }
            self.assertEqual(set(summary["tracing_vs_native_ratios"].keys()), expected_ratio_keys)
            self.assertGreater(summary["absolute_metrics"]["core_light"]["run_requests"]["median"], 0)
            self.assertGreater(summary["absolute_metrics"]["tracing_light"]["run_stages"]["median"], 0)
            self.assertGreater(summary["absolute_metrics"]["tracing_light_tokio_sampler"]["run_queues"]["median"], 0)
            self.assertGreater(summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"], 0)
            self.assertGreater(
                summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"],
                0,
            )
            self.assertGreater(summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"], 0)

            drop_summary = summary["absolute_metrics"]["tracing_light_drop_path"]["truncation"]
            self.assertEqual(drop_summary["limit_reached_rounds"], 4)
            self.assertGreater(drop_summary["dropped_requests"]["mean"], 0)

    def test_sanity_fails_on_parity_latency_ratio(self) -> None:
        summary = {"absolute_metrics": {m: {"throughput_rps": {"median": 10.0}, "latency_p95_ms": {"median": 2.0},
                                            "run_requests": {"median": 1}, "run_stages": {"median": 1}, "run_queues": {"median": 1},
                                            "runtime_snapshots": {"median": 0},
                                            "effective_tokio_sampler_config_present_rounds": 0,
                                            "drop_path_signal_present_rounds": 0} for m in measure_runtime_cost.MODES},
                   "tracing_vs_native_ratios": {
                       "tracing_light_vs_core_light_latency_p95": 1.26,
                       "tracing_light_vs_core_light_throughput": 0.5,
                       "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": 1.0,
                       "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": 1.0,
                       "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": 1.0,
                       "tracing_light_drop_path_vs_core_light_drop_path_throughput": 1.0,
                   }}
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 1
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 1
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1
        with self.assertRaises(SystemExit):
            measure_runtime_cost._validate_sanity(summary)

    def test_sanity_fails_on_parity_throughput_ratio(self) -> None:
        summary = {"absolute_metrics": {m: {"throughput_rps": {"median": 10.0}, "latency_p95_ms": {"median": 2.0},
                                            "run_requests": {"median": 1}, "run_stages": {"median": 1}, "run_queues": {"median": 1},
                                            "runtime_snapshots": {"median": 0},
                                            "effective_tokio_sampler_config_present_rounds": 0,
                                            "drop_path_signal_present_rounds": 0} for m in measure_runtime_cost.MODES},
                   "tracing_vs_native_ratios": {
                       "tracing_light_vs_core_light_latency_p95": 1.0,
                       "tracing_light_vs_core_light_throughput": 0.74,
                       "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": 1.0,
                       "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": 1.0,
                       "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": 1.0,
                       "tracing_light_drop_path_vs_core_light_drop_path_throughput": 1.0,
                   }}
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 1
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 1
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1
        with self.assertRaises(SystemExit):
            measure_runtime_cost._validate_sanity(summary)

    def test_sanity_passes_for_reasonable_tracing_ratios(self) -> None:
        summary = {"absolute_metrics": {m: {"throughput_rps": {"median": 10.0}, "latency_p95_ms": {"median": 2.0},
                                            "run_requests": {"median": 1}, "run_stages": {"median": 1}, "run_queues": {"median": 1},
                                            "runtime_snapshots": {"median": 0},
                                            "effective_tokio_sampler_config_present_rounds": 0,
                                            "drop_path_signal_present_rounds": 0} for m in measure_runtime_cost.MODES},
                   "tracing_vs_native_ratios": {
                       "tracing_light_vs_core_light_latency_p95": 1.10,
                       "tracing_light_vs_core_light_throughput": 0.8,
                       "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": 1.10,
                       "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": 0.8,
                       "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": 1.10,
                       "tracing_light_drop_path_vs_core_light_drop_path_throughput": 0.8,
                   }}
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] = 1
        summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] = 1
        summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] = 1
        measure_runtime_cost._validate_sanity(summary)

    def test_parity_warning_for_latency_above_soft_band(self) -> None:
        warnings = measure_runtime_cost.evaluate_tracing_parity({
            "tracing_light_vs_core_light_latency_p95": 1.07,
            "tracing_light_vs_core_light_throughput": 1.0,
            "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": 1.0,
            "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": 1.0,
            "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": 1.0,
            "tracing_light_drop_path_vs_core_light_drop_path_throughput": 1.0,
        })
        self.assertTrue(any("tracing_light p95 is 1.07x native" in w for w in warnings))

    def test_parity_warning_for_throughput_below_soft_band(self) -> None:
        warnings = measure_runtime_cost.evaluate_tracing_parity({
            "tracing_light_vs_core_light_latency_p95": 1.0,
            "tracing_light_vs_core_light_throughput": 0.93,
            "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": 1.0,
            "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": 1.0,
            "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": 1.0,
            "tracing_light_drop_path_vs_core_light_drop_path_throughput": 1.0,
        })
        self.assertTrue(any("tracing_light throughput is 0.93x native" in w for w in warnings))


if __name__ == "__main__":
    unittest.main()
