#!/usr/bin/env python3
"""Run queue demo variants and emit analysis artifacts."""

from __future__ import annotations

import argparse
import json
from pathlib import Path

from _demo_runner import run_cli_analysis_json, run_demo_binary, write_before_after_comparison


def run_variant(root_dir: Path, artifact_dir: Path, variant: str) -> None:
    run_path = artifact_dir / f"{variant}-run.json"
    analysis_path = artifact_dir / f"{variant}-analysis.json"
    mode_arg = "baseline" if variant == "before" else "mitigated"

    artifact_dir.mkdir(parents=True, exist_ok=True)

    run_demo_binary(
        root_dir / "demos/queue_service/Cargo.toml",
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
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
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
    parser = argparse.ArgumentParser(description="Run queue demo and analyze outputs.")
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
    artifact_dir = root_dir / "demos/queue_service/artifacts"

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
