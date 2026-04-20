#!/usr/bin/env python3
"""Orchestrate collector-stress limit measurements with machine-scoped summaries."""

from __future__ import annotations

import argparse
import json
import os
import shutil
import statistics
import subprocess
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import Any

MODES: tuple[str, ...] = (
    "baseline",
    "core_light",
    "core_investigation",
    "core_light_tokio_sampler",
    "core_investigation_tokio_sampler",
)
SAMPLER_MODES: tuple[str, ...] = (
    "core_light_tokio_sampler",
    "core_investigation_tokio_sampler",
)


@dataclass(frozen=True)
class Case:
    case_id: str
    description: str
    concurrency: int
    duration_secs: int
    queues_per_request: int
    stages_per_request: int
    inflight_cycles_per_request: int
    work_ms: int
    requests: int | None = None
    queue_slots: int | None = None
    sampler_interval_ms: int | None = None
    sampler_max_runtime_snapshots: int | None = None
    modes: tuple[str, ...] = MODES


def case_payload(case: Case) -> dict[str, Any]:
    return {
        "case_id": case.case_id,
        "description": case.description,
        "concurrency": case.concurrency,
        "duration_secs": case.duration_secs,
        "queues_per_request": case.queues_per_request,
        "stages_per_request": case.stages_per_request,
        "inflight_cycles_per_request": case.inflight_cycles_per_request,
        "work_ms": case.work_ms,
        "requests": case.requests,
        "queue_slots": case.queue_slots,
        "sampler_interval_ms": case.sampler_interval_ms,
        "sampler_max_runtime_snapshots": case.sampler_max_runtime_snapshots,
        "modes": list(case.modes),
    }


DEFAULT_CASES: tuple[Case, ...] = (
    Case(
        case_id="baseline_shape",
        description="Reference shape for cross-mode comparison.",
        concurrency=128,
        duration_secs=20,
        queues_per_request=3,
        stages_per_request=4,
        inflight_cycles_per_request=6,
        work_ms=2,
    ),
    Case(
        case_id="high_concurrency",
        description="Higher concurrency with the same event shape to surface throughput/latency pressure.",
        concurrency=256,
        duration_secs=20,
        queues_per_request=3,
        stages_per_request=4,
        inflight_cycles_per_request=6,
        work_ms=2,
    ),
    Case(
        case_id="heavy_event_shape",
        description="Denser request event shape to expose artifact and truncation growth.",
        concurrency=128,
        duration_secs=20,
        queues_per_request=8,
        stages_per_request=8,
        inflight_cycles_per_request=10,
        work_ms=2,
    ),
    Case(
        case_id="longer_run",
        description="Longer sustained run to expose event-volume and memory growth.",
        concurrency=128,
        duration_secs=45,
        queues_per_request=3,
        stages_per_request=4,
        inflight_cycles_per_request=6,
        work_ms=2,
    ),
    Case(
        case_id="sampler_dense",
        description="Denser runtime sampler cadence impact versus baseline sampler cadence.",
        concurrency=128,
        duration_secs=20,
        queues_per_request=3,
        stages_per_request=4,
        inflight_cycles_per_request=6,
        work_ms=2,
        sampler_interval_ms=50,
        modes=SAMPLER_MODES,
    ),
)

SMOKE_CASES: tuple[Case, ...] = (
    Case(
        case_id="smoke_baseline_shape",
        description="Small matrix sanity check across all modes.",
        concurrency=16,
        duration_secs=3,
        queues_per_request=2,
        stages_per_request=2,
        inflight_cycles_per_request=2,
        work_ms=1,
        requests=400,
    ),
    Case(
        case_id="smoke_sampler_dense",
        description="Small sampler-density check limited to sampler-enabled modes.",
        concurrency=16,
        duration_secs=3,
        queues_per_request=2,
        stages_per_request=2,
        inflight_cycles_per_request=2,
        work_ms=1,
        requests=400,
        sampler_interval_ms=30,
        modes=SAMPLER_MODES,
    ),
)


