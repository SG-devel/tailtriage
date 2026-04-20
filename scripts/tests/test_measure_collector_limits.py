#!/usr/bin/env python3
"""Schema/shape coverage for collector-limits orchestrator summaries."""

from __future__ import annotations

import unittest

import sys
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / "scripts"
sys.path.insert(0, str(SCRIPTS_DIR))

import measure_collector_limits  # noqa: E402


class CollectorLimitsSummaryTests(unittest.TestCase):
    def test_default_profile_includes_required_modes_and_dimensions(self) -> None:
        self.assertEqual(
            measure_collector_limits.MODES,
            (
                "baseline",
                "core_light",
                "core_investigation",
                "core_light_tokio_sampler",
                "core_investigation_tokio_sampler",
            ),
        )
        case_ids = {case.case_id for case in measure_collector_limits.DEFAULT_CASES}
        self.assertEqual(
            case_ids,
            {
                "baseline_shape",
                "high_concurrency",
                "heavy_event_shape",
                "longer_run",
                "sampler_dense",
            },
        )

    def test_summarize_contains_required_sections(self) -> None:
        rows = []
        run_id = 0
        for case in measure_collector_limits.DEFAULT_CASES:
            for mode in case.modes:
                run_id += 1
                rows.append(
                    {
                        "run_id": run_id,
                        "repeat": 0,
                        "case_id": case.case_id,
                        "case_description": case.description,
                        "mode": mode,
                        "event_shape": {
                            "queues_per_request": case.queues_per_request,
                            "stages_per_request": case.stages_per_request,
                            "inflight_cycles_per_request": case.inflight_cycles_per_request,
                            "work_ms": case.work_ms,
                        },
                        "sampler_settings": {
                            "resolved_sampler_cadence_ms": 200,
                            "cli_interval_ms_override": case.sampler_interval_ms,
                        },
                        "requests_completed": 1000,
                        "run_duration_secs": float(case.duration_secs),
                        "throughput_rps": 1000.0,
                        "latency": {"p50_ms": 1.0, "p95_ms": 2.0, "p99_ms": 3.0, "max_ms": 4.0},
                        "retained_counts": {},
                        "truncation_counts": {
                            "limits_hit": False,
                            "dropped_requests": 0,
                            "dropped_stages": 0,
                            "dropped_queues": 0,
                            "dropped_inflight_snapshots": 0,
                            "dropped_runtime_snapshots": 0,
                        },
                        "artifact": {},
                        "peak_memory": {
                            "collector_peak_rss_bytes": 50_000_000,
                            "collector_end_rss_bytes": 40_000_000,
                        },
                        "memory_measurement": {
                            "path": "external_time_v",
                            "external_peak_rss_bytes": 55_000_000,
                            "notes": [],
                        },
                        "script_artifact": {
                            "size_bytes_measured_by_script": 10_000,
                            "size_bytes_reported_by_binary": 10_000,
                        },
                        "measurement_notes": ["test note"],
                    }
                )

        summary = measure_collector_limits.summarize(
            rows,
            "default",
            measure_collector_limits.MODES,
            measure_collector_limits.DEFAULT_CASES,
        )

        self.assertEqual(summary["measurement_kind"], "collector_limits")
        self.assertIn("cases_by_mode", summary)
        self.assertIn("collector_stress_signals", summary)
        self.assertIn("sampler_density_impact", summary)
        self.assertIn("measurement_quality", summary)
        self.assertIn("outputs", summary)


if __name__ == "__main__":
    unittest.main()
