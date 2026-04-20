#!/usr/bin/env python3
"""Run collector-stress mode/shape matrices and summarize sustained collector behavior."""

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
    "core_light",
    "core_investigation",
    "core_light_tokio_sampler",
    "core_investigation_tokio_sampler",
)

DEFAULT_CONCURRENCY = "128,256"
DEFAULT_DURATION_SECS = "30"
DEFAULT_QUEUES_PER_REQUEST = "3,6"
DEFAULT_STAGES_PER_REQUEST = "4"
DEFAULT_INFLIGHT_TRANSITIONS = "6"
DEFAULT_WORK_MS = "2"
DEFAULT_REPEATS = 2


def parse_csv_ints(value: str, name: str) -> list[int]:
    values = []
    for part in value.split(","):
        stripped = part.strip()
        if not stripped:
            continue
        try:
            parsed = int(stripped)
        except ValueError as err:
            raise SystemExit(f"invalid integer in --{name}: {stripped}") from err
        if parsed <= 0:
            raise SystemExit(f"--{name} values must be > 0")
        values.append(parsed)

    if not values:
        raise SystemExit(f"--{name} must include at least one integer value")
    return values


def parse_args() -> argparse.Namespace:
    root_dir = Path(__file__).resolve().parent.parent
    parser = argparse.ArgumentParser(description="Measure collector stress behavior.")
    parser.add_argument(
        "--artifact-dir",
        default=str(root_dir / "demos/collector_stress/artifacts"),
        help="Directory for raw and summary output files.",
    )
    parser.add_argument("--modes", default=",".join(MODES), help="Comma-separated modes to execute.")
    parser.add_argument(
        "--concurrency-matrix",
        default=os.environ.get("COLLECTOR_STRESS_CONCURRENCY", DEFAULT_CONCURRENCY),
        help="Comma-separated concurrency values.",
    )
    parser.add_argument(
        "--duration-secs-matrix",
        default=os.environ.get("COLLECTOR_STRESS_DURATION", DEFAULT_DURATION_SECS),
        help="Comma-separated duration values in seconds.",
    )
    parser.add_argument(
        "--queues-per-request-matrix",
        default=os.environ.get("COLLECTOR_STRESS_QUEUES", DEFAULT_QUEUES_PER_REQUEST),
        help="Comma-separated queue events per request.",
    )
    parser.add_argument(
        "--stages-per-request-matrix",
        default=os.environ.get("COLLECTOR_STRESS_STAGES", DEFAULT_STAGES_PER_REQUEST),
        help="Comma-separated stage events per request.",
    )
    parser.add_argument(
        "--inflight-transitions-matrix",
        default=os.environ.get("COLLECTOR_STRESS_INFLIGHT", DEFAULT_INFLIGHT_TRANSITIONS),
        help="Comma-separated inflight transition counts per request.",
    )
    parser.add_argument(
        "--work-ms-matrix",
        default=os.environ.get("COLLECTOR_STRESS_WORK_MS", DEFAULT_WORK_MS),
        help="Comma-separated simulated work duration values in milliseconds.",
    )
    parser.add_argument(
        "--queue-slots",
        type=int,
        default=None,
        help="Explicit queue slots for all matrix runs; defaults to max(concurrency/2,1) in binary.",
    )
    parser.add_argument(
        "--max-requests",
        type=int,
        default=None,
        help="Optional hard stop for requests per run.",
    )
    parser.add_argument("--repeats", type=int, default=DEFAULT_REPEATS, help="Repeated samples per mode/shape.")
    return parser.parse_args()


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


def mode_list(raw_modes: str) -> list[str]:
    modes = [mode.strip() for mode in raw_modes.split(",") if mode.strip()]
    if not modes:
        raise SystemExit("--modes must include at least one mode")
    unknown = sorted(set(modes) - set(MODES))
    if unknown:
        raise SystemExit(f"unsupported mode(s): {', '.join(unknown)}")
    return modes


