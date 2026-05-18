#!/usr/bin/env python3
"""Validate runtime-cost smoke artifacts for CI sanity coverage."""

from __future__ import annotations

import argparse
import json
import sys
from pathlib import Path

ROOT_DIR = Path(__file__).resolve().parent.parent
if str(ROOT_DIR) not in sys.path:
    sys.path.insert(0, str(ROOT_DIR))

from scripts import measure_runtime_cost


EXPECTED_MODES = measure_runtime_cost.MODES
REQUIRED_RATIO_KEYS = (
    "tracing_light_vs_core_light_latency_p95",
    "tracing_light_vs_core_light_throughput",
    "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95",
    "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput",
    "tracing_light_drop_path_vs_core_light_drop_path_latency_p95",
    "tracing_light_drop_path_vs_core_light_drop_path_throughput",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate runtime-cost smoke raw/summary artifacts.")
    parser.add_argument("--raw", required=True, help="Path to runtime-cost raw JSONL file.")
    parser.add_argument("--summary", required=True, help="Path to runtime-cost summary JSON file.")
    return parser.parse_args()


def _read_non_empty_jsonl(path: Path) -> None:
    if not path.exists():
        raise SystemExit(f"raw JSONL does not exist: {path}")
    lines = [line for line in path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not lines:
        raise SystemExit(f"raw JSONL is empty: {path}")


def _read_summary(path: Path) -> dict:
    if not path.exists():
        raise SystemExit(f"summary JSON does not exist: {path}")
    try:
        summary = json.loads(path.read_text(encoding="utf-8"))
    except json.JSONDecodeError as exc:
        raise SystemExit(f"summary JSON is invalid: {path}: {exc}") from exc
    if not isinstance(summary, dict):
        raise SystemExit("summary JSON root must be an object")
    return summary


def validate_summary(summary: dict) -> None:
    absolute_metrics = summary.get("absolute_metrics")
    if not isinstance(absolute_metrics, dict):
        raise SystemExit("summary must include absolute_metrics object")

    missing_modes = [mode for mode in EXPECTED_MODES if mode not in absolute_metrics]
    if missing_modes:
        raise SystemExit(f"summary missing expected modes: {', '.join(missing_modes)}")

    measure_runtime_cost._validate_sanity(summary)

    ratios = summary.get("tracing_vs_native_ratios")
    if not isinstance(ratios, dict):
        raise SystemExit("summary must include tracing_vs_native_ratios object")

    for key in REQUIRED_RATIO_KEYS:
        if key not in ratios:
            raise SystemExit(f"summary missing required tracing-vs-native ratio key: {key}")


def main() -> None:
    args = parse_args()
    raw_path = Path(args.raw)
    summary_path = Path(args.summary)
    _read_non_empty_jsonl(raw_path)
    summary = _read_summary(summary_path)
    validate_summary(summary)


if __name__ == "__main__":
    main()
