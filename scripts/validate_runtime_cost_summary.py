#!/usr/bin/env python3
"""Validate runtime-cost smoke outputs for CI sanity checks."""

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
    parser = argparse.ArgumentParser(description="Validate runtime-cost smoke summary outputs.")
    parser.add_argument("--raw", required=True, help="Path to runtime-cost-raw.jsonl")
    parser.add_argument("--summary", required=True, help="Path to runtime-cost-summary.json")
    return parser.parse_args()


def fail(message: str) -> None:
    raise SystemExit(message)


def _median(summary: dict, mode: str, key: str) -> float:
    return summary["absolute_metrics"][mode][key]["median"]


def validate(raw_path: Path, summary_path: Path) -> None:
    if not raw_path.exists():
        fail(f"raw JSONL not found: {raw_path}")
    raw_lines = [line for line in raw_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not raw_lines:
        fail(f"raw JSONL is empty: {raw_path}")

    if not summary_path.exists():
        fail(f"summary JSON not found: {summary_path}")
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    modes = summary.get("absolute_metrics", {})
    missing_modes = [mode for mode in EXPECTED_MODES if mode not in modes]
    if missing_modes:
        fail(f"summary is missing expected modes: {', '.join(missing_modes)}")

    for mode in ("tracing_light", "tracing_light_tokio_sampler"):
        if _median(summary, mode, "run_requests") <= 0:
            fail(f"{mode} must include non-zero request evidence")
        if _median(summary, mode, "run_stages") <= 0:
            fail(f"{mode} must include non-zero stage evidence")
        if _median(summary, mode, "run_queues") <= 0:
            fail(f"{mode} must include non-zero queue evidence")

    if _median(summary, "tracing_light_tokio_sampler", "runtime_snapshots") <= 0:
        fail("tracing_light_tokio_sampler must include non-zero runtime snapshots")
    if summary["absolute_metrics"]["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] <= 0:
        fail("tracing_light_tokio_sampler must include sampler metadata")
    if summary["absolute_metrics"]["tracing_light_drop_path"]["drop_path_signal_present_rounds"] <= 0:
        fail("tracing_light_drop_path must include drop-path signal")

    ratios = summary.get("tracing_vs_native_ratios")
    if not isinstance(ratios, dict):
        fail("summary is missing tracing_vs_native_ratios")

    for key in REQUIRED_RATIO_KEYS:
        if key not in ratios:
            fail(f"missing required tracing-vs-native ratio key: {key}")
        value = ratios[key]
        if value is None or not isinstance(value, (int, float)) or not math.isfinite(value):
            fail(f"{key} must be a finite numeric value")

    if ratios["tracing_light_vs_core_light_latency_p95"] > 20:
        fail("catastrophic threshold failed: tracing_light_vs_core_light_latency_p95 > 20x")
    if ratios["tracing_light_vs_core_light_throughput"] < 0.05:
        fail("catastrophic threshold failed: tracing_light_vs_core_light_throughput < 0.05x")
    if ratios["tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95"] > 20:
        fail("catastrophic threshold failed: tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95 > 20x")
    if ratios["tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput"] < 0.05:
        fail("catastrophic threshold failed: tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput < 0.05x")
    if ratios["tracing_light_drop_path_vs_core_light_drop_path_latency_p95"] > 20:
        fail("catastrophic threshold failed: tracing_light_drop_path_vs_core_light_drop_path_latency_p95 > 20x")
    if ratios["tracing_light_drop_path_vs_core_light_drop_path_throughput"] < 0.05:
        fail("catastrophic threshold failed: tracing_light_drop_path_vs_core_light_drop_path_throughput < 0.05x")


def main() -> None:
    args = parse_args()
    validate(Path(args.raw), Path(args.summary))
    print("runtime-cost summary validation passed")


if __name__ == "__main__":
    main()
