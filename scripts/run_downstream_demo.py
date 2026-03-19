#!/usr/bin/env python3
"""Run downstream-stage demo and generate analysis artifact."""

from __future__ import annotations

import argparse
from pathlib import Path

from _demo_runner import run_cli_analysis_json, run_demo_binary


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run downstream demo and analyze output.")
    parser.add_argument(
        "artifact_path",
        nargs="?",
        help="Optional path to write the run JSON artifact.",
    )
    return parser.parse_args()


def main() -> None:
    # Canonical cargo run + analysis helpers live in scripts/_demo_runner.py.
    args = parse_args()
    root_dir = Path(__file__).resolve().parent.parent
    artifact_path = (
        Path(args.artifact_path)
        if args.artifact_path
        else root_dir / "demos/downstream_service/artifacts/downstream-run.json"
    )
    analysis_path = root_dir / "demos/downstream_service/artifacts/downstream-analysis.json"

    artifact_path.parent.mkdir(parents=True, exist_ok=True)

    run_demo_binary(
        root_dir / "demos/downstream_service/Cargo.toml",
        artifact_path,
    )
    run_cli_analysis_json(
        root_dir / "tailscope-cli/Cargo.toml",
        artifact_path,
        analysis_path,
    )

    print(f"run artifact: {artifact_path}")
    print(f"analysis: {analysis_path}")


if __name__ == "__main__":
    main()
