#!/usr/bin/env python3
"""Measure and summarize runtime overhead for tailtriage modes."""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
from pathlib import Path

MODES = ("baseline", "light", "investigation")
METRIC_KEYS = ("throughput_rps", "latency_p50_ms", "latency_p95_ms", "latency_p99_ms")
PROFILE_CHOICES = ("dev", "release")


def parse_args() -> argparse.Namespace:
    root_dir = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(
        description=(
            "Measure runtime overhead for demo modes. "
            "Release profile is default for production-like overhead measurement."
        )
    )
    parser.add_argument(
        "--artifact-dir",
        default=str(root_dir / "demos/runtime_cost/artifacts"),
        help="Directory for raw and summary output files.",
    )
    parser.add_argument("--requests", type=int, default=int(os.environ.get("REQUESTS", "6000")))
    parser.add_argument("--concurrency", type=int, default=int(os.environ.get("CONCURRENCY", "48")))
    parser.add_argument("--work-ms", type=int, default=int(os.environ.get("WORK_MS", "3")))
    parser.add_argument(
        "--rounds",
        type=int,
        default=int(os.environ.get("ROUNDS", "8")),
        help="Measured interleaved rounds per mode (excluding warmup rounds).",
    )
    parser.add_argument(
        "--warmup-rounds",
        type=int,
        default=int(os.environ.get("WARMUP_ROUNDS", "2")),
        help="Interleaved warmup rounds per mode; excluded from summary statistics.",
    )
    parser.add_argument(
        "--profile",
        choices=PROFILE_CHOICES,
        default=os.environ.get("RUNTIME_COST_PROFILE", "release"),
        help="Cargo build profile for runtime_cost binary (default: release).",
    )
    parser.add_argument(
        "--release",
        action="store_true",
        help="Shortcut for --profile release.",
    )
    return parser.parse_args()


def _stats(values: list[float]) -> dict[str, float]:
    if not values:
        return {"mean": 0.0, "median": 0.0, "min": 0.0, "max": 0.0, "stdev": 0.0, "cv": 0.0}
    mean = statistics.fmean(values)
    stdev = statistics.stdev(values) if len(values) > 1 else 0.0
    cv = stdev / mean if mean else 0.0
    return {
        "mean": mean,
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": stdev,
        "cv": cv,
    }


def _paired_overhead_pct(baseline_values: list[float], target_values: list[float]) -> dict[str, float]:
    deltas = []
    for baseline, target in zip(baseline_values, target_values):
        if baseline == 0:
            continue
        deltas.append(((target - baseline) / baseline) * 100.0)
    return _stats(deltas)


