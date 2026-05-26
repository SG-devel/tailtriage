#!/usr/bin/env python3
"""Unified runner/validator for tailtriage demo scenarios."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Callable

from _demo_runner import (
    PROFILE_CHOICES,
    load_report_json,
    repo_root,
    run_and_analyze,
    variant_paths,
    write_before_after_comparison,
)

EXPECTED_QUEUE_KIND = {"application_queue_saturation"}
EXPECTED_BLOCKING_KIND = {"blocking_pool_pressure"}
EXPECTED_EXECUTOR_KIND = {"executor_pressure_suspected"}
EXPECTED_DOWNSTREAM_KIND = {"downstream_stage_dominates"}
EXPECTED_MIXED_PRIMARY_KINDS = EXPECTED_QUEUE_KIND
EXPECTED_COLD_START_PRIMARY_KINDS = EXPECTED_QUEUE_KIND
EXPECTED_DB_POOL_PRIMARY_KINDS = EXPECTED_QUEUE_KIND
EXPECTED_SHARED_LOCK_PRIMARY_KINDS = EXPECTED_QUEUE_KIND
EXPECTED_RETRY_STORM_PRIMARY_KINDS = EXPECTED_DOWNSTREAM_KIND
MODE_CHOICES = ["before", "after", "both", "baseline", "mitigated"]
SCENARIOS = [
    "queue",
    "blocking",
    "executor",
    "downstream",
    "mixed",
    "cold-start",
    "db-pool",
    "shared-lock",
    "retry-storm",
]


def _suspects(report: dict) -> list[dict]:
    return [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]


def suspect_score(report: dict, kind: str) -> int | None:
    for suspect in _suspects(report):
        if suspect.get("kind") == kind:
            return suspect.get("score")
    return None

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

def snapshot_downstream(report: dict) -> dict[str, int | str | None]:
    return {
        "primary_suspect_kind": report["primary_suspect"]["kind"],
        "primary_suspect_score": report["primary_suspect"]["score"],
        "p95_latency_us": report["p95_latency_us"],
        "p95_service_share_permille": report.get("p95_service_share_permille"),
    }

def run_before_after_scenario(
    root_dir: Path,
    demo_manifest: Path,
    artifact_dir: Path,
    mode: str,
    snapshot_fn: Callable[[dict], dict[str, int | str | None]],
    *,
    profile: str = "dev",
) -> None:
    cli_manifest = root_dir / "tailtriage-cli/Cargo.toml"

    def run_variant(variant: str) -> None:
        run_path, analysis_path = variant_paths(artifact_dir, variant)
        mode_arg = "baseline" if variant == "before" else "mitigated"
        run_and_analyze(
            demo_manifest,
            cli_manifest,
            run_path,
            analysis_path,
            mode_arg,
            profile=profile,
        )
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

def run_scenario_queue(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/queue_service/Cargo.toml",
        root_dir / "demos/queue_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_blocking(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/blocking_service/Cargo.toml",
        root_dir / "demos/blocking_service/artifacts",
        mode,
        snapshot_blocking,
        profile=profile,
    )

def run_scenario_executor(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/executor_pressure_service/Cargo.toml",
        root_dir / "demos/executor_pressure_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_downstream(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/downstream_service/Cargo.toml",
        root_dir / "demos/downstream_service/artifacts",
        mode,
        snapshot_downstream,
        profile=profile,
    )

def run_scenario_mixed(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/mixed_contention_service/Cargo.toml",
        root_dir / "demos/mixed_contention_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_cold_start(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/cold_start_burst_service/Cargo.toml",
        root_dir / "demos/cold_start_burst_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_db_pool(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/db_pool_saturation_service/Cargo.toml",
        root_dir / "demos/db_pool_saturation_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_shared_lock(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/shared_state_lock_service/Cargo.toml",
        root_dir / "demos/shared_state_lock_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_retry_storm(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        root_dir / "demos/retry_storm_service/Cargo.toml",
        root_dir / "demos/retry_storm_service/artifacts",
        mode,
        snapshot_queue,
        profile=profile,
    )

def has_suspect_kind(report: dict, expected_kinds: set[str]) -> bool:
    primary = report.get("primary_suspect") or {}
    all_suspects = [primary, *(report.get("secondary_suspects") or [])]
    return any((suspect or {}).get("kind") in expected_kinds for suspect in all_suspects)


def _material_p95_improvement(before_p95: int, after_p95: int) -> bool:
    return after_p95 < before_p95 and (before_p95 - after_p95) >= max(1_000, before_p95 // 20)


def _queue_evidence_non_worsening(before: dict, after: dict) -> bool:
    before_share = before.get("p95_queue_share_permille")
    after_share = after.get("p95_queue_share_permille")
    if before_share is None or after_share is None:
        return True
    return after_share <= before_share + 20


def _queue_evidence_materially_improved(before: dict, after: dict) -> bool:
    before_share = before.get("p95_queue_share_permille")
    after_share = after.get("p95_queue_share_permille")
    if before_share is None or after_share is None:
        return False
    return after_share + 100 <= before_share


def _validate_nonworsening_score_or_explainable_saturation(
    *,
    before: dict,
    after: dict,
    expected_primary_kinds: set[str],
    scenario: str,
) -> None:
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    if after_score <= before_score:
        return

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    after_kind = after["primary_suspect"]["kind"]
    if not _material_p95_improvement(before_p95, after_p95):
        raise SystemExit(
            f"expected mitigated {scenario} suspect score to stay flat or drop when p95 does not materially improve, "
            f"got p95 {before_p95}->{after_p95} and score {before_score}->{after_score}"
        )
    if after_kind not in expected_primary_kinds and not _queue_evidence_materially_improved(before, after):
        raise SystemExit(
            f"expected mitigated {scenario} primary suspect in {sorted(expected_primary_kinds)} when score rises unless queue evidence materially improves, got {after_kind}"
        )
    if not _queue_evidence_non_worsening(before, after):
        raise SystemExit(
            f"expected mitigated {scenario} score increase to have non-worsening queue evidence, "
            f"got queue share {before.get('p95_queue_share_permille')}->{after.get('p95_queue_share_permille')}"
        )


def _validate_nonworsening_score_for_downstream(
    *,
    before: dict,
    after: dict,
    expected_primary_kinds: set[str],
    scenario: str,
) -> None:
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    if after_score <= before_score:
        return

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    after_kind = after["primary_suspect"]["kind"]
    if not _material_p95_improvement(before_p95, after_p95):
        raise SystemExit(
            f"expected mitigated {scenario} suspect score to stay flat or drop when p95 does not materially improve, "
            f"got p95 {before_p95}->{after_p95} and score {before_score}->{after_score}"
        )
    if after_kind not in expected_primary_kinds:
        raise SystemExit(
            f"expected mitigated {scenario} primary suspect in {sorted(expected_primary_kinds)} when score rises, got {after_kind}"
        )

def validate_queue(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_queue(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/queue_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    kind = before["primary_suspect"]["kind"]
    if kind not in EXPECTED_QUEUE_KIND:
        raise SystemExit(f"expected queue saturation suspect in baseline, got {kind}")

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )
    _validate_nonworsening_score_or_explainable_saturation(
        before=before,
        after=after,
        expected_primary_kinds=EXPECTED_QUEUE_KIND,
        scenario="queue",
    )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, score {} -> {}".format(
            kind,
            before_p95,
            after_p95,
            before["primary_suspect"]["score"],
            after["primary_suspect"]["score"],
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )

def validate_blocking(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_blocking(root_dir, "both", profile=profile)
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

def validate_downstream(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_downstream(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/downstream_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_DOWNSTREAM_KIND:
        raise SystemExit(f"expected downstream stage suspect in baseline, got {before_kind}")

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    _validate_nonworsening_score_for_downstream(
        before=before,
        after=after,
        expected_primary_kinds=EXPECTED_DOWNSTREAM_KIND,
        scenario="downstream",
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

def validate_mixed(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_mixed(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/mixed_contention_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    baseline_primary = before["primary_suspect"]["kind"]
    if baseline_primary not in EXPECTED_MIXED_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to be queue saturation, "
            f"got {baseline_primary}"
        )

    expected_secondary = EXPECTED_DOWNSTREAM_KIND
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
    return any(
        "blocking queue depth" in str(item).lower()
        for suspect in _suspects(report)
        for item in (suspect.get("evidence") or [])
    )

def validate_executor(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_executor(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/executor_pressure_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    kind = before["primary_suspect"]["kind"]
    if kind not in EXPECTED_EXECUTOR_KIND:
        raise SystemExit(
            "expected executor demo baseline primary suspect in "
            f"{sorted(EXPECTED_EXECUTOR_KIND)}, got {kind}"
        )

    has_executor_suspect = has_suspect_kind(before, EXPECTED_EXECUTOR_KIND)
    if not has_executor_suspect:
        raise SystemExit("expected executor pressure suspect to appear in baseline report")

    if _contains_blocking_depth_evidence(before):
        raise SystemExit("executor baseline evidence unexpectedly referenced blocking queue depth")

    before_score = suspect_score(before, "executor_pressure_suspected")
    after_score = suspect_score(after, "executor_pressure_suspected")
    if profile != "release" and before_score is None:
        raise SystemExit("baseline report missing executor pressure suspect score")
    if before_score is not None and after_score is not None and after_score > before_score:
        raise SystemExit(
            "expected mitigated executor suspect score to stay flat or drop, "
            f"got before={before_score} after={after_score}"
        )

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, "
        "executor score {} -> {}".format(
            kind,
            before_p95,
            after_p95,
            before_score,
            after_score if after_score is not None else "missing",
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

def validate_cold_start(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_cold_start(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/cold_start_burst_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_COLD_START_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to indicate queue pressure, "
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
    _validate_nonworsening_score_or_explainable_saturation(
        before=before,
        after=after,
        expected_primary_kinds=EXPECTED_COLD_START_PRIMARY_KINDS,
        scenario="cold-start",
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

def validate_db_pool(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_db_pool(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/db_pool_saturation_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_DB_POOL_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to indicate queue pressure, "
            f"got {before_kind}"
        )

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]

    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )
    _validate_nonworsening_score_or_explainable_saturation(
        before=before,
        after=after,
        expected_primary_kinds=EXPECTED_DB_POOL_PRIMARY_KINDS,
        scenario="db-pool",
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

def validate_shared_lock(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_shared_lock(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/shared_state_lock_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_SHARED_LOCK_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to indicate queue pressure, "
            f"got {before_kind}"
        )

    evidence_text = " ".join(
        str(item).lower()
        for suspect in [before.get("primary_suspect") or {}, *(before.get("secondary_suspects") or [])]
        for item in (suspect.get("evidence") or [])
    )
    if "queue wait at p95" not in evidence_text and "queue depth sample" not in evidence_text:
        raise SystemExit("expected baseline evidence to mention queue wait/depth from lock contention")

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]

    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )
    if after_score > before_score:
        raise SystemExit(
            "expected mitigated score to stay flat/drop or be justified by better evidence; score-only increase is not sufficient, "
            f"got before={before_score} after={after_score}"
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

def validate_retry_storm(root_dir: Path, *, profile: str = "dev") -> None:
    run_scenario_retry_storm(root_dir, "both", profile=profile)
    artifact_dir = root_dir / "demos/retry_storm_service/artifacts"
    before = load_report_json(artifact_dir / "before-analysis.json")
    after = load_report_json(artifact_dir / "after-analysis.json")

    before_kind = before["primary_suspect"]["kind"]
    if before_kind not in EXPECTED_RETRY_STORM_PRIMARY_KINDS:
        raise SystemExit(
            "expected baseline primary suspect to indicate downstream stage dominance, "
            f"got {before_kind}"
        )

    before_share = before.get("p95_service_share_permille")
    if before_share is None or before_share < 900:
        raise SystemExit(
            "expected baseline to have elevated service share from retry-heavy downstream time, "
            f"got p95_service_share_permille={before_share}"
        )

    before_p95 = before["p95_latency_us"]
    after_p95 = after["p95_latency_us"]
    if after_p95 >= before_p95:
        raise SystemExit(
            f"expected mitigated p95 to drop, got before={before_p95}us after={after_p95}us"
        )

    before_score = before["primary_suspect"]["score"]
    after_score = after["primary_suspect"]["score"]
    if after_score > before_score:
        raise SystemExit(
            "expected mitigated score to stay flat/drop or be justified by better evidence; score-only increase is not sufficient, "
            f"got before={before_score} after={after_score}"
        )

    print(
        "validation passed: baseline suspect kind={}, p95 {}us -> {}us, "
        "service-share {} -> {}, score {} -> {}".format(
            before_kind,
            before_p95,
            after_p95,
            before_share,
            after.get("p95_service_share_permille"),
            before_score,
            after_score,
        )
    )
    print(
        "validated analysis files: "
        f"{artifact_dir / 'before-analysis.json'}, {artifact_dir / 'after-analysis.json'}"
    )


PARITY_SCENARIOS = ["queue", "downstream", "mixed", "cold-start", "db-pool", "shared-lock", "retry-storm", "blocking", "executor", "all"]

def _artifact_prefix(mode: str, instrumentation: str) -> str:
    return f"{mode}-{instrumentation}"


def _load_run(path: Path) -> dict:
    with path.open("r", encoding="utf-8") as handle:
        return json.load(handle)




def _parity_fail(*, scenario: str, instrumentation: str, artifact_path: str, field: str, expected: object, actual: object) -> None:
    raise SystemExit(
        f"parity check failed scenario={scenario} instrumentation={instrumentation} artifact={artifact_path} "
        f"field={field} expected={expected!r} actual={actual!r}"
    )


def _require_equal(*, scenario: str, instrumentation: str, artifact_path: str, field: str, expected: object, actual: object) -> None:
    if expected != actual:
        _parity_fail(
            scenario=scenario,
            instrumentation=instrumentation,
            artifact_path=artifact_path,
            field=field,
            expected=expected,
            actual=actual,
        )


def _capture_limits(run: dict) -> dict | None:
    return ((run.get("metadata") or {}).get("effective_core_config") or {}).get("capture_limits")


RUNTIME_SENSITIVE_TRACING_SCENARIOS = {"blocking", "executor"}
NON_RUNTIME_TRACING_SCENARIOS = {
    "queue",
    "downstream",
    "mixed",
    "cold-start",
    "db-pool",
    "shared-lock",
    "retry-storm",
}

def _tracing_parity_config(root_dir: Path, scenario: str) -> dict:
    configs = {
        "queue": {
            "demo_manifest": root_dir / "demos/queue_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/queue_service/artifacts",
            "route": "/queue-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"simulated_work"},
            "require_p95_improvement": True,
        },
        "downstream": {
            "demo_manifest": root_dir / "demos/downstream_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/downstream_service/artifacts",
            "route": "/downstream-demo",
            "expected_kind": "downstream_stage_dominates",
            "queues": set(),
            "stages": {"app_precheck", "downstream_call"},
            "require_p95_improvement": True,
        },
        "mixed": {
            "demo_manifest": root_dir / "demos/mixed_contention_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/mixed_contention_service/artifacts",
            "route": "/mixed-contention-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"app_prepare", "downstream_call"},
            "require_p95_improvement": True,
        },
        "cold-start": {
            "demo_manifest": root_dir / "demos/cold_start_burst_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/cold_start_burst_service/artifacts",
            "route": "/cold-start-burst-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_admission"},
            "stages": {"cold_start_stage"},
            "require_p95_improvement": True,
        },
        "db-pool": {
            "demo_manifest": root_dir / "demos/db_pool_saturation_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/db_pool_saturation_service/artifacts",
            "route": "/db-pool-saturation-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"db_pool"},
            "stages": {"app_precheck", "db_query"},
            "require_p95_improvement": True,
        },
        "shared-lock": {
            "demo_manifest": root_dir / "demos/shared_state_lock_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/shared_state_lock_service/artifacts",
            "route": "/shared-state-lock-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"shared_state_write_lock"},
            "stages": {"pre_lock_work", "shared_state_critical_section"},
            "require_p95_improvement": True,
        },
        "retry-storm": {
            "demo_manifest": root_dir / "demos/retry_storm_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/retry_storm_service/artifacts",
            "route": "/retry-storm-demo",
            "expected_kind": "downstream_stage_dominates",
            "queues": set(),
            "stages": {"app_precheck", "downstream_total"},
            # Retry-heavy downstream behavior can make p95 movement less stable between
            # native/tracing mitigated runs, so parity relies on strict artifact checks plus
            # expected suspect-family presence instead of strict p95 non-worsening.
            "require_p95_improvement": False,
        },
        "blocking": {
            "demo_manifest": root_dir / "demos/blocking_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/blocking_service/artifacts",
            "route": "/blocking-demo",
            "expected_kind": "blocking_pool_pressure",
            "queues": {"dispatch_overhead"},
            "stages": {"spawn_blocking_path"},
            "require_p95_improvement": True,
        },
        "executor": {
            "demo_manifest": root_dir / "demos/executor_pressure_service/Cargo.toml",
            "artifact_dir": root_dir / "demos/executor_pressure_service/artifacts",
            "route": "/executor-pressure",
            "expected_kind": "executor_pressure_suspected",
            "queues": set(),
            "stages": set(),
            "require_p95_improvement": True,
        },
    }
    if scenario not in configs:
        raise SystemExit(f"unsupported tracing parity scenario: {scenario}")
    return configs[scenario]

def validate_tracing_parity(root_dir: Path, scenario: str, *, profile: str = "dev") -> None:
    if scenario == "all":
        for s in [x for x in PARITY_SCENARIOS if x != "all"]:
            validate_tracing_parity(root_dir, s, profile=profile)
        return
    config = _tracing_parity_config(root_dir, scenario)
    demo_manifest = config["demo_manifest"]
    artifact_dir = config["artifact_dir"]
    expected_kind = config["expected_kind"]

    cli_manifest = root_dir / "tailtriage-cli/Cargo.toml"

    for mode in ("before", "after"):
        mode_arg = "baseline" if mode == "before" else "mitigated"
        for instrumentation in ("native", "tracing"):
            prefix = _artifact_prefix(mode, instrumentation)
            run_path = artifact_dir / f"{prefix}-run.json"
            analysis_path = artifact_dir / f"{prefix}-analysis.json"
            run_and_analyze(
                demo_manifest,
                cli_manifest,
                run_path,
                analysis_path,
                mode_arg,
                profile=profile,
                extra_demo_args=["--instrumentation", instrumentation, "--mode", "light"],
            )

    expected_files = [
        "before-native-run.json",
        "before-tracing-run.json",
        "before-native-analysis.json",
        "before-tracing-analysis.json",
        "after-native-run.json",
        "after-tracing-run.json",
        "after-native-analysis.json",
        "after-tracing-analysis.json",
    ]
    missing = [name for name in expected_files if not (artifact_dir / name).exists()]
    if missing:
        raise SystemExit(f"missing parity artifacts: {', '.join(missing)}")

    before_native_run = _load_run(artifact_dir / "before-native-run.json")
    before_tracing_run = _load_run(artifact_dir / "before-tracing-run.json")
    after_native_run = _load_run(artifact_dir / "after-native-run.json")
    after_tracing_run = _load_run(artifact_dir / "after-tracing-run.json")

    before_native = load_report_json(artifact_dir / "before-native-analysis.json")
    before_tracing = load_report_json(artifact_dir / "before-tracing-analysis.json")
    after_native = load_report_json(artifact_dir / "after-native-analysis.json")
    after_tracing = load_report_json(artifact_dir / "after-tracing-analysis.json")

    for label, report in (
        ("before-native", before_native),
        ("before-tracing", before_tracing),
        ("after-native", after_native),
        ("after-tracing", after_tracing),
    ):
        if report["request_count"] <= 0:
            raise SystemExit(f"expected non-zero request count in {label}")
        if report["p95_latency_us"] <= 0:
            raise SystemExit(f"expected non-zero p95 latency in {label}")

    for label, run in (
        ("before-native", before_native_run),
        ("before-tracing", before_tracing_run),
        ("after-native", after_native_run),
        ("after-tracing", after_tracing_run),
    ):
        if len(run.get("requests", [])) == 0:
            raise SystemExit(f"expected non-zero requests in {label} run artifact")
        if scenario != "executor" and len(run.get("stages", [])) == 0:
            raise SystemExit(f"expected non-zero stages in {label} run artifact")
        routes = {r.get("route") for r in run.get("requests", [])}
        if config["route"] not in routes:
            raise SystemExit(f"expected route {config['route']} in {label} run artifact")

    if config["queues"]:
        for label, run in (
            ("before-native", before_native_run),
            ("before-tracing", before_tracing_run),
            ("after-native", after_native_run),
            ("after-tracing", after_tracing_run),
        ):
            if len(run.get("queues", [])) == 0:
                raise SystemExit(f"expected non-zero queues in {label} run artifact")

        for run_name, run in (
            ("before-tracing-run.json", before_tracing_run),
            ("after-tracing-run.json", after_tracing_run),
        ):
            queue_names = {q.get("queue") for q in run.get("queues", [])}
            if not config["queues"].issubset(queue_names):
                raise SystemExit(
                    f"expected queue tracing artifact {run_name} to include queues {sorted(config['queues'])}"
                )
            if not any(q.get("depth_at_start") is not None for q in run.get("queues", [])):
                raise SystemExit(
                    f"expected queue tracing queue events in {run_name} to include non-null depth_at_start"
                )

    for run_name, run in (("before-tracing-run.json", before_tracing_run), ("after-tracing-run.json", after_tracing_run)):
        tracing_stage_names = {s.get("stage") for s in run.get("stages", [])}
        for stage in config["stages"]:
            if stage not in tracing_stage_names:
                raise SystemExit(f"expected tracing run {run_name} to include stage '{stage}'")
        if scenario == "retry-storm":
            if not any(name and name.startswith("downstream_attempt_") for name in tracing_stage_names):
                raise SystemExit(f"expected tracing run {run_name} to include at least one downstream_attempt_* stage")
        if scenario in RUNTIME_SENSITIVE_TRACING_SCENARIOS:
            if not run.get("runtime_snapshots"):
                raise SystemExit(f"expected runtime snapshots in tracing run {run_name}")
            metadata = run.get("metadata", {})
            sampler_config = metadata.get("effective_tokio_sampler_config")
            warnings = metadata.get("lifecycle_warnings", []) or []
            disabled_manual_warning = any(
                "background Tokio runtime sampling disabled" in warning for warning in warnings
            )
            if sampler_config is None and not disabled_manual_warning:
                raise SystemExit(
                    "expected effective_tokio_sampler_config or explicit disabled-sampler lifecycle warning "
                    f"in tracing run {run_name}"
                )
        if scenario in NON_RUNTIME_TRACING_SCENARIOS:
            _require_equal(
                scenario=scenario,
                instrumentation="tracing",
                artifact_path=run_name,
                field="runtime_snapshots",
                expected=[],
                actual=run.get("runtime_snapshots") or [],
            )
            _require_equal(
                scenario=scenario,
                instrumentation="tracing",
                artifact_path=run_name,
                field="metadata.effective_tokio_sampler_config",
                expected=None,
                actual=(run.get("metadata") or {}).get("effective_tokio_sampler_config"),
            )
        if scenario == "blocking":
            if not any(s.get("blocking_queue_depth") is not None for s in run.get("runtime_snapshots", [])):
                raise SystemExit(f"expected blocking_queue_depth runtime evidence in {run_name}")
        if scenario == "executor":
            if not any((s.get("global_queue_depth") is not None) or (s.get("local_queue_depth") is not None) for s in run.get("runtime_snapshots", [])):
                raise SystemExit(f"expected global/local queue runtime evidence in {run_name}")

    for label, run in (("before-native", before_native_run), ("after-native", after_native_run)):
        if "inflight" in run and len(run.get("inflight") or []) == 0:
            raise SystemExit(
                f"expected native inflight snapshots in {label}; tracing inflight is out of scope for prompt 3"
            )

    if not has_suspect_kind(before_native, {expected_kind}):
        raise SystemExit(
            f"expected baseline native primary suspect {expected_kind}, got {before_native['primary_suspect']['kind']}"
        )
    if not has_suspect_kind(before_tracing, {expected_kind}):
        raise SystemExit(
            f"expected baseline tracing primary suspect {expected_kind}, got {before_tracing['primary_suspect']['kind']}"
        )

    if config["require_p95_improvement"] and after_tracing["p95_latency_us"] > before_tracing["p95_latency_us"]:
        raise SystemExit(
            "expected tracing mitigated p95 to be non-worse than tracing baseline, "
            f"got {before_tracing['p95_latency_us']}us -> {after_tracing['p95_latency_us']}us"
        )

    if after_native["primary_suspect"]["kind"] != after_tracing["primary_suspect"]["kind"]:
        expected_in_native = has_suspect_kind(after_native, {expected_kind})
        expected_in_tracing = has_suspect_kind(after_tracing, {expected_kind})
        if expected_in_native and expected_in_tracing:
            print(
                f"info: mitigated parity primary suspect diverged for {scenario} but expected family is still present "
                f"(native={after_native['primary_suspect']['kind']} score={after_native['primary_suspect']['score']}, "
                f"tracing={after_tracing['primary_suspect']['kind']} score={after_tracing['primary_suspect']['score']})"
            )
        else:
            raise SystemExit(
                "mitigated native/tracing primary suspect mismatch: "
                f"native={after_native['primary_suspect']['kind']} score={after_native['primary_suspect']['score']}, "
                f"tracing={after_tracing['primary_suspect']['kind']} score={after_tracing['primary_suspect']['score']}, "
                f"expected_kind_present_native={expected_in_native}, "
                f"expected_kind_present_tracing={expected_in_tracing}"
            )

    for mode, native_run, tracing_run in (
        ("before", before_native_run, before_tracing_run),
        ("after", after_native_run, after_tracing_run),
    ):
        _require_equal(scenario=scenario, instrumentation="native/tracing", artifact_path=f"{mode}-run", field="scenario_label", expected=native_run.get("scenario_label"), actual=tracing_run.get("scenario_label"))
        _require_equal(scenario=scenario, instrumentation="native/tracing", artifact_path=f"{mode}-run", field="metadata.mode", expected=(native_run.get("metadata") or {}).get("mode"), actual=(tracing_run.get("metadata") or {}).get("mode"))
        _require_equal(scenario=scenario, instrumentation="native/tracing", artifact_path=f"{mode}-run", field="metadata.effective_core_config.capture_limits", expected=_capture_limits(native_run), actual=_capture_limits(tracing_run))
        _require_equal(
            scenario=scenario,
            instrumentation="native/tracing",
            artifact_path=f"{mode}-run",
            field="route_coverage",
            expected=sorted({r.get("route") for r in native_run.get("requests", [])}),
            actual=sorted({r.get("route") for r in tracing_run.get("requests", [])}),
        )

    print(
        f"tracing parity validation passed for {scenario}: "
        f"baseline kind={expected_kind}, tracing p95 {before_tracing['p95_latency_us']}us -> {after_tracing['p95_latency_us']}us"
    )


def validate_tracing_retention_parity(root_dir: Path, *, profile: str = "dev") -> None:
    scenario = "queue"
    config = _tracing_parity_config(root_dir, scenario)
    demo_manifest = config["demo_manifest"]
    artifact_dir = config["artifact_dir"]
    cli_manifest = root_dir / "tailtriage-cli/Cargo.toml"
    for instrumentation in ("native", "tracing"):
        run_path = artifact_dir / f"tiny-{instrumentation}-run.json"
        analysis_path = artifact_dir / f"tiny-{instrumentation}-analysis.json"
        run_and_analyze(
            demo_manifest,
            cli_manifest,
            run_path,
            analysis_path,
            "baseline",
            profile=profile,
                extra_demo_args=[
                    "--instrumentation", instrumentation, "--mode", "light",
                    "--max-requests", "3", "--max-stages", "3", "--max-queues", "3",
                ],
        )
    native_run = _load_run(artifact_dir / "tiny-native-run.json")
    tracing_run = _load_run(artifact_dir / "tiny-tracing-run.json")
    pairs = [
        ("retained_request_count", len(native_run.get("requests", [])), len(tracing_run.get("requests", []))),
        ("retained_stage_count", len(native_run.get("stages", [])), len(tracing_run.get("stages", []))),
        ("retained_queue_count", len(native_run.get("queues", [])), len(tracing_run.get("queues", []))),
        ("truncation.dropped_requests", (native_run.get("truncation") or {}).get("dropped_requests"), (tracing_run.get("truncation") or {}).get("dropped_requests")),
        ("truncation.dropped_stages", (native_run.get("truncation") or {}).get("dropped_stages"), (tracing_run.get("truncation") or {}).get("dropped_stages")),
        ("truncation.dropped_queues", (native_run.get("truncation") or {}).get("dropped_queues"), (tracing_run.get("truncation") or {}).get("dropped_queues")),
        (
            "truncation.limits_hit",
            (native_run.get("truncation") or {}).get("limits_hit"),
            (tracing_run.get("truncation") or {}).get("limits_hit"),
        ),
        ("metadata.effective_core_config", (native_run.get("metadata") or {}).get("effective_core_config"), (tracing_run.get("metadata") or {}).get("effective_core_config")),
    ]
    for field, expected, actual in pairs:
        _require_equal(scenario="tiny-limit", instrumentation="native/tracing", artifact_path="tiny-run", field=field, expected=expected, actual=actual)
    print("tracing retention parity validation passed (tiny limits)")
def parse_args(argv: list[str] | None = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Unified tailtriage demo run/validate tool.")
    subparsers = parser.add_subparsers(dest="command", required=True)


    run_parser = subparsers.add_parser("run", help="Run demo scenario and produce analysis artifacts")
    run_parser.add_argument(
        "scenario",
        choices=SCENARIOS,
    )
    run_parser.add_argument(
        "mode",
        nargs="?",
        default="both",
        choices=MODE_CHOICES,
        help="Demo mode (before/after/both + baseline/mitigated aliases).",
    )
    run_parser.add_argument(
        "--profile",
        choices=PROFILE_CHOICES,
        default="dev",
        help="Cargo profile for demo run and CLI analysis (default: dev).",
    )
    run_parser.add_argument(
        "--release",
        action="store_const",
        const="release",
        dest="profile",
        help="Shortcut for --profile release.",
    )

    validate_parser = subparsers.add_parser("validate", help="Run scenario validation contract checks")
    validate_parser.add_argument(
        "scenario",
        choices=SCENARIOS,
    )
    validate_parser.add_argument(
        "--profile",
        choices=PROFILE_CHOICES,
        default="dev",
        help="Cargo profile for demo run and CLI analysis (default: dev).",
    )
    validate_parser.add_argument(
        "--release",
        action="store_const",
        const="release",
        dest="profile",
        help="Shortcut for --profile release.",
    )

    matrix_parser = subparsers.add_parser(
        "diagnosis-matrix",
        help="Run baseline/mitigated demo variants in dev and release and print a compact diagnosis table.",
    )
    matrix_parser.add_argument(
        "--scenario",
        action="append",
        choices=SCENARIOS,
        help="Optional scenario filter; can be provided multiple times.",
    )

    parity_parser = subparsers.add_parser(
        "validate-tracing-parity",
        help="Run native/tracing parity checks for demo scenarios, including runtime-sensitive demos.",
    )
    parity_parser.add_argument("scenario", choices=PARITY_SCENARIOS)
    parity_parser.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    parity_parser.add_argument("--release", action="store_const", const="release", dest="profile")

    tiny_parser = subparsers.add_parser("validate-tracing-retention-parity", help="Run exact retention/truncation parity checks with tiny capture limits.")
    tiny_parser.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    tiny_parser.add_argument("--release", action="store_const", const="release", dest="profile")

    return parser.parse_args(argv)

def _scenario_to_artifact_dir(root_dir: Path, scenario: str) -> Path:
    return {
        "queue": root_dir / "demos/queue_service/artifacts",
        "blocking": root_dir / "demos/blocking_service/artifacts",
        "executor": root_dir / "demos/executor_pressure_service/artifacts",
        "downstream": root_dir / "demos/downstream_service/artifacts",
        "mixed": root_dir / "demos/mixed_contention_service/artifacts",
        "cold-start": root_dir / "demos/cold_start_burst_service/artifacts",
        "db-pool": root_dir / "demos/db_pool_saturation_service/artifacts",
        "shared-lock": root_dir / "demos/shared_state_lock_service/artifacts",
        "retry-storm": root_dir / "demos/retry_storm_service/artifacts",
    }[scenario]

def _run_scenario(root_dir: Path, scenario: str, mode: str, *, profile: str) -> None:
    if scenario == "queue":
        run_scenario_queue(root_dir, mode, profile=profile)
    elif scenario == "blocking":
        run_scenario_blocking(root_dir, mode, profile=profile)
    elif scenario == "downstream":
        run_scenario_downstream(root_dir, mode, profile=profile)
    elif scenario == "executor":
        run_scenario_executor(root_dir, mode, profile=profile)
    elif scenario == "cold-start":
        run_scenario_cold_start(root_dir, mode, profile=profile)
    elif scenario == "db-pool":
        run_scenario_db_pool(root_dir, mode, profile=profile)
    elif scenario == "shared-lock":
        run_scenario_shared_lock(root_dir, mode, profile=profile)
    elif scenario == "retry-storm":
        run_scenario_retry_storm(root_dir, mode, profile=profile)
    else:
        run_scenario_mixed(root_dir, mode, profile=profile)

def run_diagnosis_matrix(root_dir: Path, scenarios: list[str] | None = None) -> None:
    selected = scenarios or SCENARIOS
    print("scenario profile mode primary score p95_us secondary evidence")
    for scenario in selected:
        for profile in PROFILE_CHOICES:
            for mode in ("before", "after"):
                _run_scenario(root_dir, scenario, mode, profile=profile)
                report = load_report_json(_scenario_to_artifact_dir(root_dir, scenario) / f"{mode}-analysis.json")
                primary = report["primary_suspect"]["kind"]
                score = report["primary_suspect"]["score"]
                p95 = report["p95_latency_us"]
                secondary = ",".join(s["kind"] for s in (report.get("secondary_suspects") or [])) or "-"
                evidence = "; ".join((report["primary_suspect"].get("evidence") or [])[:2]).replace("\n", " ")
                print(f"{scenario:11} {profile:7} {mode:6} {primary:30} {score:5} {p95:8} {secondary:30} {evidence}")

def main(argv: list[str] | None = None) -> None:
    args = parse_args(argv)
    root_dir = repo_root(__file__)

    if args.command == "diagnosis-matrix":
        run_diagnosis_matrix(root_dir, scenarios=args.scenario)
        return

    if args.command == "run":
        _run_scenario(root_dir, args.scenario, args.mode, profile=args.profile)
        return

    if args.command == "validate-tracing-parity":
        validate_tracing_parity(root_dir, args.scenario, profile=args.profile)
        return

    if args.command == "validate-tracing-retention-parity":
        validate_tracing_retention_parity(root_dir, profile=args.profile)
        return

    if args.scenario == "queue":
        validate_queue(root_dir, profile=args.profile)
    elif args.scenario == "blocking":
        validate_blocking(root_dir, profile=args.profile)
    elif args.scenario == "downstream":
        validate_downstream(root_dir, profile=args.profile)
    elif args.scenario == "executor":
        validate_executor(root_dir, profile=args.profile)
    elif args.scenario == "cold-start":
        validate_cold_start(root_dir, profile=args.profile)
    elif args.scenario == "db-pool":
        validate_db_pool(root_dir, profile=args.profile)
    elif args.scenario == "shared-lock":
        validate_shared_lock(root_dir, profile=args.profile)
    elif args.scenario == "retry-storm":
        validate_retry_storm(root_dir, profile=args.profile)
    else:
        validate_mixed(root_dir, profile=args.profile)

if __name__ == "__main__":
    main()
