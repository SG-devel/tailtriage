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
DEFAULT_REQUESTS = 6000
DEFAULT_CONCURRENCY = 64
DEFAULT_WORK_MS = 3
DEFAULT_ROUNDS = 6
DEFAULT_WARMUP_ROUNDS = 2
QUALITY_STABLE = "stable"
QUALITY_NOISY = "noisy"
QUALITY_UNSTABLE = "unstable"
QUALITY_INSUFFICIENT_DATA = "insufficient_data"
MIN_ROUNDS_FOR_STABLE = 4


def parse_args() -> argparse.Namespace:
    root_dir = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Measure runtime overhead for demo modes.")
    parser.add_argument(
        "--artifact-dir",
        default=str(root_dir / "demos/runtime_cost/artifacts"),
        help="Directory for raw and summary output files.",
    )
    parser.add_argument("--requests", type=int, default=int(os.environ.get("REQUESTS", str(DEFAULT_REQUESTS))))
    parser.add_argument("--concurrency", type=int, default=int(os.environ.get("CONCURRENCY", str(DEFAULT_CONCURRENCY))))
    parser.add_argument("--work-ms", type=int, default=int(os.environ.get("WORK_MS", str(DEFAULT_WORK_MS))))
    parser.add_argument("--rounds", type=int, default=int(os.environ.get("ROUNDS", str(DEFAULT_ROUNDS))))
    parser.add_argument("--warmup-rounds", type=int, default=int(os.environ.get("WARMUP_ROUNDS", str(DEFAULT_WARMUP_ROUNDS))))
    return parser.parse_args()


def summarize_values(values: list[float]) -> dict[str, float]:
    if not values:
        return {
            "mean": 0.0,
            "median": 0.0,
            "min": 0.0,
            "max": 0.0,
            "stdev": 0.0,
            "cv": 0.0,
        }

    mean = statistics.fmean(values)
    stdev = statistics.stdev(values) if len(values) > 1 else 0.0
    return {
        "mean": mean,
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": stdev,
        "cv": stdev / abs(mean) if mean else 0.0,
    }


def paired_overhead_rows(measured_rounds: list[dict], mode: str, metric: str) -> list[float]:
    values = []
    for round_rows in measured_rounds:
        baseline = round_rows["baseline"][metric]
        target = round_rows[mode][metric]
        if baseline <= 0:
            continue

        if metric == "throughput_rps":
            delta = ((baseline - target) / baseline) * 100.0
        else:
            delta = ((target - baseline) / baseline) * 100.0

        values.append(delta)

    return values


def assess_quality(summary: dict, measured_rounds: list[dict]) -> tuple[str, list[str]]:
    measured_round_count = len(measured_rounds)
    if measured_round_count < MIN_ROUNDS_FOR_STABLE:
        return (
            QUALITY_INSUFFICIENT_DATA,
            [
                (
                    "fewer than minimum measured rounds for stable classification "
                    f"({measured_round_count} < {MIN_ROUNDS_FOR_STABLE})"
                )
            ],
        )

    reasons: list[str] = []

    for mode in MODES:
        throughput_cv = summary["modes"][mode]["throughput_rps"]["cv"]
        p95_cv = summary["modes"][mode]["latency_p95_ms"]["cv"]
        if throughput_cv >= 0.10:
            reasons.append(f"{mode} throughput CV is high ({throughput_cv:.3f} >= 0.100)")
        elif throughput_cv >= 0.05:
            reasons.append(f"{mode} throughput CV is elevated ({throughput_cv:.3f} >= 0.050)")
        if p95_cv >= 0.15:
            reasons.append(f"{mode} p95 CV is high ({p95_cv:.3f} >= 0.150)")
        elif p95_cv >= 0.08:
            reasons.append(f"{mode} p95 CV is elevated ({p95_cv:.3f} >= 0.080)")

    for mode in ("light", "investigation"):
        throughput_deltas = paired_overhead_rows(measured_rounds, mode, "throughput_rps")
        crossing = 0
        for idx in range(1, len(throughput_deltas)):
            prev, cur = throughput_deltas[idx - 1], throughput_deltas[idx]
            if prev == 0 or cur == 0:
                continue
            if (prev < 0 < cur) or (prev > 0 > cur):
                crossing += 1
        if throughput_deltas and crossing / len(throughput_deltas) >= 0.4:
            reasons.append(
                f"{mode} paired throughput overhead crosses zero frequently ({crossing}/{len(throughput_deltas)})"
            )

    if any("high" in reason for reason in reasons):
        return QUALITY_UNSTABLE, reasons
    if reasons:
        return QUALITY_NOISY, reasons
    return QUALITY_STABLE, ["Measured rounds are within configured variance thresholds."]