def parse_args() -> argparse.Namespace:
    root_dir = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Measure collector stress limits with machine-scoped summaries.")
    parser.add_argument(
        "--artifact-dir",
        default=str(root_dir / "demos/collector_stress/artifacts"),
        help="Directory for raw and summary output files.",
    )
    parser.add_argument(
        "--profile",
        choices=("default", "smoke"),
        default="default",
        help="default runs the full documented matrix; smoke runs a bounded quick matrix for CI/dev checks.",
    )
    parser.add_argument(
        "--repeats",
        type=int,
        default=1,
        help="Repeat each case/mode combination this many times.",
    )
    parser.add_argument(
        "--modes",
        default=",".join(MODES),
        help="Comma-separated subset of modes to execute.",
    )
    return parser.parse_args()


def parse_modes(raw_modes: str) -> tuple[str, ...]:
    selected = tuple(mode.strip() for mode in raw_modes.split(",") if mode.strip())
    if not selected:
        raise SystemExit("--modes must include at least one mode")
    unknown = sorted(set(selected) - set(MODES))
    if unknown:
        raise SystemExit(f"unsupported mode(s): {', '.join(unknown)}")
    return selected


def build_release_binary(root_dir: Path) -> Path:
    manifest_path = root_dir / "demos/collector_stress/Cargo.toml"
    print("Building collector_stress demo in release mode...", file=sys.stderr)
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

    binary_name = "collector_stress.exe" if os.name == "nt" else "collector_stress"
    binary_path = root_dir / "target/release" / binary_name
    if not binary_path.exists():
        raise SystemExit(f"release binary not found at {binary_path}")
    return binary_path


def parse_time_max_rss_bytes(time_output: str) -> int | None:
    for line in time_output.splitlines():
        if "Maximum resident set size" not in line:
            continue
        parts = line.split(":", maxsplit=1)
        if len(parts) != 2:
            continue
        value = parts[1].strip()
        if not value:
            continue
        try:
            return int(value) * 1024
        except ValueError:
            return None
    return None


def run_measurement(binary_path: Path, artifact_dir: Path, mode: str, case: Case) -> tuple[dict[str, Any], dict[str, Any]]:
    cmd = [
        str(binary_path),
        "--mode",
        mode,
        "--concurrency",
        str(case.concurrency),
        "--duration-secs",
        str(case.duration_secs),
        "--queues-per-request",
        str(case.queues_per_request),
        "--stages-per-request",
        str(case.stages_per_request),
        "--inflight-cycles-per-request",
        str(case.inflight_cycles_per_request),
        "--work-ms",
        str(case.work_ms),
        "--output-dir",
        str(artifact_dir),
    ]
    if case.queue_slots is not None:
        cmd.extend(["--queue-slots", str(case.queue_slots)])
    if case.requests is not None:
        cmd.extend(["--requests", str(case.requests)])
    if case.sampler_interval_ms is not None:
        cmd.extend(["--sampler-interval-ms", str(case.sampler_interval_ms)])
    if case.sampler_max_runtime_snapshots is not None:
        cmd.extend(["--sampler-max-runtime-snapshots", str(case.sampler_max_runtime_snapshots)])

    mem_meta: dict[str, Any] = {
        "path": "in_process_fallback",
        "notes": [],
        "external_peak_rss_bytes": None,
    }

    time_bin = shutil.which("/usr/bin/time") or shutil.which("time")
    if time_bin:
        with tempfile.NamedTemporaryFile(mode="w+", encoding="utf-8", delete=False) as tmp:
            tmp_path = Path(tmp.name)
        try:
            completed = subprocess.run(
                [time_bin, "-v", "-o", str(tmp_path), *cmd],
                check=True,
                capture_output=True,
                text=True,
            )
            time_text = tmp_path.read_text(encoding="utf-8")
            rss = parse_time_max_rss_bytes(time_text)
            if rss is not None:
                mem_meta["path"] = "external_time_v"
                mem_meta["external_peak_rss_bytes"] = rss
            else:
                mem_meta["notes"].append(
                    "Could not parse maximum RSS from /usr/bin/time -v output; using in-process fallback where available."
                )
        finally:
            tmp_path.unlink(missing_ok=True)
    else:
        completed = subprocess.run(cmd, check=True, capture_output=True, text=True)
        mem_meta["notes"].append(
            "No compatible `time -v` binary found; using in-process memory fields when available."
        )

    lines = [line for line in completed.stdout.splitlines() if line.strip()]
    if not lines:
        raise SystemExit("collector_stress produced no JSON output")

    measurement = json.loads(lines[-1])

    artifact_meta = {
        "artifact_path": measurement.get("artifact", {}).get("artifact_path"),
        "size_bytes_reported_by_binary": measurement.get("artifact", {}).get("artifact_size_bytes"),
        "size_bytes_measured_by_script": None,
    }
    artifact_path = artifact_meta["artifact_path"]
    if artifact_path:
        artifact_file = Path(artifact_path)
        if artifact_file.exists():
            artifact_meta["size_bytes_measured_by_script"] = artifact_file.stat().st_size
        else:
            mem_meta["notes"].append(
                f"Artifact path reported but not found on disk: {artifact_path}."
            )

    return measurement, {"memory": mem_meta, "artifact": artifact_meta}


