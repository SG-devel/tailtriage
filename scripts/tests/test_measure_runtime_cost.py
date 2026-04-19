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
            self.assertIn("Baked-in overhead", summary["delta_vs_baseline_pct"])
            self.assertIn("Post-limit / drop-path overhead", summary["delta_vs_baseline_pct"])
            self.assertIn(
                "Incremental runtime sampler overhead",
                summary["incremental_runtime_sampler_overhead_pct"],
            )

            drop_summary = summary["absolute_metrics"]["core_light_drop_path"]["truncation"]
            self.assertEqual(drop_summary["limit_reached_rounds"], 4)
            self.assertGreater(drop_summary["dropped_requests"]["mean"], 0)


if __name__ == "__main__":
    unittest.main()
