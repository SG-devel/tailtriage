#!/usr/bin/env python3
"""Unified runner/validator for tailtriage demo scenarios."""

from __future__ import annotations

import argparse
import re
from pathlib import Path
from typing import Callable

from _demo_runner import (
    load_report_json,
    repo_root,
    run_and_analyze,
    variant_paths,
    write_before_after_comparison,
)

EXPECTED_QUEUE_KIND = {"application_queue_saturation", "ApplicationQueueSaturation"}
EXPECTED_BLOCKING_KIND = {"blocking_pool_pressure", "BlockingPoolPressure"}
EXPECTED_EXECUTOR_KIND = {"executor_pressure_suspected", "ExecutorPressureSuspected"}
EXPECTED_DOWNSTREAM_KIND = {"downstream_stage_dominates", "DownstreamStageDominates"}
EXPECTED_MIXED_PRIMARY_KINDS = EXPECTED_QUEUE_KIND | EXPECTED_DOWNSTREAM_KIND
EXPECTED_COLD_START_PRIMARY_KINDS = EXPECTED_QUEUE_KIND | EXPECTED_DOWNSTREAM_KIND
EXPECTED_DB_POOL_PRIMARY_KINDS = EXPECTED_QUEUE_KIND | EXPECTED_DOWNSTREAM_KIND
MODE_CHOICES = ["before", "after", "both", "baseline", "mitigated"]

def extract_blocking_queue_depth_p95(report: dict) -> int | None:
    suspect = report.get("primary_suspect") or {}
    for evidence in suspect.get("evidence") or []:
        match = re.search(r"Blocking queue depth p95 is (\d+)", evidence)
        if match:
            return int(match.group(1))
    return None

def normalize_mode(mode: str) -> str:
    if mode in {"baseline", "before"}:
        return "before"
    if mode in {"mitigated", "after"}:
        return "after"
    return mode

def snapshot_queue(report: dict) -> dict[str, int | str | None]:
    return {
        "primary_suspect_kind": report["primary_suspect"]["kind"],
        "primary_suspect_score": report["primary_suspect"]["score"],
        "p95_latency_us": report["p95_latency_us"],
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
    }

def snapshot_blocking(report: dict) -> dict[str, int | str | None]:
    return {
        "primary_suspect_kind": report["primary_suspect"]["kind"],
        "primary_suspect_score": report["primary_suspect"]["score"],
        "p95_latency_us": report["p95_latency_us"],
        "p95_service_share_permille": report.get("p95_service_share_permille"),
        "blocking_queue_depth_p95": extract_blocking_queue_depth_p95(report),
    }

def run_before_after_scenario(
    root_dir: Path,
    demo_manifest: Path,
    artifact_dir: Path,
    mode: str,
    snapshot_fn: Callable[[dict], dict[str, int | str | None]],
) -> None:
    cli_manifest = root_dir / "tailtriage-cli/Cargo.toml"

    def run_variant(variant: str) -> None:
        run_path, analysis_path = variant_paths(artifact_dir, variant)
        mode_arg = "baseline" if variant == "before" else "mitigated"
        run_and_analyze(demo_manifest, cli_manifest, run_path, analysis_path, mode_arg)
        print(f"run artifact ({variant}): {run_path}")
        print(f"analysis ({variant}): {analysis_path}")

    normalized = normalize_mode(mode)
    if normalized in {"before", "after"}:
        run_variant(normalized)
        return

    run_variant("before")
    run_variant("after")
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")
    comparison_path = write_before_after_comparison(
        artifact_dir,
        snapshot_fn(before),
        snapshot_fn(after),
    )
    print(f"comparison: {comparison_path}")

def run_scenario_queue(root_dir: Path, mode: str) -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/queue_service/Cargo.toml",
        root_dir / "demos/queue_service/artifacts",
        mode,
        snapshot_queue,
    )

def run_scenario_blocking(root_dir: Path, mode: str) -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/blocking_service/Cargo.toml",
        root_dir / "demos/blocking_service/artifacts",
        mode,
        snapshot_blocking,
    )

def run_scenario_executor(root_dir: Path, mode: str) -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/executor_pressure_service/Cargo.toml",
        root_dir / "demos/executor_pressure_service/artifacts",
        mode,
        snapshot_queue,
    )

def run_scenario_downstream(root_dir: Path, artifact_path: str | None) -> None:
    run_path = (
        Path(artifact_path)
        if artifact_path
        else root_dir / "demos/downstream_service/artifacts/downstream-run.json"
    )
    analysis_path = root_dir / "demos/downstream_service/artifacts/downstream-analysis.json"
    run_and_analyze(
        root_dir / "demos/downstream_service/Cargo.toml",
        root_dir / "tailtriage-cli/Cargo.toml",
        run_path,
        analysis_path,
    )
    print(f"run artifact: {run_path}")
    print(f"analysis: {analysis_path}")

