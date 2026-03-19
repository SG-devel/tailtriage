#!/usr/bin/env python3
"""Run downstream-stage demo and generate analysis artifact."""

from __future__ import annotations

import argparse
import subprocess
from pathlib import Path


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Run downstream demo and analyze output.")
    parser.add_argument(
        "artifact_path",
        nargs="?",
        help="Optional path to write the run JSON artifact.",
    )
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    root_dir = Path(__file__).resolve().parent.parent
    artifact_path = (
        Path(args.artifact_path)
        if args.artifact_path
        else root_dir / "demos/downstream_service/artifacts/downstream-run.json"
    )
    analysis_path = root_dir / "demos/downstream_service/artifacts/downstream-analysis.json"

    artifact_path.parent.mkdir(parents=True, exist_ok=True)

    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(root_dir / "demos/downstream_service/Cargo.toml"),
            "--",
            str(artifact_path),
        ],
        check=True,
    )

    with analysis_path.open("w", encoding="utf-8") as analysis_file:
        subprocess.run(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(root_dir / "tailscope-cli/Cargo.toml"),
                "--",
                "analyze",
                str(artifact_path),
                "--format",
                "json",
            ],
            check=True,
            stdout=analysis_file,
        )

    print(f"run artifact: {artifact_path}")
    print(f"analysis: {analysis_path}")


if __name__ == "__main__":
    main()