def run_case(
    binary_path: Path,
    artifact_dir: Path,
    mode: str,
    concurrency: int,
    duration_secs: int,
    queues_per_request: int,
    stages_per_request: int,
    inflight_transitions: int,
    work_ms: int,
    queue_slots: int | None,
    max_requests: int | None,
) -> dict:
    cmd = [
        str(binary_path),
        "--mode",
        mode,
        "--concurrency",
        str(concurrency),
        "--duration-secs",
        str(duration_secs),
        "--queues-per-request",
        str(queues_per_request),
        "--stages-per-request",
        str(stages_per_request),
        "--inflight-transitions-per-request",
        str(inflight_transitions),
        "--work-ms",
        str(work_ms),
        "--output-dir",
        str(artifact_dir),
    ]
    if queue_slots is not None:
        cmd.extend(["--queue-slots", str(queue_slots)])
    if max_requests is not None:
        cmd.extend(["--max-requests", str(max_requests)])

    result = subprocess.run(cmd, check=True, capture_output=True, text=True)
    output_lines = [line for line in result.stdout.splitlines() if line.strip()]
    if not output_lines:
        raise SystemExit("missing measurement output from collector_stress")
    return json.loads(output_lines[-1])


def summarize_values(values: list[float]) -> dict[str, float]:
    if not values:
        return {
            "mean": 0.0,
            "median": 0.0,
            "min": 0.0,
            "max": 0.0,
            "stdev": 0.0,
        }

    return {
        "mean": statistics.fmean(values),
        "median": statistics.median(values),
        "min": min(values),
        "max": max(values),
        "stdev": statistics.stdev(values) if len(values) > 1 else 0.0,
    }


def summarize(rows: list[dict]) -> dict:
    grouped: dict[str, list[dict]] = {}
    for row in rows:
        event_shape = row["event_shape"]
        key = (
            f"mode={row['mode']};concurrency={row['concurrency']};duration_secs={row['duration_secs']};"
            f"queues={event_shape['queues_per_request']};stages={event_shape['stages_per_request']};"
            f"inflight={event_shape['inflight_transitions_per_request']};work_ms={event_shape['work_ms']}"
        )
        grouped.setdefault(key, []).append(row)

    by_case = {}
    for key, case_rows in grouped.items():
        by_case[key] = {
            "samples": len(case_rows),
            "throughput_rps": summarize_values([entry["throughput_rps"] for entry in case_rows]),
            "latency_p95_ms": summarize_values([entry["latency"]["p95_ms"] for entry in case_rows]),
            "artifact_size_bytes": summarize_values(
                [
                    float(entry["artifact"]["artifact_size_bytes"])
                    for entry in case_rows
                    if entry["artifact"]["artifact_size_bytes"] is not None
                ]
            ),
            "retained_events": {
                "requests": summarize_values([float(entry["retained_events"]["requests"]) for entry in case_rows]),
                "stages": summarize_values([float(entry["retained_events"]["stages"]) for entry in case_rows]),
                "queues": summarize_values([float(entry["retained_events"]["queues"]) for entry in case_rows]),
                "inflight_snapshots": summarize_values(
                    [float(entry["retained_events"]["inflight_snapshots"]) for entry in case_rows]
                ),
                "runtime_snapshots": summarize_values(
                    [float(entry["retained_events"]["runtime_snapshots"]) for entry in case_rows]
                ),
            },
            "dropped_events": {
                "dropped_requests": summarize_values(
                    [float(entry["truncation"]["dropped_requests"]) for entry in case_rows]
                ),
                "dropped_stages": summarize_values([float(entry["truncation"]["dropped_stages"]) for entry in case_rows]),
                "dropped_queues": summarize_values([float(entry["truncation"]["dropped_queues"]) for entry in case_rows]),
                "dropped_inflight_snapshots": summarize_values(
                    [float(entry["truncation"]["dropped_inflight_snapshots"]) for entry in case_rows]
                ),
                "dropped_runtime_snapshots": summarize_values(
                    [float(entry["truncation"]["dropped_runtime_snapshots"]) for entry in case_rows]
                ),
                "limits_hit_runs": sum(1 for entry in case_rows if entry["truncation"]["limits_hit"]),
            },
            "memory": {
                "collector_end_rss_bytes": summarize_values(
                    [
                        float(entry["memory"]["collector_end_rss_bytes"])
                        for entry in case_rows
                        if entry["memory"]["collector_end_rss_bytes"] is not None
                    ]
                ),
                "collector_peak_rss_bytes": summarize_values(
                    [
                        float(entry["memory"]["collector_peak_rss_bytes"])
                        for entry in case_rows
                        if entry["memory"]["collector_peak_rss_bytes"] is not None
                    ]
                ),
            },
            "measurement_notes": case_rows[0]["measurement_notes"],
        }

    return {
        "measurement_kind": "collector_stress",
        "run_count": len(rows),
        "case_count": len(by_case),
        "cases": by_case,
        "notes": [
            "collector_stress complements runtime_cost by targeting sustained high-concurrency collector behavior and artifact scaling rather than small-mode overhead attribution.",
            "results are measured output for this machine and should not be hardcoded into docs as universal numbers.",
        ],
    }


