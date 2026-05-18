#!/usr/bin/env python3
"""Validate bounded runtime-cost smoke outputs used by CI."""

from __future__ import annotations

import argparse
import json
import math
from pathlib import Path

EXPECTED_MODES = (
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
)

REQUIRED_RATIO_KEYS = (
    "tracing_light_vs_core_light_latency_p95",
    "tracing_light_vs_core_light_throughput",
    "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95",
    "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput",
    "tracing_light_drop_path_vs_core_light_drop_path_latency_p95",
    "tracing_light_drop_path_vs_core_light_drop_path_throughput",
)


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Validate runtime-cost smoke raw+summary artifacts.")
    p.add_argument("--raw", required=True)
    p.add_argument("--summary", required=True)
    return p.parse_args()


def _require(cond: bool, msg: str) -> None:
    if not cond:
        raise SystemExit(msg)


def _median(summary: dict, mode: str, metric: str) -> float:
    return float(summary["absolute_metrics"][mode][metric]["median"])


def validate(raw_path: Path, summary_path: Path) -> None:
    _require(raw_path.exists(), f"missing raw JSONL: {raw_path}")
    _require(raw_path.stat().st_size > 0, f"raw JSONL is empty: {raw_path}")

    _require(summary_path.exists(), f"missing summary JSON: {summary_path}")
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    absolute_metrics = summary.get("absolute_metrics")
    _require(isinstance(absolute_metrics, dict), "summary missing absolute_metrics")

    missing_modes = [m for m in EXPECTED_MODES if m not in absolute_metrics]
    _require(not missing_modes, f"summary missing modes: {', '.join(missing_modes)}")

    for metric in ("run_requests", "run_stages", "run_queues"):
        _require(_median(summary, "tracing_light", metric) > 0, f"tracing_light missing {metric} evidence")
        _require(
            _median(summary, "tracing_light_tokio_sampler", metric) > 0,
            f"tracing_light_tokio_sampler missing {metric} evidence",
        )

    _require(_median(summary, "tracing_light_tokio_sampler", "runtime_snapshots") > 0, "tracing tokio sampler missing runtime snapshots")
    _require(
        int(absolute_metrics["tracing_light_tokio_sampler"].get("effective_tokio_sampler_config_present_rounds", 0)) > 0,
        "tracing tokio sampler missing sampler metadata",
    )
    _require(
        int(absolute_metrics["tracing_light_drop_path"].get("drop_path_signal_present_rounds", 0)) > 0,
        "tracing drop-path mode missing drop-path signal",
    )

    ratios = summary.get("tracing_vs_native_ratios")
    _require(isinstance(ratios, dict), "summary missing tracing_vs_native_ratios")
    for key in REQUIRED_RATIO_KEYS:
        _require(key in ratios, f"missing tracing-vs-native ratio key: {key}")
        val = ratios[key]
        _require(val is not None, f"ratio {key} is null")
        _require(isinstance(val, (int, float)), f"ratio {key} is non-numeric")
        _require(math.isfinite(float(val)), f"ratio {key} is NaN or infinite")


if __name__ == "__main__":
    args = parse_args()
    validate(Path(args.raw), Path(args.summary))
    print("runtime-cost smoke summary validation passed")
