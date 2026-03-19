#!/usr/bin/env python3
"""Run blocking demo variants and emit analysis artifacts."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from _demo_runner import run_cli_analysis_json, run_demo_binary


def extract_blocking_queue_depth_p95(report: dict) -> int | None:
    suspect = report.get("primary_suspect") or {}
    for evidence in suspect.get("evidence") or []:
        match = re.search(r"Blocking queue depth p95 is (\d+)", evidence)
        if match:
            return int(match.group(1))
    return None


def run_variant(root_dir: Path, artifact_dir: Path, variant: str) -> None:
    # Canonical cargo run + analysis helpers live in scripts/_demo_runner.py.
    run_path = artifact_dir / f"{variant}-run.json"
    analysis_path = artifact_dir / f"{variant}-analysis.json"
    mode_arg = "baseline" if variant == "before" else "mitigated"

    artifact_dir.mkdir(parents=True, exist_ok=True)

    run_demo_binary(
        root_dir / "demos/blocking_service/Cargo.toml",
        run_path,
        mode_arg,
    )
    run_cli_analysis_json(
        root_dir / "tailscope-cli/Cargo.toml",
        run_path,
        analysis_path,
    )

    print(f"run artifact ({variant}): {run_path}")
    print(f"analysis ({variant}): {analysis_path}")


def write_comparison(artifact_dir: Path) -> None:
    before = json.loads((artifact_dir / "before-analysis.json").read_text())
    after = json.loads((artifact_dir / "after-analysis.json").read_text())

    comparison = {
        "before": {
            "primary_suspect_kind": before["primary_suspect"]["kind"],
            "primary_suspect_score": before["primary_suspect"]["score"],
            "p95_latency_us": before["p95_latency_us"],
            "p95_service_share_permille": before.get("p95_service_share_permille"),
            "blocking_queue_depth_p95": extract_blocking_queue_depth_p95(before),
        },
        "after": {
            "primary_suspect_kind": after["primary_suspect"]["kind"],
            "primary_suspect_score": after["primary_suspect"]["score"],
            "p95_latency_us": after["p95_latency_us"],
            "p95_service_share_permille": after.get("p95_service_share_permille"),
            "blocking_queue_depth_p95": extract_blocking_queue_depth_p95(after),
        },
    }

    before_snapshot = comparison["before"]
    after_snapshot = comparison["after"]
    comparison["delta"] = {
        "p95_latency_us": after_snapshot["p95_latency_us"] - before_snapshot["p95_latency_us"],
        "primary_suspect_score": after_snapshot["primary_suspect_score"]
        - before_snapshot["primary_suspect_score"],
        "p95_service_share_permille": (
            None
            if before_snapshot["p95_service_share_permille"] is None
            or after_snapshot["p95_service_share_permille"] is None
            else after_snapshot["p95_service_share_permille"] - before_snapshot["p95_service_share_permille"]
        ),
        "blocking_queue_depth_p95": (
            None
            if before_snapshot["blocking_queue_depth_p95"] is None
            or after_snapshot["blocking_queue_depth_p95"] is None
            else after_snapshot["blocking_queue_depth_p95"] - before_snapshot["blocking_queue_depth_p95"]
        ),
    }

    comparison_path = artifact_dir / "before-after-comparison.json"
    comparison_path.write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
    print(f"comparison: {comparison_path}")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run blocking demo and analyze outputs.")
    parser.add_argument(
        "mode",
        nargs="?",
        default="both",
        choices=["before", "after", "both", "baseline", "mitigated"],
        help="Variant to run.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = root_dir / "demos/blocking_service/artifacts"

    mode = args.mode
    if mode in {"before", "baseline"}:
        run_variant(root_dir, artifact_dir, "before")
    elif mode in {"after", "mitigated"}:
        run_variant(root_dir, artifact_dir, "after")
    else:
        run_variant(root_dir, artifact_dir, "before")
        run_variant(root_dir, artifact_dir, "after")
        write_comparison(artifact_dir)


if __name__ == "__main__":
    main()
