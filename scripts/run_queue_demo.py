#!/usr/bin/env python3
"""Run queue demo variants and emit analysis artifacts."""

from __future__ import annotations

import argparse
import json
import subprocess
from pathlib import Path


def run_variant(root_dir: Path, artifact_dir: Path, variant: str) -> None:
    run_path = artifact_dir / f"{variant}-run.json"
    analysis_path = artifact_dir / f"{variant}-analysis.json"
    mode_arg = "baseline" if variant == "before" else "mitigated"

    artifact_dir.mkdir(parents=True, exist_ok=True)

    subprocess.run(
        [
            "cargo",
            "run",
            "--quiet",
            "--manifest-path",
            str(root_dir / "demos/queue_service/Cargo.toml"),
            "--",
            str(run_path),
            mode_arg,
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
                str(run_path),
                "--format",
                "json",
            ],
            check=True,
            stdout=analysis_file,
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
            "p95_queue_share_permille": before.get("p95_queue_share_permille"),
        },
        "after": {
            "primary_suspect_kind": after["primary_suspect"]["kind"],
            "primary_suspect_score": after["primary_suspect"]["score"],
            "p95_latency_us": after["p95_latency_us"],
            "p95_queue_share_permille": after.get("p95_queue_share_permille"),
        },
    }

    before_snapshot = comparison["before"]
    after_snapshot = comparison["after"]
    comparison["delta"] = {
        "p95_latency_us": after_snapshot["p95_latency_us"] - before_snapshot["p95_latency_us"],
        "primary_suspect_score": after_snapshot["primary_suspect_score"]
        - before_snapshot["primary_suspect_score"],
        "p95_queue_share_permille": (
            None
            if before_snapshot["p95_queue_share_permille"] is None
            or after_snapshot["p95_queue_share_permille"] is None
            else after_snapshot["p95_queue_share_permille"] - before_snapshot["p95_queue_share_permille"]
        ),
    }

    comparison_path = artifact_dir / "before-after-comparison.json"
    comparison_path.write_text(json.dumps(comparison, indent=2) + "\n", encoding="utf-8")
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