def summarize(raw_path: Path, summary_path: Path, *, profile: str, warmup_rounds: int) -> dict:
    rows = [json.loads(line) for line in raw_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    measured_rows = [row for row in rows if row["phase"] == "measure"]

    by_mode: dict[str, list[dict]] = {}
    for row in measured_rows:
        by_mode.setdefault(row["mode"], []).append(row)

    for mode in MODES:
        if not by_mode[mode]:
            raise SystemExit(f"missing measured data for mode: {mode}")

    measured_round_indices = sorted({row["round"] for row in measured})
    measured_rounds: list[dict] = []
    for round_idx in measured_round_indices:
        round_rows = {row["mode"]: row for row in measured if row["round"] == round_idx}
        missing_modes = [mode for mode in MODES if mode not in round_rows]
        if missing_modes:
            raise SystemExit(f"round {round_idx} missing modes: {', '.join(missing_modes)}")
        measured_rounds.append(round_rows)

    summary = {
        "profile": profile,
        "warmup_rounds_per_mode": warmup_rounds,
        "measured_rounds_per_mode": len(by_mode["baseline"]),
        "requests": by_mode["baseline"][0]["requests"],
        "concurrency": by_mode["baseline"][0]["concurrency"],
        "work_ms": by_mode["baseline"][0]["work_ms"],
        "modes": {},
        "stability": {},
    }

    for mode, values in by_mode.items():
        metrics = {key: [float(row[key]) for row in values] for key in METRIC_KEYS}
        summary["modes"][mode] = {
            "throughput_rps": _stats(metrics["throughput_rps"]),
            "latency_p50_ms": _stats(metrics["latency_p50_ms"]),
            "latency_p95_ms": _stats(metrics["latency_p95_ms"]),
            "latency_p99_ms": _stats(metrics["latency_p99_ms"]),
        }

    baseline = by_mode["baseline"]
    for mode in ("light", "investigation"):
        target_rows = by_mode[mode]
        summary["modes"][mode]["paired_overhead_pct_vs_baseline"] = {
            "throughput": _paired_overhead_pct(
                [float(row["throughput_rps"]) for row in baseline],
                [float(row["throughput_rps"]) for row in target_rows],
            ),
            "latency_p50": _paired_overhead_pct(
                [float(row["latency_p50_ms"]) for row in baseline],
                [float(row["latency_p50_ms"]) for row in target_rows],
            ),
            "latency_p95": _paired_overhead_pct(
                [float(row["latency_p95_ms"]) for row in baseline],
                [float(row["latency_p95_ms"]) for row in target_rows],
            ),
            "latency_p99": _paired_overhead_pct(
                [float(row["latency_p99_ms"]) for row in baseline],
                [float(row["latency_p99_ms"]) for row in target_rows],
            ),
        }

    throughput_cv = max(summary["modes"][mode]["throughput_rps"]["cv"] for mode in MODES)
    p95_cv = max(summary["modes"][mode]["latency_p95_ms"]["cv"] for mode in MODES)
    stable = throughput_cv <= 0.10 and p95_cv <= 0.10
    summary["stability"] = {
        "is_stable": stable,
        "max_throughput_cv": throughput_cv,
        "max_latency_p95_cv": p95_cv,
        "quality": "stable" if stable else "noisy",
        "warning": (
            "high variance detected; collect more rounds or run on a quieter machine"
            if not stable
            else ""
        ),
    }

    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return summary


def _build_runtime_cost_binary(root_dir: Path, profile: str) -> Path:
    manifest = root_dir / "demos/runtime_cost/Cargo.toml"
    command = ["cargo", "build", "--quiet", "--manifest-path", str(manifest)]
    if profile == "release":
        command.append("--release")
    subprocess.run(command, check=True)

    binary = root_dir / "target" / ("release" if profile == "release" else "debug") / "runtime_cost"
    if not binary.exists():
        raise SystemExit(f"runtime_cost binary not found after build: {binary}")
    return binary


def _run_binary(binary: Path, artifact_dir: Path, mode: str, requests: int, concurrency: int, work_ms: int) -> dict:
    result = subprocess.run(
        [
            str(binary),
            "--mode",
            mode,
            "--requests",
            str(requests),
            "--concurrency",
            str(concurrency),
            "--work-ms",
            str(work_ms),
            "--output-dir",
            str(artifact_dir),
        ],
        check=True,
        capture_output=True,
        text=True,
    )
    lines = [line for line in result.stdout.splitlines() if line.strip()]
    if not lines:
        raise SystemExit(f"runtime_cost emitted no output for mode={mode}")
    return json.loads(lines[-1])


def main() -> None:
    args = parse_args()
    profile = "release" if args.release else args.profile
    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = Path(args.artifact_dir)
    artifact_dir.mkdir(parents=True, exist_ok=True)

    if args.rounds <= 0:
        raise SystemExit("--rounds must be > 0")
    if args.warmup_rounds < 0:
        raise SystemExit("--warmup-rounds must be >= 0")

    binary = _build_runtime_cost_binary(root_dir, profile)

    raw_path = artifact_dir / "runtime-cost-raw.jsonl"
    summary_path = artifact_dir / "runtime-cost-summary.json"
    raw_path.write_text("", encoding="utf-8")

    total_rounds = args.warmup_rounds + args.rounds
    for round_index in range(total_rounds):
        phase = "warmup" if round_index < args.warmup_rounds else "measure"
        mode_order = list(MODES)
        if round_index % 2 == 1:
            mode_order.reverse()

        for mode in mode_order:
            measurement = _run_binary(
                binary,
                artifact_dir,
                mode,
                args.requests,
                args.concurrency,
                args.work_ms,
            )
            measurement["round"] = round_index
            measurement["phase"] = phase
            measurement["profile"] = profile
            with raw_path.open("a", encoding="utf-8") as raw_file:
                raw_file.write(json.dumps(measurement) + "\n")

    summary = summarize(raw_path, summary_path, profile=profile, warmup_rounds=args.warmup_rounds)
    print(json.dumps(summary, indent=2))
    print(f"raw results: {raw_path}")
    print(f"summary: {summary_path}")


if __name__ == "__main__":
    main()
