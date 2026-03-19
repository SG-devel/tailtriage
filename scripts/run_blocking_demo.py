#!/usr/bin/env python3
"""Run blocking demo variants and emit analysis artifacts."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path

from _demo_runner import run_cli_analysis_json, run_demo_binary, write_before_after_comparison


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


def snapshot(report: dict) -> dict[str, int | str | None]:
    return {
        "primary_suspect_kind": report["primary_suspect"]["kind"],
        "primary_suspect_score": report["primary_suspect"]["score"],
        "p95_latency_us": report["p95_latency_us"],
        "p95_service_share_permille": report.get("p95_service_share_permille"),
        "blocking_queue_depth_p95": extract_blocking_queue_depth_p95(report),
    }


def write_comparison(artifact_dir: Path) -> None:
    before = json.loads((artifact_dir / "before-analysis.json").read_text())
    after = json.loads((artifact_dir / "after-analysis.json").read_text())

    comparison_path = write_before_after_comparison(
        artifact_dir,
        snapshot(before),
        snapshot(after),
    )
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
