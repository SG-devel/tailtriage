#!/usr/bin/env python3
"""Measure and summarize runtime overhead for runtime-cost demo scenarios."""

from __future__ import annotations

import argparse
import json
import os
import statistics
import subprocess
import sys
from pathlib import Path

MODES = (
    "baseline",
    "baked_in_no_request_context",
    "core_light",
    "core_investigation",
    "core_light_tokio_sampler",
    "core_investigation_tokio_sampler",
    "core_light_drop_path",
    "core_investigation_drop_path",
    "tracing_light",
    "tracing_light_tokio_sampler",
    "tracing_light_drop_path",
)
UNSATURATED_CORE_MODES = ("core_light", "core_investigation")
SATURATED_DROP_PATH_MODES = ("core_light_drop_path", "core_investigation_drop_path")
TOKIO_SAMPLER_MODES = ("core_light_tokio_sampler", "core_investigation_tokio_sampler")
METRIC_KEYS = (
    "throughput_rps",
    "latency_p50_ms",
    "latency_p95_ms",
    "latency_p99_ms",
    "artifact_finalize_ms",
    "analyze_ms",
    "report_render_ms",
    "run_requests",
    "run_stages",
    "run_queues",
    "runtime_snapshots",
    "lifecycle_warning_count",
)
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

DELTA_VS_BASELINE_MODE_GROUPS: tuple[tuple[str, tuple[str, ...]], ...] = (
    ("Baked-in overhead", ("baked_in_no_request_context",)),
    ("Core mode overhead", UNSATURATED_CORE_MODES),
    ("Tokio mode overhead", TOKIO_SAMPLER_MODES),
    ("Post-limit / drop-path overhead", SATURATED_DROP_PATH_MODES),
)


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


def safe_ratio(comparison: float, reference: float) -> float | None:
    if reference <= 0:
        return None
    ratio = comparison / reference
    if ratio != ratio or ratio in (float("inf"), float("-inf")):
        return None
    return ratio


def paired_delta_rows(measured_rounds: list[dict], mode: str, metric: str) -> list[float]:
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


def paired_incremental_rows(
    measured_rounds: list[dict],
    base_mode: str,
    sampler_mode: str,
    metric: str,
) -> list[float]:
    values = []
    for round_rows in measured_rounds:
        base_value = round_rows[base_mode][metric]
        sampler_value = round_rows[sampler_mode][metric]
        if base_value <= 0:
            continue

        if metric == "throughput_rps":
            delta = ((base_value - sampler_value) / base_value) * 100.0
        else:
            delta = ((sampler_value - base_value) / base_value) * 100.0

        values.append(delta)

    return values


