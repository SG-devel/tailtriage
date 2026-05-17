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
    def test_mode_matrix_preserves_unsaturated_saturated_and_sampler_scenarios(self) -> None:
        self.assertEqual(
            measure_runtime_cost.UNSATURATED_CORE_MODES,
            ("core_light", "core_investigation", "tracing_light"),
        )
        self.assertEqual(
            measure_runtime_cost.SATURATED_DROP_PATH_MODES,
            ("core_light_drop_path", "core_investigation_drop_path", "tracing_light_drop_path"),
        )
        self.assertEqual(
            measure_runtime_cost.TOKIO_SAMPLER_MODES,
            ("core_light_tokio_sampler", "core_investigation_tokio_sampler", "tracing_light_tokio_sampler"),
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

    def test_summary_includes_required_overhead_headings_and_drop_path(self) -> None:
        with tempfile.TemporaryDirectory() as tmp:
            raw_path = Path(tmp) / "runtime-cost-raw.jsonl"
            summary_path = Path(tmp) / "runtime-cost-summary.json"

            rows = []
            for round_idx in range(4):
                for mode in measure_runtime_cost.MODES:
                    row = {
                        "mode": mode,
                        "requests": 100,
                        "concurrency": 10,
                        "work_ms": 1,
                        "throughput_rps": 1000.0,
                        "latency_p50_ms": 1.0,
                        "latency_p95_ms": 2.0,
                        "latency_p99_ms": 3.0,
                        "artifact_finalize_ms": 1.0,
                        "analyze_ms": 1.0,
                        "report_render_ms": 1.0,
                        "run_requests": 100,
                        "run_stages": 100,
                        "run_queues": 100,
                        "runtime_snapshots": 0,
                        "lifecycle_warning_count": 0,
                        "effective_tokio_sampler_config_present": mode.endswith("tokio_sampler"),
                        "inflight_supported": not mode.startswith("tracing"),
                        "drop_path_signal_present": mode.endswith("drop_path"),
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
                    if mode == "core_light_drop_path":
                        row["truncation"] = {
                            "dropped_requests": 4,
                            "dropped_stages": 4,
                            "dropped_queues": 4,
                            "dropped_inflight_snapshots": 4,
                            "dropped_runtime_snapshots": 0,
                            "limits_reached": True,
                        }
                    rows.append(row)

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

            drop_summary = summary["absolute_metrics"]["core_light_drop_path"]["truncation"]
            self.assertEqual(drop_summary["limit_reached_rounds"], 4)
            self.assertGreater(drop_summary["dropped_requests"]["mean"], 0)


if __name__ == "__main__":
    unittest.main()


class RuntimeCostHelperTests(unittest.TestCase):
    def test_safe_ratio_zero_denominator(self) -> None:
        self.assertIsNone(measure_runtime_cost.safe_ratio(1.0, 0.0))

    def test_median_even_odd(self) -> None:
        by_mode = {"m": [{"x": 1.0}, {"x": 3.0}], "n": [{"x": 1.0}, {"x": 2.0}, {"x": 3.0}]}
        self.assertEqual(measure_runtime_cost.median_metric(by_mode, "m", "x"), 2.0)
        self.assertEqual(measure_runtime_cost.median_metric(by_mode, "n", "x"), 2.0)