def summarize(raw_path: Path, summary_path: Path) -> dict:
    rows = [json.loads(line) for line in raw_path.read_text(encoding="utf-8").splitlines() if line.strip()]
    measured = [row for row in rows if not row["is_warmup"]]
    by_mode: dict[str, list[dict]] = {mode: [] for mode in MODES}

    for row in measured:
        by_mode[row["mode"]].append(row)

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
        "requests": by_mode["baseline"][0]["requests"],
        "concurrency": by_mode["baseline"][0]["concurrency"],
        "work_ms": by_mode["baseline"][0]["work_ms"],
        "warmup_rounds": len({row["round"] for row in rows if row["is_warmup"]}),
        "measured_rounds": len(measured_rounds),
        "samples_per_mode": {mode: len(by_mode[mode]) for mode in MODES},
        "minimum_rounds_for_stable": MIN_ROUNDS_FOR_STABLE,
        "round_ordering": "interleaved_rotating",
        "execution_profile": "release_binary",
        "modes": {},
        "paired_overhead_pct_vs_baseline": {},
    }

    for mode in MODES:
        metrics = {key: [row[key] for row in by_mode[mode]] for key in METRIC_KEYS}
        summary["modes"][mode] = {
            metric: summarize_values(values) for metric, values in metrics.items()
        }

    for mode in ("light", "investigation"):
        summary["paired_overhead_pct_vs_baseline"][mode] = {
            "throughput_rps": summarize_values(paired_overhead_rows(measured_rounds, mode, "throughput_rps")),
            "latency_p50_ms": summarize_values(paired_overhead_rows(measured_rounds, mode, "latency_p50_ms")),
            "latency_p95_ms": summarize_values(paired_overhead_rows(measured_rounds, mode, "latency_p95_ms")),
            "latency_p99_ms": summarize_values(paired_overhead_rows(measured_rounds, mode, "latency_p99_ms")),
        }

    quality, reasons = assess_quality(summary, measured_rounds)
    summary["measurement_quality"] = quality
    summary["stability_warning"] = None if quality == QUALITY_STABLE else reasons

    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return summary


def build_release_binary(root_dir: Path) -> Path:
    manifest_path = root_dir / "demos/runtime_cost/Cargo.toml"
    print("Building runtime_cost demo in release mode...", file=sys.stderr)
    subprocess.run(
        [
            "cargo",
            "build",
            "--release",
            "--quiet",
            "--manifest-path",
            str(manifest_path),
        ],
        check=True,
    )

    binary_name = "runtime_cost.exe" if os.name == "nt" else "runtime_cost"
    binary_path = root_dir / "target/release" / binary_name
    if not binary_path.exists():
        raise SystemExit(f"release binary not found at {binary_path}")
    return binary_path


def rotating_mode_order(round_number: int) -> tuple[str, str, str]:
    offset = round_number % len(MODES)
    return MODES[offset:] + MODES[:offset]


def run_mode(binary_path: Path, mode: str, args: argparse.Namespace, artifact_dir: Path) -> dict:
    result = subprocess.run(
        [
            str(binary_path),
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

    output = result.stdout.strip().splitlines()
    if not output:
        raise SystemExit(f"missing measurement output for mode: {mode}")

    return json.loads(output[-1])


def main() -> None:
    args = parse_args()
    if args.requests <= 0 or args.concurrency <= 0 or args.work_ms <= 0:
        raise SystemExit("--requests, --concurrency, and --work-ms must all be > 0")
    if args.rounds <= 0:
        raise SystemExit("--rounds must be > 0")
    if args.warmup_rounds < 0:
        raise SystemExit("--warmup-rounds must be >= 0")

    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = Path(args.artifact_dir)
    artifact_dir.mkdir(parents=True, exist_ok=True)

    binary_path = build_release_binary(root_dir)

    raw_path = artifact_dir / "runtime-cost-raw.jsonl"
    summary_path = artifact_dir / "runtime-cost-summary.json"
    raw_path.write_text("", encoding="utf-8")

    total_rounds = args.warmup_rounds + args.rounds
    for round_number in range(total_rounds):
        is_warmup = round_number < args.warmup_rounds
        phase = "warmup" if is_warmup else "measured"
        mode_order = rotating_mode_order(round_number)

        print(
            f"round={round_number + 1}/{total_rounds} phase={phase} order={','.join(mode_order)}",
            file=sys.stderr,
        )
        for mode in mode_order:
            measurement = run_mode(binary_path, mode, args, artifact_dir)
            measurement["round"] = round_number
            measurement["phase"] = phase
            measurement["is_warmup"] = is_warmup
            with raw_path.open("a", encoding="utf-8") as raw_file:
                raw_file.write(json.dumps(measurement) + "\n")

    summary = summarize(raw_path, summary_path)
    if summary["measurement_quality"] != QUALITY_STABLE:
        print(
            f"WARNING: measurement quality is {summary['measurement_quality']}; rerun on a quieter machine for stronger conclusions.",
            file=sys.stderr,
        )
        for reason in summary["stability_warning"] or []:
            print(f" - {reason}", file=sys.stderr)

    print(json.dumps(summary, indent=2))
    print(f"raw results: {raw_path}")
    print(f"summary: {summary_path}")


if __name__ == "__main__":
    main()