def summarize_mode_metrics(by_mode: dict[str, list[dict]], mode: str) -> dict:
    metrics = {key: [row[key] for row in by_mode[mode]] for key in METRIC_KEYS}
    truncations = [row.get("truncation") for row in by_mode[mode] if row.get("truncation") is not None]
    summary = {metric: summarize_values(values) for metric, values in metrics.items()}
    if truncations:
        summary["truncation"] = {
            "dropped_requests": summarize_values([entry["dropped_requests"] for entry in truncations]),
            "dropped_stages": summarize_values([entry["dropped_stages"] for entry in truncations]),
            "dropped_queues": summarize_values([entry["dropped_queues"] for entry in truncations]),
            "dropped_inflight_snapshots": summarize_values(
                [entry["dropped_inflight_snapshots"] for entry in truncations]
            ),
            "dropped_runtime_snapshots": summarize_values(
                [entry["dropped_runtime_snapshots"] for entry in truncations]
            ),
            "limit_reached_rounds": sum(1 for entry in truncations if entry["limits_reached"]),
        }
    summary["effective_tokio_sampler_config_present_rounds"] = sum(
        1 for row in by_mode[mode] if row.get("effective_tokio_sampler_config_present")
    )
    summary["inflight_supported"] = bool(by_mode[mode][0].get("inflight_supported"))
    summary["drop_path_signal_present_rounds"] = sum(
        1 for row in by_mode[mode] if row.get("drop_path_signal_present")
    )
    summary["artifact_path_last"] = by_mode[mode][-1].get("artifact_path")
    return summary


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
        throughput_cv = summary["absolute_metrics"][mode]["throughput_rps"]["cv"]
        p95_cv = summary["absolute_metrics"][mode]["latency_p95_ms"]["cv"]
        if throughput_cv >= 0.10:
            reasons.append(f"{mode} throughput CV is high ({throughput_cv:.3f} >= 0.100)")
        elif throughput_cv >= 0.05:
            reasons.append(f"{mode} throughput CV is elevated ({throughput_cv:.3f} >= 0.050)")
        if p95_cv >= 0.15:
            reasons.append(f"{mode} p95 CV is high ({p95_cv:.3f} >= 0.150)")
        elif p95_cv >= 0.08:
            reasons.append(f"{mode} p95 CV is elevated ({p95_cv:.3f} >= 0.080)")

    for _heading, modes in DELTA_VS_BASELINE_MODE_GROUPS:
        for mode in modes:
            throughput_deltas = paired_delta_rows(measured_rounds, mode, "throughput_rps")
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
        "absolute_metrics": {},
        "delta_vs_baseline_pct": {heading: {} for heading, _modes in DELTA_VS_BASELINE_MODE_GROUPS},
        "incremental_runtime_sampler_overhead_pct": {
            "Incremental runtime sampler overhead": {},
        },
        "tracing_vs_native_ratios": {},
    }

    for mode in MODES:
        summary["absolute_metrics"][mode] = summarize_mode_metrics(by_mode, mode)

    def baseline_delta(mode: str) -> dict:
        return {
            metric: summarize_values(paired_delta_rows(measured_rounds, mode, metric))
            for metric in METRIC_KEYS
        }

    for heading, modes in DELTA_VS_BASELINE_MODE_GROUPS:
        for mode in modes:
            summary["delta_vs_baseline_pct"][heading][mode] = baseline_delta(mode)

    summary["incremental_runtime_sampler_overhead_pct"]["Incremental runtime sampler overhead"] = {
        "light_mode": {
            metric: summarize_values(
                paired_incremental_rows(measured_rounds, "core_light", "core_light_tokio_sampler", metric)
            )
            for metric in METRIC_KEYS
        },
        "investigation_mode": {
            metric: summarize_values(
                paired_incremental_rows(
                    measured_rounds,
                    "core_investigation",
                    "core_investigation_tokio_sampler",
                    metric,
                )
            )
            for metric in METRIC_KEYS
        },
    }
    abs_m = summary["absolute_metrics"]
    summary["tracing_vs_native_ratios"] = {
        "core_light_vs_baseline_latency_p95": safe_ratio(
            abs_m["core_light"]["latency_p95_ms"]["median"], abs_m["baseline"]["latency_p95_ms"]["median"]
        ),
        "tracing_light_vs_baseline_latency_p95": safe_ratio(
            abs_m["tracing_light"]["latency_p95_ms"]["median"], abs_m["baseline"]["latency_p95_ms"]["median"]
        ),
        "tracing_light_vs_core_light_latency_p95": safe_ratio(
            abs_m["tracing_light"]["latency_p95_ms"]["median"], abs_m["core_light"]["latency_p95_ms"]["median"]
        ),
        "core_light_tokio_sampler_vs_core_light_latency_p95": safe_ratio(
            abs_m["core_light_tokio_sampler"]["latency_p95_ms"]["median"], abs_m["core_light"]["latency_p95_ms"]["median"]
        ),
        "tracing_light_tokio_sampler_vs_tracing_light_latency_p95": safe_ratio(
            abs_m["tracing_light_tokio_sampler"]["latency_p95_ms"]["median"], abs_m["tracing_light"]["latency_p95_ms"]["median"]
        ),
        "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95": safe_ratio(
            abs_m["tracing_light_tokio_sampler"]["latency_p95_ms"]["median"], abs_m["core_light_tokio_sampler"]["latency_p95_ms"]["median"]
        ),
        "tracing_light_drop_path_vs_core_light_drop_path_latency_p95": safe_ratio(
            abs_m["tracing_light_drop_path"]["latency_p95_ms"]["median"], abs_m["core_light_drop_path"]["latency_p95_ms"]["median"]
        ),
        "tracing_light_vs_core_light_throughput": safe_ratio(
            abs_m["tracing_light"]["throughput_rps"]["median"], abs_m["core_light"]["throughput_rps"]["median"]
        ),
        "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput": safe_ratio(
            abs_m["tracing_light_tokio_sampler"]["throughput_rps"]["median"], abs_m["core_light_tokio_sampler"]["throughput_rps"]["median"]
        ),
        "tracing_light_drop_path_vs_core_light_drop_path_throughput": safe_ratio(
            abs_m["tracing_light_drop_path"]["throughput_rps"]["median"], abs_m["core_light_drop_path"]["throughput_rps"]["median"]
        ),
        "tracing_finalize_vs_native_finalize": safe_ratio(
            abs_m["tracing_light"]["artifact_finalize_ms"]["median"], abs_m["core_light"]["artifact_finalize_ms"]["median"]
        ),
        "tracing_analyze_vs_native_analyze": safe_ratio(
            abs_m["tracing_light"]["analyze_ms"]["median"], abs_m["core_light"]["analyze_ms"]["median"]
        ),
        "tracing_render_vs_native_render": safe_ratio(
            abs_m["tracing_light"]["report_render_ms"]["median"], abs_m["core_light"]["report_render_ms"]["median"]
        ),
    }

    quality, reasons = assess_quality(summary, measured_rounds)
    summary["measurement_quality"] = quality
    summary["stability_warning"] = None if quality == QUALITY_STABLE else reasons
    _validate_sanity(summary)

    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    return summary


