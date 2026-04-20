#!/usr/bin/env python3
"""Lightweight structural validation for collector-limits smoke artifacts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path
from typing import Any


REQUIRED_TOP_LEVEL_KEYS = (
    "measurement_kind",
    "profile",
    "default_matrix",
    "cases_by_mode",
    "collector_stress_signals",
    "collector_pressure_onset_markers",
    "measurement_quality",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate collector-limits smoke summary shape (structural only)."
    )
    parser.add_argument("--raw", type=Path, required=True, help="Path to raw JSONL output.")
    parser.add_argument("--summary", type=Path, required=True, help="Path to summary JSON output.")
    return parser.parse_args()


def _load_json(path: Path) -> dict[str, Any]:
    with path.open("r", encoding="utf-8") as handle:
        value = json.load(handle)
    if not isinstance(value, dict):
        raise ValueError(f"{path} did not contain a top-level JSON object")
    return value


def validate_raw_exists(raw_path: Path) -> None:
    if not raw_path.is_file():
        raise FileNotFoundError(f"raw JSONL output not found: {raw_path}")
    lines = [line for line in raw_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    if not lines:
        raise ValueError(f"raw JSONL output is empty: {raw_path}")
    for index, line in enumerate(lines, start=1):
        try:
            parsed = json.loads(line)
        except json.JSONDecodeError as exc:
            raise ValueError(f"raw JSONL line {index} is not valid JSON: {exc}") from exc
        if not isinstance(parsed, dict):
            raise ValueError(f"raw JSONL line {index} is not a JSON object")


def validate_summary_shape(summary: dict[str, Any]) -> None:
    missing = [key for key in REQUIRED_TOP_LEVEL_KEYS if key not in summary]
    if missing:
        raise ValueError(f"summary JSON missing top-level keys: {missing}")

    matrix = summary.get("default_matrix")
    if not isinstance(matrix, list) or not matrix:
        raise ValueError("summary JSON must include a non-empty default_matrix list")

    case_ids = []
    for case in matrix:
        if isinstance(case, dict):
            case_id = case.get("case_id")
            if isinstance(case_id, str):
                case_ids.append(case_id)

    if not any("baseline" in case_id for case_id in case_ids):
        raise ValueError("expected at least one baseline case in default_matrix.case_id")
    if not any("sampler_dense" in case_id for case_id in case_ids):
        raise ValueError("expected at least one sampler-density case in default_matrix.case_id")


def main() -> int:
    args = parse_args()
    validate_raw_exists(args.raw)
    summary = _load_json(args.summary)
    validate_summary_shape(summary)
    print("collector-limits smoke artifacts validated successfully")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