def main() -> None:
    args = parse_args()
    if args.repeats <= 0:
        raise SystemExit("--repeats must be > 0")
    if args.queue_slots is not None and args.queue_slots <= 0:
        raise SystemExit("--queue-slots must be > 0")
    if args.max_requests is not None and args.max_requests <= 0:
        raise SystemExit("--max-requests must be > 0")

    root_dir = Path(__file__).resolve().parent.parent
    artifact_dir = Path(args.artifact_dir)
    artifact_dir.mkdir(parents=True, exist_ok=True)

    binary_path = build_release_binary(root_dir)
    selected_modes = mode_list(args.modes)

    concurrencies = parse_csv_ints(args.concurrency_matrix, "concurrency-matrix")
    durations = parse_csv_ints(args.duration_secs_matrix, "duration-secs-matrix")
    queues = parse_csv_ints(args.queues_per_request_matrix, "queues-per-request-matrix")
    stages = parse_csv_ints(args.stages_per_request_matrix, "stages-per-request-matrix")
    inflights = parse_csv_ints(args.inflight_transitions_matrix, "inflight-transitions-matrix")
    work_values = parse_csv_ints(args.work_ms_matrix, "work-ms-matrix")

    raw_path = artifact_dir / "collector-stress-raw.jsonl"
    summary_path = artifact_dir / "collector-stress-summary.json"
    raw_path.write_text("", encoding="utf-8")

    all_rows: list[dict] = []
    case_index = 0

    for repeat in range(args.repeats):
        for mode in selected_modes:
            for concurrency in concurrencies:
                for duration_secs in durations:
                    for queue_count in queues:
                        for stage_count in stages:
                            for inflight_count in inflights:
                                for work_ms in work_values:
                                    case_index += 1
                                    print(
                                        (
                                            f"case={case_index} repeat={repeat + 1}/{args.repeats} mode={mode} "
                                            f"concurrency={concurrency} duration={duration_secs}s queues={queue_count} "
                                            f"stages={stage_count} inflight={inflight_count} work_ms={work_ms}"
                                        ),
                                        file=sys.stderr,
                                    )
                                    row = run_case(
                                        binary_path,
                                        artifact_dir,
                                        mode,
                                        concurrency,
                                        duration_secs,
                                        queue_count,
                                        stage_count,
                                        inflight_count,
                                        work_ms,
                                        args.queue_slots,
                                        args.max_requests,
                                    )
                                    row["repeat"] = repeat
                                    all_rows.append(row)
                                    with raw_path.open("a", encoding="utf-8") as raw_file:
                                        raw_file.write(json.dumps(row) + "\n")

    summary = summarize(all_rows)
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")

    print(json.dumps(summary, indent=2))
    print(f"raw results: {raw_path}")
    print(f"summary: {summary_path}")


if __name__ == "__main__":
    main()