def _validate_sanity(summary: dict) -> None:
    abs_m = summary["absolute_metrics"]
    required = ("core_light", "tracing_light", "tracing_light_tokio_sampler")
    for mode in MODES:
        if abs_m[mode]["throughput_rps"]["median"] <= 0:
            raise SystemExit(f"{mode} throughput must be > 0")
        if abs_m[mode]["latency_p95_ms"]["median"] <= 0:
            raise SystemExit(f"{mode} p95 must be > 0")
    for mode in required:
        if abs_m[mode]["run_requests"]["median"] <= 0 or abs_m[mode]["run_stages"]["median"] <= 0 or abs_m[mode]["run_queues"]["median"] <= 0:
            raise SystemExit(f"{mode} must record request/stage/queue evidence")
    if abs_m["tracing_light_tokio_sampler"]["runtime_snapshots"]["median"] <= 0:
        raise SystemExit("tracing_light_tokio_sampler must have runtime snapshots")
    if abs_m["tracing_light_tokio_sampler"]["effective_tokio_sampler_config_present_rounds"] <= 0:
        raise SystemExit("tracing_light_tokio_sampler must include sampler metadata")
    if abs_m["tracing_light_drop_path"]["drop_path_signal_present_rounds"] <= 0:
        raise SystemExit("tracing_light_drop_path must include drop-path signal")
    ratios = summary["tracing_vs_native_ratios"]
    required_ratio_keys = (
        "tracing_light_vs_core_light_latency_p95",
        "tracing_light_vs_core_light_throughput",
        "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95",
        "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput",
        "tracing_light_drop_path_vs_core_light_drop_path_latency_p95",
        "tracing_light_drop_path_vs_core_light_drop_path_throughput",
    )
    for key in required_ratio_keys:
        value = ratios.get(key)
        if value is None:
            raise SystemExit(f"{key} is required and cannot be null (likely zero/missing denominator)")
        if value != value or value in (float("inf"), float("-inf")):
            raise SystemExit(f"{key} must be finite (not NaN or infinity)")
    if ratios["tracing_light_vs_core_light_latency_p95"] > 20:
        raise SystemExit("tracing_light_vs_core_light_latency_p95 exceeds catastrophic threshold (>20x)")
    if ratios["tracing_light_vs_core_light_throughput"] < 0.05:
        raise SystemExit("tracing_light_vs_core_light_throughput is below catastrophic threshold (<0.05x)")
    if ratios["tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95"] > 20:
        raise SystemExit(
            "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95 exceeds catastrophic threshold (>20x)"
        )
    if ratios["tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput"] < 0.05:
        raise SystemExit(
            "tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput is below catastrophic threshold (<0.05x)"
        )
    if ratios["tracing_light_drop_path_vs_core_light_drop_path_latency_p95"] > 20:
        raise SystemExit(
            "tracing_light_drop_path_vs_core_light_drop_path_latency_p95 exceeds catastrophic threshold (>20x)"
        )
    if ratios["tracing_light_drop_path_vs_core_light_drop_path_throughput"] < 0.05:
        raise SystemExit(
            "tracing_light_drop_path_vs_core_light_drop_path_throughput is below catastrophic threshold (<0.05x)"
        )


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


def rotating_mode_order(round_number: int) -> tuple[str, ...]:
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
    ratios = summary.get("tracing_vs_native_ratios", {})
    rows = (
        (
            "tracing_light / core_light",
            ratios.get("tracing_light_vs_core_light_latency_p95"),
            ratios.get("tracing_light_vs_core_light_throughput"),
        ),
        (
            "tracing_sampler / native_sampler",
            ratios.get("tracing_light_tokio_sampler_vs_core_light_tokio_sampler_latency_p95"),
            ratios.get("tracing_light_tokio_sampler_vs_core_light_tokio_sampler_throughput"),
        ),
        (
            "tracing_drop_path / native_drop_path",
            ratios.get("tracing_light_drop_path_vs_core_light_drop_path_latency_p95"),
            ratios.get("tracing_light_drop_path_vs_core_light_drop_path_throughput"),
        ),
    )
    print("| comparison | p95 ratio | throughput ratio |")
    print("|---|---:|---:|")
    for name, p95_ratio, throughput_ratio in rows:
        p95_text = "n/a" if p95_ratio is None else f"{p95_ratio:.2f}x"
        throughput_text = "n/a" if throughput_ratio is None else f"{throughput_ratio:.2f}x"
        print(f"| {name} | {p95_text} | {throughput_text} |")
    print(f"raw results: {raw_path}")
    print(f"summary: {summary_path}")


if __name__ == "__main__":
    main()