def run_scenario_mixed(root_dir: Path, mode: str) -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/mixed_contention_service/Cargo.toml",
        root_dir / "demos/mixed_contention_service/artifacts",
        mode,
        snapshot_queue,
    )

def run_scenario_cold_start(root_dir: Path, mode: str) -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/cold_start_burst_service/Cargo.toml",
        root_dir / "demos/cold_start_burst_service/artifacts",
        mode,
        snapshot_queue,
    )

def run_scenario_db_pool(root_dir: Path, mode: str) -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/db_pool_saturation_service/Cargo.toml",
        root_dir / "demos/db_pool_saturation_service/artifacts",
        mode,
        snapshot_queue,
    )

def has_suspect_kind(report: dict, expected_kinds: set[str]) -> bool:
    primary = report.get("primary_suspect") or {}
    all_suspects = [primary, *(report.get("secondary_suspects") or [])]
    return any((suspect or {}).get("kind") in expected_kinds for suspect in all_suspects)

def validate_queue(root_dir: Path) -> None:
    run_scenario_queue(root_dir, "both")
    artifact_dir = root_dir / "demos/queue_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    kind = before["primary_suspect"]["kind"]
    if kind not in EXPECTED_QUEUE_KIND:
        raise SystemExit(f"expected queue saturation suspect in baseline, got {kind}")

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]

    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    if after_score >= before_score:
        raise SystemExit(
            f"expected mitigated suspect score to drop, got before={before_score} after={after_score}"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}".format(
            kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def validate_blocking(root_dir: Path) -> None:
    run_scenario_blocking(root_dir, "both")
    artifact_dir = root_dir / "demos/blocking_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_BLOCKING_KIND:
        raise SystemExit(f"expected blocking pool pressure suspect in baseline, got {before_kind}")

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]

    before_service_share = before.get("p95_service_share_permille")
    after_service_share = after.get("p95_service_share_permille")
    before_blocking_depth = extract_blocking_queue_depth_p95(before)
    after_blocking_depth = extract_blocking_queue_depth_p95(after)

    improvement_signals = []
    if after_score < before_score:
        improvement_signals.append("score")
    if (
        before_service_share is not None
        and after_service_share is not None
        and after_service_share < before_service_share
    ):
        improvement_signals.append("service_share")
    if (
        before_blocking_depth is not None
        and after_blocking_depth is not None
        and after_blocking_depth < before_blocking_depth
    ):
        improvement_signals.append("blocking_queue_depth")

    if not improvement_signals:
        raise SystemExit(
            "expected at least one non-latency improvement signal (score/share/blocking depth), "
            f"got score {before_score}->{after_score}, "
            f"service_share {before_service_share}->{after_service_share}, "
            f"blocking_queue_depth {before_blocking_depth}->{after_blocking_depth}"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}, "
        "service-share {} -> {}, blocking-depth {} -> {} (signals: {})".format(
            before_kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
            before_service_share,
            after_service_share,
            before_blocking_depth,
            after_blocking_depth,
            ", ".join(improvement_signals),
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def validate_downstream(root_dir: Path) -> None:
    run_scenario_downstream(root_dir, None)
    analysis_path = root_dir / "demos/downstream_service/artifacts/downstream-analysis.json"

    report = load_report_json(analysis_path)
    kind = report["primary_suspect"]["kind"]
    if kind not in EXPECTED_DOWNSTREAM_KIND:
        raise SystemExit(f"expected downstream stage suspect, got {kind}")

    print(f"validation passed: primary suspect is {kind}")
    print(f"validated analysis file: {analysis_path}")

def validate_mixed(root_dir: Path) -> None:
    run_scenario_mixed(root_dir, "both")
    artifact_dir = root_dir / "demos/mixed_contention_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    baseline_primary = before["primary_suspect"]["kind"]
    if baseline_primary not in EXPECTED_MIXED_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to be queue or downstream, "
            f"got {baseline_primary}"
        )

    if baseline_primary in EXPECTED_QUEUE_KIND:
        expected_secondary = EXPECTED_DOWNSTREAM_KIND
    else:
        expected_secondary = EXPECTED_QUEUE_KIND

    if not has_suspect_kind(before, expected_secondary):
        raise SystemExit(
            "expected baseline report to include secondary contention source, "
            f"missing one of {sorted(expected_secondary)}"
        )

    after_primary = after["primary_suspect"]["kind"]
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    rank_shifted = after_primary != baseline_primary
    score_shifted = after_score != before_score
    if not (rank_shifted or score_shifted):
        raise SystemExit(
            "expected mitigation to shift rank or score for the primary suspect, "
            f"got kind {baseline_primary}->{after_primary}, score {before_score}->{after_score}"
        )

    print(
        "validation passed: baseline primary={}, mitigated primary={}, "
        "baseline score={} mitigated score={}".format(
            baseline_primary,
            after_primary,
            before_score,
            after_score,
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def _contains_blocking_depth_evidence(report: dict) -> bool:
    suspect = report.get("primary_suspect") or {}
    evidence = suspect.get("evidence") or []
    return any("blocking queue depth" in str(item).lower() for item in evidence)

def validate_executor(root_dir: Path) -> None:
    run_scenario_executor(root_dir, "both")
    artifact_dir = root_dir / "demos/executor_pressure_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    kind = before["primary_suspect"]["kind"]
    if kind not in EXPECTED_EXECUTOR_KIND:
        raise SystemExit(f"expected executor pressure suspect in baseline, got {kind}")

    if _contains_blocking_depth_evidence(before):
        raise SystemExit("executor baseline evidence unexpectedly referenced blocking queue depth")

    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    if after_score >= before_score:
        raise SystemExit(
            f"expected mitigated suspect score to drop, got before={before_score} after={after_score}"
        )

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}".format(
            kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def _report_mentions_cold_start_or_queue(report: dict) -> bool:
    suspects = [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]
    evidence_items = [
        str(item).lower()
        for suspect in suspects
        for item in (suspect.get("evidence") or [])
    ]
    return any(
        "cold_start_stage" in item
        or "queue wait at p95" in item
        or "queue depth sample" in item
        for item in evidence_items
    )

def validate_cold_start(root_dir: Path) -> None:
    run_scenario_cold_start(root_dir, "both")
    artifact_dir = root_dir / "demos/cold_start_burst_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_COLD_START_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to indicate queue or downstream pressure, "
            f"got {before_kind}"
        )

    if not _report_mentions_cold_start_or_queue(before):
        raise SystemExit(
            "expected baseline evidence to reference warmup-driven service stage or queue impact"
        )

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    if after_score >= before_score:
        raise SystemExit(
            f"expected mitigated suspect score to drop, got before={before_score} after={after_score}"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}".format(
            before_kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def validate_db_pool(root_dir: Path) -> None:
    run_scenario_db_pool(root_dir, "both")
    artifact_dir = root_dir / "demos/db_pool_saturation_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_DB_POOL_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to indicate queue or downstream pressure, "
            f"got {before_kind}"
        )

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]

    if after_p95 >= before_p95 and after_score >= before_score:
        raise SystemExit(
            "expected mitigation to improve p95 and/or primary suspect score, "
            f"got p95 {before_p95}->{after_p95} and score {before_score}->{after_score}"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}".format(
            before_kind,
            before_p95,
            after_p95,
            before_score,
            after_score,
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Unified tailtriage demo run/validate tool.")
    subparsers = parser.add_subparsers(dest="command", required=True)

    run_parser = subparsers.add_parser("run", help="Run demo scenario and produce analysis artifacts")
    run_parser.add_argument(
        "scenario",
        choices=["queue", "blocking", "executor", "downstream", "mixed", "cold-start", "db-pool"],
    )
    run_parser.add_argument(
        "mode",
        nargs="?",
        default="both",
        choices=MODE_CHOICES,
        help="Queue/blocking mode (before/after/both + baseline/mitigated aliases).",
    )
    run_parser.add_argument(
        "--artifact-path",
        help="Downstream only: custom run artifact path.",
    )

    validate_parser = subparsers.add_parser("validate", help="Run scenario validation contract checks")
    validate_parser.add_argument(
        "scenario",
        choices=["queue", "blocking", "executor", "downstream", "mixed", "cold-start", "db-pool"],
    )

    return parser.parse_args(argv)

