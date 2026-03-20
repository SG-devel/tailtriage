#!/usr/bin/env python3
"""Measure and summarize runtime overhead for tailtriage modes."""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
from pathlib import Path


MODES = ("baseline", "light", "investigation")
METRIC_KEYS = ("throughput_rps", "latency_p50_ms", "latency_p95_ms", "latency_p99_ms")


def parse_args() -> argparse.Namespace:
    root_dir = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Measure runtime overhead for demo modes.")
    parser.add_argument(
        "--artifact-dir",
        default=str(root_dir / "demos/runtime_cost/artifacts"),
        help="Directory for raw and summary output files.",
    )
    parser.add_argument("--requests", type=int, default=int(os.environ.get("REQUESTS", "1200")))
    parser.add_argument("--concurrency", type=int, default=int(os.environ.get("CONCURRENCY", "48")))
    parser.add_argument("--work-ms", type=int, default=int(os.environ.get("WORK_MS", "3")))
    parser.add_argument("--iterations", type=int, default=int(os.environ.get("ITERATIONS", "5")))
    return parser.parse_args()


def summarize(raw_path: Path, summary_path: Path) -> dict:
    rows = [json.loads(line) for line in raw_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    by_mode: dict[str, list[dict]] = {}
    for row in rows:
        by_mode.setdefault(row["mode"], []).append(row)

    for mode in MODES:
        if mode not in by_mode:
            raise SystemExit(f"missing mode: {mode}")

    summary = {
        "requests": by_mode["baseline"][0]["requests"],
        "concurrency": by_mode["baseline"][0]["concurrency"],
        "work_ms": by_mode["baseline"][0]["work_ms"],
        "iterations_per_mode": len(by_mode["baseline"]),
        "modes": {},
    }

    for mode, values in by_mode.items():
        metrics = {key: [row[key] for row in values] for key in METRIC_KEYS}
        summary["modes"][mode] = {
            "throughput_rps_mean": statistics.fmean(metrics["throughput_rps"]),
            "latency_p50_ms_mean": statistics.fmean(metrics["latency_p50_ms"]),
            "latency_p95_ms_mean": statistics.fmean(metrics["latency_p95_ms"]),
            "latency_p99_ms_mean": statistics.fmean(metrics["latency_p99_ms"]),
        }

    baseline = summary["modes"]["baseline"]
    for mode in ("light", "investigation"):
        target = summary["modes"][mode]
        target["throughput_overhead_pct_vs_baseline"] = (
            (baseline["throughput_rps_mean"] - target["throughput_rps_mean"])
            / baseline["throughput_rps_mean"]
        ) * 100.0
        target["p50_overhead_pct_vs_baseline"] = (
            (target["latency_p50_ms_mean"] - baseline["latency_p50_ms_mean"])
            / baseline["latency_p50_ms_mean"]
        ) * 100.0
        target["p95_overhead_pct_vs_baseline"] = (
            (target["latency_p95_ms_mean"] - baseline["latency_p95_ms_mean"])
            / baseline["latency_p95_ms_mean"]
        ) * 100.0
        target["p99_overhead_pct_vs_baseline"] = (
            (target["latency_p99_ms_mean"] - baseline["latency_p99_ms_mean"])
            / baseline["latency_p99_ms_mean"]
        ) * 100.0

    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return summary


def main() -> None:
    args = parse_args()
    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = Path(args.artifact_dir)
    artifact_dir.mkdir(parents=True, exist_ok=True)

    raw_path = artifact_dir / "runtime-cost-raw.jsonl"
    summary_path = artifact_dir / "runtime-cost-summary.json"
    raw_path.write_text("", encoding="utf-8")

    for mode in MODES:
        for _ in range(args.iterations):
            result = subprocess.run(
                [
                    "cargo",
                    "run",
                    "--quiet",
                    "--manifest-path",
                    str(root_dir / "demos/runtime_cost/Cargo.toml"),
                    "--",
                    "--mode",
                    mode,
                    "--requests",
                    str(args.requests),
                    "--concurrency",
                    str(args.concurrency),
                    "--work-ms",
                    str(args.work_ms),
                    "--output-dir",
                    str(artifact_dir),
                ],
                check=True,
                capture_output=True,
                text=True,
            )
            with raw_path.open("a", encoding="utf-8") as raw_file:
                raw_file.write(result.stdout)

    summary = summarize(raw_path, summary_path)
    print(json.dumps(summary, indent=2))
    print(f"raw results: {raw_path}")
    print(f"summary: {summary_path}")


if __name__ == "__main__":
    main()
