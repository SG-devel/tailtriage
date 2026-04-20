#!/usr/bin/env python3
"""Deterministic unit coverage for collector-limits summary helpers."""

from __future__ import annotations

import sys
import unittest
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import measure_collector_limits  # noqa: E402


class CollectorLimitsSummaryTests(unittest.TestCase):
    def _make_row(
        self,
        *,
        case_id: str,
        mode: str,
        throughput_rps: float,
        latency_p95_ms: float,
        artifact_bytes: int | None,
        external_peak_rss: int | None,
        fallback_peak_rss: int | None,
        limits_hit: bool,
        dropped_requests: int = 0,
        dropped_runtime_snapshots: int = 0,
        sampler_interval_override: int | None = None,
    ) -> dict[str, object]:
        return {
            "run_id": 1,
            "repeat": 0,
            "case_id": case_id,
            "case_description": f"synthetic-{case_id}",
            "mode": mode,
            "event_shape": {
                "queues_per_request": 3,
                "stages_per_request": 4,
                "inflight_cycles_per_request": 6,
                "work_ms": 2,
            },
            "concurrency": {
                "low_concurrency": 32,
                "baseline_shape": 128,
                "high_concurrency": 256,
                "heavy_event_shape": 128,
                "longer_run": 128,
                "sampler_dense": 128,
            }[case_id],
            "configured_duration_secs": 20,
            "request_limit": None,
            "sampler_settings": {
                "resolved_sampler_cadence_ms": 200,
                "cli_interval_ms_override": sampler_interval_override,
            },
            "requests_completed": 1000,
            "run_duration_secs": 20.0,
            "throughput_rps": throughput_rps,
            "latency": {
                "p50_ms": 5.0,
                "p95_ms": latency_p95_ms,
                "p99_ms": latency_p95_ms + 2.0,
                "max_ms": latency_p95_ms + 8.0,
            },
            "retained_counts": {},
            "truncation_counts": {
                "limits_hit": limits_hit,
                "dropped_requests": dropped_requests,
                "dropped_stages": 0,
                "dropped_queues": 0,
                "dropped_inflight_snapshots": 0,
                "dropped_runtime_snapshots": dropped_runtime_snapshots,
            },
            "artifact": {},
            "peak_memory": {
                "collector_peak_rss_bytes": fallback_peak_rss,
                "collector_end_rss_bytes": fallback_peak_rss,
            },
            "memory_measurement": {
                "path": "external_time_v" if external_peak_rss is not None else "in_process_fallback",
                "external_peak_rss_bytes": external_peak_rss,
                "notes": ["synthetic note"],
            },
            "script_artifact": {
                "size_bytes_measured_by_script": artifact_bytes,
                "size_bytes_reported_by_binary": artifact_bytes,
            },
            "measurement_notes": ["collector synthetic note"],
        }

    def test_parse_modes_validation(self) -> None:
        parsed = measure_collector_limits.parse_modes(" baseline , core_light ")
        self.assertEqual(parsed, ("baseline", "core_light"))

        with self.assertRaises(SystemExit):
            measure_collector_limits.parse_modes("  ")
        with self.assertRaises(SystemExit):
            measure_collector_limits.parse_modes("baseline,unknown_mode")

    def test_pct_delta(self) -> None:
        self.assertEqual(measure_collector_limits.pct_delta(100.0, 125.0), 25.0)
        self.assertEqual(measure_collector_limits.pct_delta(10.0, 8.0), -20.0)
        self.assertIsNone(measure_collector_limits.pct_delta(None, 8.0))
        self.assertIsNone(measure_collector_limits.pct_delta(10.0, None))
        self.assertIsNone(measure_collector_limits.pct_delta(0.0, 1.0))

    def test_summarize_values_handles_empty_and_nonempty(self) -> None:
        self.assertEqual(
            measure_collector_limits.summarize_values([]),
            {
                "count": 0,
                "mean": None,
                "median": None,
                "min": None,
                "max": None,
                "stdev": None,
            },
        )

        summary = measure_collector_limits.summarize_values([1.0, 2.0, 3.0])
        self.assertEqual(summary["count"], 3)
        self.assertEqual(summary["mean"], 2.0)
        self.assertEqual(summary["median"], 2.0)
        self.assertEqual(summary["min"], 1.0)
        self.assertEqual(summary["max"], 3.0)
        self.assertEqual(summary["stdev"], 1.0)

    def test_group_rows_groups_by_case_and_mode(self) -> None:
        rows = [
            {"case_id": "baseline_shape", "mode": "baseline", "run_id": 1},
            {"case_id": "baseline_shape", "mode": "baseline", "run_id": 2},
            {"case_id": "high_concurrency", "mode": "baseline", "run_id": 3},
        ]
        grouped = measure_collector_limits.group_rows(rows)

        self.assertEqual(set(grouped), {"baseline_shape::baseline", "high_concurrency::baseline"})
        self.assertEqual([row["run_id"] for row in grouped["baseline_shape::baseline"]], [1, 2])

    def test_signal_for_mode_handles_derived_and_missing_metrics(self) -> None:
        summary_by_case_mode = {
            "low_concurrency::core_light": {
                "absolute_metrics": {"throughput_rps": {"mean": 120.0}, "latency_p95_ms": {"mean": 8.0}},
                "artifact_size": {"size_bytes_measured_by_script": {"mean": None}},
                "memory": {"peak_rss_bytes": {"mean": None}},
                "truncation": {"limits_hit_runs": 0},
            },
            "baseline_shape::core_light": {
                "absolute_metrics": {"throughput_rps": {"mean": 100.0}, "latency_p95_ms": {"mean": 10.0}},
                "artifact_size": {"size_bytes_measured_by_script": {"mean": 1000.0}},
                "memory": {"peak_rss_bytes": {"mean": 100.0}},
                "truncation": {"limits_hit_runs": 0},
            },
            "high_concurrency::core_light": {
                "absolute_metrics": {"throughput_rps": {"mean": 70.0}, "latency_p95_ms": {"mean": 15.0}},
                "artifact_size": {"size_bytes_measured_by_script": {"mean": 1200.0}},
                "memory": {"peak_rss_bytes": {"mean": 110.0}},
                "truncation": {"limits_hit_runs": 1},
            },
            "heavy_event_shape::core_light": {
                "absolute_metrics": {"throughput_rps": {"mean": 95.0}, "latency_p95_ms": {"mean": 11.0}},
                "artifact_size": {"size_bytes_measured_by_script": {"mean": 1500.0}},
                "memory": {"peak_rss_bytes": {"mean": 120.0}},
                "truncation": {"limits_hit_runs": 0},
            },
            "longer_run::core_light": {
                "absolute_metrics": {"throughput_rps": {"mean": 92.0}, "latency_p95_ms": {"mean": 11.5}},
                "artifact_size": {"size_bytes_measured_by_script": {"mean": 1050.0}},
                "memory": {"peak_rss_bytes": {"mean": 140.0}},
                "truncation": {"limits_hit_runs": 0},
            },
        }

        signal = measure_collector_limits.signal_for_mode(summary_by_case_mode, "core_light")

        self.assertEqual(signal["throughput_delta_low_to_mid_pct"], -16.666666666666664)
        self.assertEqual(signal["throughput_delta_mid_to_high_pct"], -30.0)
        self.assertEqual(signal["latency_p95_delta_mid_to_high_pct"], 50.0)
        self.assertEqual(signal["artifact_growth_heavy_event_shape_pct"], 50.0)
        self.assertEqual(signal["peak_rss_growth_longer_run_pct"], 40.0)
        self.assertEqual(signal["limits_hit_runs_by_case"]["high_concurrency"], 1.0)

        missing = measure_collector_limits.signal_for_mode({}, "baseline")
        self.assertIsNone(missing["throughput_delta_low_to_mid_pct"])
        self.assertEqual(
            missing["limits_hit_runs_by_case"],
            {
                "low_concurrency": None,
                "baseline_shape": None,
                "high_concurrency": None,
                "heavy_event_shape": None,
                "longer_run": None,
            },
        )

    def test_summarize_synthetic_rows_exposes_structure_mode_filtering_sampler_and_onset(self) -> None:
        mode = "core_light_tokio_sampler"
        rows = [
            self._make_row(
                case_id="low_concurrency",
                mode=mode,
                throughput_rps=120.0,
                latency_p95_ms=8.0,
                artifact_bytes=None,
                external_peak_rss=None,
                fallback_peak_rss=None,
                limits_hit=False,
            ),
            self._make_row(
                case_id="baseline_shape",
                mode=mode,
                throughput_rps=100.0,
                latency_p95_ms=10.0,
                artifact_bytes=1000,
                external_peak_rss=100,
                fallback_peak_rss=90,
                limits_hit=False,
                dropped_runtime_snapshots=1,
            ),
            self._make_row(
                case_id="high_concurrency",
                mode=mode,
                throughput_rps=70.0,
                latency_p95_ms=15.0,
                artifact_bytes=1200,
                external_peak_rss=110,
                fallback_peak_rss=105,
                limits_hit=True,
                dropped_requests=1,
            ),
            self._make_row(
                case_id="heavy_event_shape",
                mode=mode,
                throughput_rps=95.0,
                latency_p95_ms=11.0,
                artifact_bytes=1500,
                external_peak_rss=120,
                fallback_peak_rss=115,
                limits_hit=True,
            ),
            self._make_row(
                case_id="longer_run",
                mode=mode,
                throughput_rps=92.0,
                latency_p95_ms=11.5,
                artifact_bytes=1050,
                external_peak_rss=140,
                fallback_peak_rss=130,
                limits_hit=True,
            ),
            self._make_row(
                case_id="sampler_dense",
                mode=mode,
                throughput_rps=90.0,
                latency_p95_ms=12.0,
                artifact_bytes=1005,
                external_peak_rss=102,
                fallback_peak_rss=95,
                limits_hit=False,
                dropped_runtime_snapshots=5,
                sampler_interval_override=50,
            ),
        ]

        cases = tuple(
            case
            for case in measure_collector_limits.DEFAULT_CASES
            if case.case_id
            in {"low_concurrency", "baseline_shape", "high_concurrency", "heavy_event_shape", "longer_run", "sampler_dense"}
        )

        summary = measure_collector_limits.summarize(rows, "default", (mode,), cases)

        self.assertEqual(summary["measurement_kind"], "collector_limits")
        self.assertEqual(summary["modes"], [mode])
        self.assertEqual(summary["mode_count"], 1)
        self.assertIn("cases_by_mode", summary)
        self.assertIn("collector_stress_signals", summary)
        self.assertIn("collector_pressure_onset_markers", summary)
        self.assertIn("sampler_density_impact", summary)
        self.assertIn("measurement_quality", summary)

        low_case = summary["cases_by_mode"][f"low_concurrency::{mode}"]
        self.assertEqual(low_case["artifact_size"]["size_bytes_measured_by_script"]["count"], 0)
        self.assertIsNone(low_case["artifact_size"]["size_bytes_measured_by_script"]["mean"])
        self.assertEqual(low_case["memory"]["peak_rss_bytes"]["count"], 0)
        self.assertIsNone(low_case["memory"]["peak_rss_bytes"]["mean"])

        sampler = summary["sampler_density_impact"][mode]
        self.assertEqual(sampler["throughput_delta_pct"], -10.0)
        self.assertEqual(sampler["latency_p95_delta_pct"], 20.0)
        self.assertEqual(sampler["runtime_snapshot_drop_delta_pct"], 400.0)
        self.assertEqual(sampler["baseline_sampler_cadence_ms"], 200)
        self.assertEqual(sampler["dense_sampler_cadence_ms"], 50)

        signal = summary["collector_stress_signals"]["per_mode"][0]
        self.assertEqual(signal["throughput_delta_high_concurrency_pct"], -30.0)
        self.assertEqual(signal["latency_p95_delta_high_concurrency_pct"], 50.0)
        self.assertEqual(signal["artifact_growth_heavy_event_shape_pct"], 50.0)
        self.assertEqual(signal["peak_rss_growth_longer_run_pct"], 40.0)

        onset = summary["collector_pressure_onset_markers"]["per_mode"][0]
        self.assertEqual(onset["first_limits_hit_case"], "high_concurrency")
        self.assertEqual(onset["first_nonzero_dropped_case_by_category"]["dropped_requests"], "high_concurrency")
        self.assertEqual(onset["first_growth_threshold_crossing_case"]["artifact_size"], "heavy_event_shape")
        self.assertEqual(onset["first_growth_threshold_crossing_case"]["peak_rss_memory"], "longer_run")


if __name__ == "__main__":
    unittest.main()