def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    root_dir = repo_root(__file__)

    if args.command == "run":
        if args.scenario == "queue":
            run_scenario_queue(root_dir, args.mode)
        elif args.scenario == "blocking":
            run_scenario_blocking(root_dir, args.mode)
        elif args.scenario == "downstream":
            if args.mode != "both":
                raise SystemExit("downstream scenario does not accept mode; use --artifact-path if needed")
            run_scenario_downstream(root_dir, args.artifact_path)
        elif args.scenario == "executor":
            run_scenario_executor(root_dir, args.mode)
        elif args.scenario == "cold-start":
            run_scenario_cold_start(root_dir, args.mode)
        elif args.scenario == "db-pool":
            run_scenario_db_pool(root_dir, args.mode)
        else:
            run_scenario_mixed(root_dir, args.mode)
        return

    if args.scenario == "queue":
        validate_queue(root_dir)
    elif args.scenario == "blocking":
        validate_blocking(root_dir)
    elif args.scenario == "downstream":
        validate_downstream(root_dir)
    elif args.scenario == "executor":
        validate_executor(root_dir)
    elif args.scenario == "cold-start":
        validate_cold_start(root_dir)
    elif args.scenario == "db-pool":
        validate_db_pool(root_dir)
    else:
        validate_mixed(root_dir)

if __name__ == "__main__":
    main()