def summarize_values(values: list[float]) -> dict[str, float | None]:
    if not values:
        return {
            "count": 0,
            "mean": None,
            "median": None,
            "min": None,
            "max": None,
            "stdev": None,
        }
    return {
        "count": len(values),
        "mean": statistics.fmean(values),
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else 0.0,
    }


def group_rows(rows: list[dict[str, Any]]) -> dict[str, list[dict[str, Any]]]:
    grouped: dict[str, list[dict[str, Any]]] = {}
    for row in rows:
        key = f"{row['case_id']}::{row['mode']}"
        grouped.setdefault(key, []).append(row)
    return grouped


def pct_delta(base: float | None, observed: float | None) -> float | None:
    if base is None or observed is None or base == 0:
        return None
    return ((observed - base) / base) * 100.0


def signal_for_mode(summary_by_case_mode: dict[str, Any], mode: str) -> dict[str, Any]:
    base_key = f"baseline_shape::{mode}"
    high_key = f"high_concurrency::{mode}"
    heavy_key = f"heavy_event_shape::{mode}"
    long_key = f"longer_run::{mode}"

    def get_metric(key: str, path: tuple[str, ...]) -> float | None:
        node = summary_by_case_mode.get(key)
        if node is None:
            return None
        value: Any = node
        for item in path:
            value = value.get(item)
            if value is None:
                return None
        if isinstance(value, (int, float)):
            return float(value)
        return None

    base_tp = get_metric(base_key, ("absolute_metrics", "throughput_rps", "mean"))
    high_tp = get_metric(high_key, ("absolute_metrics", "throughput_rps", "mean"))
    base_p95 = get_metric(base_key, ("absolute_metrics", "latency_p95_ms", "mean"))
    high_p95 = get_metric(high_key, ("absolute_metrics", "latency_p95_ms", "mean"))

    base_art = get_metric(base_key, ("artifact_size", "size_bytes_measured_by_script", "mean"))
    heavy_art = get_metric(heavy_key, ("artifact_size", "size_bytes_measured_by_script", "mean"))

    base_mem = get_metric(base_key, ("memory", "peak_rss_bytes", "mean"))
    long_mem = get_metric(long_key, ("memory", "peak_rss_bytes", "mean"))

    trunc_limits = {
        "baseline_shape": get_metric(base_key, ("truncation", "limits_hit_runs")),
        "high_concurrency": get_metric(high_key, ("truncation", "limits_hit_runs")),
        "heavy_event_shape": get_metric(heavy_key, ("truncation", "limits_hit_runs")),
        "longer_run": get_metric(long_key, ("truncation", "limits_hit_runs")),
    }

    return {
        "mode": mode,
        "throughput_delta_high_concurrency_pct": pct_delta(base_tp, high_tp),
        "latency_p95_delta_high_concurrency_pct": pct_delta(base_p95, high_p95),
        "artifact_growth_heavy_event_shape_pct": pct_delta(base_art, heavy_art),
        "peak_rss_growth_longer_run_pct": pct_delta(base_mem, long_mem),
        "limits_hit_runs_by_case": trunc_limits,
    }


