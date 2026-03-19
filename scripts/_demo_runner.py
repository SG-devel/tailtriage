#!/usr/bin/env python3
"""Shared helpers for demo runners."""

from __future__ import annotations

import subprocess
from pathlib import Path


def run_demo_binary(manifest_path: Path, artifact_path: Path, *demo_args: str) -> None:
    """Run a demo binary via ``cargo run --manifest-path ...``."""
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(manifest_path),
            "--",
            str(artifact_path),
            *demo_args,
        ],
        check=True,
    )


def run_cli_analysis_json(cli_manifest_path: Path, artifact_path: Path, analysis_path: Path) -> None:
    """Analyze an artifact and write JSON output to ``analysis_path``."""
    with analysis_path.open("w", encoding="utf-8") as analysis_file:
        subprocess.run(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(cli_manifest_path),
                "--",
                "analyze",
                str(artifact_path),
                "--format",
                "json",
            ],
            check=True,
            stdout=analysis_file,
        )
