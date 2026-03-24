#!/usr/bin/env python3
"""Shared helpers for demo run/validation scripts.

This module holds common subprocess and artifact-writing utilities used by
multiple demos. Per-demo scripts should keep only mode selection,
demo-specific metric extraction, and validation semantics.
"""

from __future__ import annotations

import json
import subprocess
from pathlib import Path
from typing import Any

PROFILE_CHOICES = ("dev", "release")


def repo_root(from_file: str) -> Path:
    """Return repository root for a script file path under ``scripts/``."""
    return Path(from_file).resolve().parent.parent


def _profile_args(profile: str) -> list[str]:
    if profile not in PROFILE_CHOICES:
        raise ValueError(f"unsupported profile: {profile}")
    return ["--release"] if profile == "release" else []


def run_demo_binary(
    manifest_path: Path,
    artifact_path: Path,
    *demo_args: str,
    profile: str = "dev",
) -> None:
    """Run a demo binary via ``cargo run --manifest-path ...``."""
    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(manifest_path),
            *_profile_args(profile),
            "--",
            str(artifact_path),
            *demo_args,
        ],
        check=True,
    )


def run_cli_analysis_json(
    cli_manifest_path: Path,
    artifact_path: Path,
    analysis_path: Path,
    *,
    profile: str = "dev",
) -> None:
    """Analyze an artifact and write JSON output to ``analysis_path``."""
    with analysis_path.open("w", encoding="utf-8") as analysis_file:
        subprocess.run(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(cli_manifest_path),
                *_profile_args(profile),
                "--",
                "analyze",
                str(artifact_path),
                "--format",
                "json",
            ],
            check=True,
            stdout=analysis_file,
        )


def run_and_analyze(
    demo_manifest_path: Path,
    cli_manifest_path: Path,
    artifact_path: Path,
    analysis_path: Path,
    *demo_args: str,
    profile: str = "dev",
) -> None:
    """Run demo and analyze the resulting artifact into ``analysis_path``."""
    artifact_path.parent.mkdir(parents=True, exist_ok=True)
    run_demo_binary(demo_manifest_path, artifact_path, *demo_args, profile=profile)
    run_cli_analysis_json(cli_manifest_path, artifact_path, analysis_path, profile=profile)


def variant_paths(artifact_dir: Path, variant: str) -> tuple[Path, Path]:
    """Return ``(run_path, analysis_path)`` for a before/after variant."""
    return artifact_dir / f"{variant}-run.json", artifact_dir / f"{variant}-analysis.json"


def load_report_json(path: Path) -> dict[str, Any]:
    """Load a JSON report file into a dictionary."""
    return json.loads(path.read_text(encoding="utf-8"))


def nullable_delta(before_value: Any, after_value: Any) -> Any:
    """Return ``after - before`` when both values are present, otherwise ``None``."""
    if before_value is None or after_value is None:
        return None
    try:
        return after_value - before_value
    except TypeError:
        return None


def write_before_after_comparison(
    artifact_dir: Path,
    before_snapshot: dict[str, Any],
    after_snapshot: dict[str, Any],
) -> Path:
    """Write a standard before/after comparison file with automatic deltas."""
    delta = {
        key: nullable_delta(before_snapshot[key], after_snapshot[key])
        for key in before_snapshot
        if key in after_snapshot
    }

    comparison = {
        "before": before_snapshot,
        "after": after_snapshot,
        "delta": delta,
    }

    comparison_path = artifact_dir / "before-after-comparison.json"
    comparison_path.write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
    return comparison_path