def summarize(rows: list[dict[str, Any]], profile: str, selected_modes: tuple[str, ...], cases: tuple[Case, ...]) -> dict[str, Any]:
    grouped = group_rows(rows)
    by_case_mode: dict[str, Any] = {}

    for key, entries in grouped.items():
        first = entries[0]
        by_case_mode[key] = {
            "case_id": first["case_id"],
            "mode": first["mode"],
            "event_shape": first["event_shape"],
            "sampler_settings": first.get("sampler_settings"),
            "absolute_metrics": {
                "requests_completed": summarize_values([float(entry["requests_completed"]) for entry in entries]),
                "run_duration_secs": summarize_values([float(entry["run_duration_secs"]) for entry in entries]),
                "throughput_rps": summarize_values([float(entry["throughput_rps"]) for entry in entries]),
                "latency_p50_ms": summarize_values([float(entry["latency"]["p50_ms"]) for entry in entries]),
                "latency_p95_ms": summarize_values([float(entry["latency"]["p95_ms"]) for entry in entries]),
                "latency_p99_ms": summarize_values([float(entry["latency"]["p99_ms"]) for entry in entries]),
                "latency_max_ms": summarize_values([float(entry["latency"]["max_ms"]) for entry in entries]),
            },
            "artifact_size": {
                "size_bytes_measured_by_script": summarize_values(
                    [
                        float(entry["script_artifact"]["size_bytes_measured_by_script"])
                        for entry in entries
                        if entry["script_artifact"]["size_bytes_measured_by_script"] is not None
                    ]
                ),
                "size_bytes_reported_by_binary": summarize_values(
                    [
                        float(entry["script_artifact"]["size_bytes_reported_by_binary"])
                        for entry in entries
                        if entry["script_artifact"]["size_bytes_reported_by_binary"] is not None
                    ]
                ),
            },
            "memory": {
                "path_usage": {
                    "external_time_v_runs": sum(
                        1 for entry in entries if entry["memory_measurement"]["path"] == "external_time_v"
                    ),
                    "in_process_fallback_runs": sum(
                        1 for entry in entries if entry["memory_measurement"]["path"] == "in_process_fallback"
                    ),
                },
                "peak_rss_bytes": summarize_values(
                    [
                        float(entry["memory_measurement"]["external_peak_rss_bytes"])
                        for entry in entries
                        if entry["memory_measurement"]["external_peak_rss_bytes"] is not None
                    ]
                    + [
                        float(entry["peak_memory"]["collector_peak_rss_bytes"])
                        for entry in entries
                        if entry["memory_measurement"]["external_peak_rss_bytes"] is None
                        and entry["peak_memory"].get("collector_peak_rss_bytes") is not None
                    ]
                ),
                "collector_end_rss_bytes": summarize_values(
                    [
                        float(entry["peak_memory"]["collector_end_rss_bytes"])
                        for entry in entries
                        if entry["peak_memory"].get("collector_end_rss_bytes") is not None
                    ]
                ),
            },
            "truncation": {
                "limits_hit_runs": sum(1 for entry in entries if entry["truncation_counts"]["limits_hit"]),
                "dropped_requests": summarize_values([float(entry["truncation_counts"]["dropped_requests"]) for entry in entries]),
                "dropped_stages": summarize_values([float(entry["truncation_counts"]["dropped_stages"]) for entry in entries]),
                "dropped_queues": summarize_values([float(entry["truncation_counts"]["dropped_queues"]) for entry in entries]),
                "dropped_inflight_snapshots": summarize_values(
                    [float(entry["truncation_counts"]["dropped_inflight_snapshots"]) for entry in entries]
                ),
                "dropped_runtime_snapshots": summarize_values(
                    [float(entry["truncation_counts"]["dropped_runtime_snapshots"]) for entry in entries]
                ),
            },
            "measurement_quality": {
                "caveats": sorted(
                    {
                        note
                        for entry in entries
                        for note in entry["memory_measurement"].get("notes", [])
                    }
                ),
                "collector_notes": sorted(
                    {
                        note
                        for entry in entries
                        for note in entry.get("measurement_notes", [])
                    }
                ),
            },
        }

    sampler_density_impact: dict[str, Any] = {}
    for mode in SAMPLER_MODES:
        if mode not in selected_modes:
            continue
        base_key = f"baseline_shape::{mode}"
        dense_key = f"sampler_dense::{mode}"
        base = by_case_mode.get(base_key)
        dense = by_case_mode.get(dense_key)
        if base is None or dense is None:
            continue

        sampler_density_impact[mode] = {
            "throughput_delta_pct": pct_delta(
                base["absolute_metrics"]["throughput_rps"]["mean"],
                dense["absolute_metrics"]["throughput_rps"]["mean"],
            ),
            "latency_p95_delta_pct": pct_delta(
                base["absolute_metrics"]["latency_p95_ms"]["mean"],
                dense["absolute_metrics"]["latency_p95_ms"]["mean"],
            ),
            "runtime_snapshot_drop_delta_pct": pct_delta(
                base["truncation"]["dropped_runtime_snapshots"]["mean"],
                dense["truncation"]["dropped_runtime_snapshots"]["mean"],
            ),
            "baseline_sampler_cadence_ms": base["sampler_settings"].get("resolved_sampler_cadence_ms") if base["sampler_settings"] else None,
            "dense_sampler_cadence_ms": dense["sampler_settings"].get("cli_interval_ms_override") if dense["sampler_settings"] else None,
        }

    mode_signals = [signal_for_mode(by_case_mode, mode) for mode in selected_modes]

    interpretation_notes = [
        "Machine-scoped measurements only; do not generalize these values to other hosts or workloads.",
        "Suspects are evidence-ranked leads for triage and are not proof of root cause.",
    ]

    steep_growth_findings = [
        signal
        for signal in mode_signals
        if signal.get("artifact_growth_heavy_event_shape_pct") is not None
        and signal["artifact_growth_heavy_event_shape_pct"] >= 100.0
    ]
    if steep_growth_findings:
        interpretation_notes.append(
            "Artifact size showed at least one >=100% heavy-event-shape jump; investigate collector/event retention pressure before raising limits."
        )
    else:
        interpretation_notes.append(
            "No defensible heavy-event-shape artifact inflection >=100% was observed in this run set."
        )

    concurrency_inflections = [
        signal
        for signal in mode_signals
        if signal.get("throughput_delta_high_concurrency_pct") is not None
        and signal.get("latency_p95_delta_high_concurrency_pct") is not None
        and signal["throughput_delta_high_concurrency_pct"] <= -20.0
        and signal["latency_p95_delta_high_concurrency_pct"] >= 20.0
    ]
    if concurrency_inflections:
        interpretation_notes.append(
            "At least one mode shows a visible concurrency inflection (throughput drop <=-20% with p95 rise >=20%)."
        )
    else:
        interpretation_notes.append(
            "No defensible concurrency breakpoint met the configured inflection rule in this matrix."
        )

    limitations = sorted(
        {
            note
            for row in rows
            for note in row["memory_measurement"].get("notes", [])
        }
    )

    return {
        "measurement_kind": "collector_limits",
        "profile": profile,
        "run_count": len(rows),
        "repeat_count": len({row["repeat"] for row in rows}),
        "mode_count": len(selected_modes),
        "modes": list(selected_modes),
        "case_count": len(cases),
        "default_matrix": [case_payload(case) for case in cases],
        "outputs": {
            "raw_format": {
                "record_type": "one JSON object per run",
                "fields": [
                    "run_id",
                    "repeat",
                    "case_id",
                    "case_description",
                    "mode",
                    "event_shape",
                    "sampler_settings",
                    "throughput_rps",
                    "latency",
                    "retained_counts",
                    "truncation_counts",
                    "artifact",
                    "peak_memory",
                    "memory_measurement",
                    "script_artifact",
                    "measurement_notes",
                ],
            },
            "summary_format": {
                "sections": [
                    "absolute metrics",
                    "artifact size summaries",
                    "memory summaries",
                    "truncation context",
                    "metadata and caveats",
                    "derived stress signals",
                ]
            },
        },
        "memory_measurement_paths": {
            "preferred": "external_time_v",
            "fallback": "in_process_fallback",
        },
        "cases_by_mode": by_case_mode,
        "sampler_density_impact": sampler_density_impact,
        "collector_stress_signals": {
            "per_mode": mode_signals,
            "collector_bottleneck_indicators": [
                "Throughput declines while p95 latency rises when moving from baseline_shape to high_concurrency.",
                "Artifact growth jumps in heavy_event_shape relative to baseline_shape.",
                "Memory peak RSS grows in longer_run relative to baseline_shape.",
                "Sampler dense runs degrade throughput/latency versus sampler baseline.",
                "limits_hit or dropped_* counters appear and persist across repeats/cases.",
            ],
        },
        "measurement_quality": {
            "limitations": limitations,
            "conservative_interpretation_notes": interpretation_notes,
        },
    }


