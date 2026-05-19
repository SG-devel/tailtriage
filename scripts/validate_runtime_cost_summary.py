#!/usr/bin/env python3
"""Validate runtime-cost smoke outputs with bounded deterministic checks."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

import sys

SCRIPTS_DIR = Path(__file__).resolve().parent
if str(SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(SCRIPTS_DIR))

from measure_runtime_cost import MODES, _validate_sanity


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate runtime-cost smoke outputs.")
    parser.add_argument("--raw", required=True, help="Path to runtime-cost-raw.jsonl")
    parser.add_argument("--summary", required=True, help="Path to runtime-cost-summary.json")
    return parser.parse_args()


def validate(raw_path: Path, summary_path: Path) -> None:
    if not raw_path.exists():
        raise SystemExit(f"raw JSONL not found: {raw_path}")
    raw_text = raw_path.read_text(encoding="utf-8")
    if not raw_text.strip():
        raise SystemExit(f"raw JSONL is empty: {raw_path}")

    if not summary_path.exists():
        raise SystemExit(f"summary JSON not found: {summary_path}")
    summary = json.loads(summary_path.read_text(encoding="utf-8"))

    absolute_metrics = summary.get("absolute_metrics")
    if not isinstance(absolute_metrics, dict):
        raise SystemExit("summary missing absolute_metrics")

    missing_modes = [mode for mode in MODES if mode not in absolute_metrics]
    if missing_modes:
        raise SystemExit(f"summary missing modes: {', '.join(missing_modes)}")

    if "tracing_vs_native_ratios" not in summary:
        raise SystemExit("summary missing tracing_vs_native_ratios")

    _validate_sanity(summary)

    for warning in summary.get("parity_warnings") or []:
        print(warning)


def main() -> None:
    args = parse_args()
    validate(Path(args.raw), Path(args.summary))


if __name__ == "__main__":
    main()