def main() -> None:
    args = parse_args()
    if args.repeats <= 0:
        raise SystemExit("--repeats must be > 0")

    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = Path(args.artifact_dir)
    artifact_dir.mkdir(parents=True, exist_ok=True)

    selected_modes = parse_modes(args.modes)
    profile_cases = DEFAULT_CASES if args.profile == "default" else SMOKE_CASES

    cases = tuple(
        Case(**{**case.__dict__, "modes": tuple(mode for mode in case.modes if mode in selected_modes)})
        for case in profile_cases
    )
    cases = tuple(case for case in cases if case.modes)

    if not cases:
        raise SystemExit("selected --modes removed all matrix cases")

    binary_path = build_release_binary(root_dir)

    raw_path = artifact_dir / f"collector-limits-{args.profile}-raw.jsonl"
    summary_path = artifact_dir / f"collector-limits-{args.profile}-summary.json"
    raw_path.write_text("", encoding="utf-8")

    rows: list[dict[str, Any]] = []
    run_index = 0

    for repeat in range(args.repeats):
        for case in cases:
            for mode in case.modes:
                run_index += 1
                print(
                    f"run={run_index} repeat={repeat + 1}/{args.repeats} case={case.case_id} mode={mode}",
                    file=sys.stderr,
                )
                measurement, extras = run_measurement(binary_path, artifact_dir, mode, case)
                row = {
                    "run_id": run_index,
                    "repeat": repeat,
                    "case_id": case.case_id,
                    "case_description": case.description,
                    **measurement,
                    "memory_measurement": extras["memory"],
                    "script_artifact": extras["artifact"],
                }
                rows.append(row)
                with raw_path.open("a", encoding="utf-8") as f:
                    f.write(json.dumps(row) + "\n")

    summary = summarize(rows, args.profile, selected_modes, cases)
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

    print(json.dumps(summary, indent=2))
    print(f"raw results: {raw_path}")
    print(f"summary: {summary_path}")


if __name__ == "__main__":
    main()
