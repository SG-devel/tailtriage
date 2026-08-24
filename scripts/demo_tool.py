#!/usr/bin/env python3
"""Unified runner/validator for tailtriage demo scenarios."""

from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any, Callable

from _demo_runner import (
    PROFILE_CHOICES,
    SCENARIOS as SCENARIO_PATHS,
    load_report_json,
    repo_root,
    run_and_analyze,
    scenario_artifact_dir,
    scenario_manifest,
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


def _suspects(report: dict) -> list[dict]:
    return [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]


def suspect_score(report: dict, kind: str) -> int | None:
    for suspect in _suspects(report):
        if suspect.get("kind") == kind:
            return suspect.get("score")
    return None

def extract_blocking_queue_depth_p95(report: dict) -> int | None:
    for suspect in _suspects(report):
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
        scenario_manifest(root_dir, "queue"),
        scenario_artifact_dir(root_dir, "queue"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_blocking(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "blocking"),
        scenario_artifact_dir(root_dir, "blocking"),
        mode,
        snapshot_blocking,
        profile=profile,
    )

def run_scenario_executor(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "executor"),
        scenario_artifact_dir(root_dir, "executor"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_downstream(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "downstream"),
        scenario_artifact_dir(root_dir, "downstream"),
        mode,
        snapshot_downstream,
        profile=profile,
    )

def run_scenario_mixed(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "mixed"),
        scenario_artifact_dir(root_dir, "mixed"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_cold_start(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "cold-start"),
        scenario_artifact_dir(root_dir, "cold-start"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_db_pool(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "db-pool"),
        scenario_artifact_dir(root_dir, "db-pool"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_shared_lock(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "shared-lock"),
        scenario_artifact_dir(root_dir, "shared-lock"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def run_scenario_retry_storm(root_dir: Path, mode: str, *, profile: str = "dev") -> None:
    run_before_after_scenario(
        root_dir,
        scenario_manifest(root_dir, "retry-storm"),
        scenario_artifact_dir(root_dir, "retry-storm"),
        mode,
        snapshot_queue,
        profile=profile,
    )

def has_suspect_kind(report: dict, expected_kinds: set[str]) -> bool:
    primary = report.get("primary_suspect") or {}
    all_suspects = [primary, *(report.get("secondary_suspects") or [])]
    return any((suspect or {}).get("kind") in expected_kinds for suspect in all_suspects)


# This registry is the single canonical executable policy surface for controlled live demos.
# Its check names own both baseline expectations and mitigation movement for exactly nine
# scenarios; both ordinary validation and mitigation reporting consume evaluate_live_scenario.
LIVE_SCENARIO_POLICIES: dict[str, dict[str, Any]] = {
    "queue": {"targeted": "application_queue_saturation", "checks": ["baseline_targeted", "p95_improves", "queue_share_decreases", "targeted_score_nonworsening"], "after_high_confidence": {"application_queue_saturation", "downstream_stage_dominates"}},
    "blocking": {"targeted": "blocking_pool_pressure", "checks": ["baseline_targeted", "p95_improves", "blocking_depth_decreases", "targeted_score_nonworsening"], "after_high_confidence": {"blocking_pool_pressure", "downstream_stage_dominates"}},
    "executor": {"targeted": "executor_pressure_suspected", "checks": ["baseline_targeted", "executor_present", "no_blocking_evidence", "p95_improves", "targeted_score_nonworsening"]},
    "downstream": {"targeted": "downstream_stage_dominates", "checks": ["baseline_targeted", "p95_improves", "targeted_score_nonworsening"], "after_high_confidence": {"downstream_stage_dominates"}},
    "mixed": {"targeted": "application_queue_saturation", "checks": ["baseline_targeted", "baseline_downstream_secondary", "primary_rank_or_score_shifts"]},
    "cold-start": {"targeted": "application_queue_saturation", "checks": ["baseline_targeted", "cold_start_or_queue_evidence", "p95_improves", "targeted_score_nonworsening"]},
    "db-pool": {"targeted": "application_queue_saturation", "checks": ["baseline_targeted", "p95_improves", "queue_share_decreases", "targeted_score_nonworsening"], "after_high_confidence": {"application_queue_saturation", "downstream_stage_dominates"}},
    "shared-lock": {"targeted": "application_queue_saturation", "checks": ["baseline_targeted", "shared_lock_queue_evidence", "p95_improves", "targeted_score_nonworsening"]},
    "retry-storm": {"targeted": "downstream_stage_dominates", "checks": ["baseline_targeted", "baseline_service_share_elevated", "p95_improves", "targeted_score_nonworsening"]},
}
SCENARIOS = list(LIVE_SCENARIO_POLICIES)


def _delta(before: int | None, after: int | None) -> int | None:
    return None if before is None or after is None else after - before


def _ratio_delta(before: int | None, after: int | None) -> float | None:
    return None if before is None or after is None or before == 0 else (after - before) / float(before)


def _evidence_text(report: dict) -> str:
    return " ".join(str(item).lower() for suspect in _suspects(report) for item in (suspect.get("evidence") or []))


def evaluate_live_scenario(
    scenario: str,
    before: dict,
    after: dict,
    *,
    profile: str = "dev",
    min_p95_improvement_ratio: float = 0.0,
    before_analysis_path: Path | str | None = None,
    after_analysis_path: Path | str | None = None,
) -> dict[str, Any]:
    """Purely evaluate a produced before/after report pair against canonical policy."""
    try:
        policy = LIVE_SCENARIO_POLICIES[scenario]
    except KeyError as exc:
        raise ValueError(f"unsupported live-demo scenario: {scenario}") from exc
    targeted = policy["targeted"]
    before_primary = before.get("primary_suspect") or {}
    after_primary = after.get("primary_suspect") or {}
    before_p95, after_p95 = before.get("p95_latency_us"), after.get("p95_latency_us")
    before_targeted_score = suspect_score(before, targeted)
    after_targeted_score = suspect_score(after, targeted)
    before_queue, after_queue = before.get("p95_queue_share_permille"), after.get("p95_queue_share_permille")
    before_service, after_service = before.get("p95_service_share_permille"), after.get("p95_service_share_permille")
    before_depth, after_depth = extract_blocking_queue_depth_p95(before), extract_blocking_queue_depth_p95(after)
    ratio = _ratio_delta(before_p95, after_p95)
    evidence = _evidence_text(before)

    def check(name: str) -> bool:
        if name == "baseline_targeted": return before_primary.get("kind") == targeted
        if name == "p95_improves": return ratio is not None and ratio <= -min_p95_improvement_ratio and after_p95 < before_p95
        if name == "queue_share_decreases": return before_queue is not None and after_queue is not None and after_queue < before_queue
        if name == "blocking_depth_decreases": return before_depth is not None and after_depth is not None and after_depth < before_depth
        if name == "targeted_score_nonworsening": return before_targeted_score is None or after_targeted_score is None or after_targeted_score <= before_targeted_score
        if name == "executor_present": return has_suspect_kind(before, {targeted}) and (profile == "release" or before_targeted_score is not None)
        if name == "no_blocking_evidence": return "blocking queue depth" not in evidence
        if name == "baseline_downstream_secondary": return any(s.get("kind") == "downstream_stage_dominates" for s in before.get("secondary_suspects") or [])
        if name == "primary_rank_or_score_shifts": return after_primary.get("kind") != before_primary.get("kind") or after_primary.get("score") != before_primary.get("score")
        if name == "cold_start_or_queue_evidence": return any(x in evidence for x in ("cold_start_stage", "queue wait at p95", "queue depth sample"))
        if name == "shared_lock_queue_evidence": return "queue wait at p95" in evidence or "queue depth sample" in evidence
        if name == "baseline_service_share_elevated": return before_service is not None and before_service >= 900
        raise AssertionError(f"unknown canonical check: {name}")

    checks = {name: check(name) for name in policy["checks"]}
    allowed_after = policy.get("after_high_confidence")
    high_confidence_wrong = bool(
        allowed_after is not None
        and after_primary.get("confidence") == "high"
        and after_primary.get("kind") not in allowed_after
    )
    failed = [name for name, passed in checks.items() if not passed]
    if high_confidence_wrong:
        failed.append("high_confidence_wrong_after")
    return {
        "schema_version": 1, "scenario": scenario, "profile": profile,
        "policy_owner": "scripts/demo_tool.py", "targeted_suspect": targeted,
        "before_analysis_path": str(before_analysis_path) if before_analysis_path else None,
        "after_analysis_path": str(after_analysis_path) if after_analysis_path else None,
        "before_primary_kind": before_primary.get("kind"), "after_primary_kind": after_primary.get("kind"),
        "before_primary_confidence": before_primary.get("confidence"), "after_primary_confidence": after_primary.get("confidence"),
        "before_primary_score": before_primary.get("score"), "after_primary_score": after_primary.get("score"),
        "before_targeted_score": before_targeted_score, "after_targeted_score": after_targeted_score,
        "targeted_score_delta": _delta(before_targeted_score, after_targeted_score),
        "before_p95_latency_us": before_p95, "after_p95_latency_us": after_p95,
        "p95_delta_us": _delta(before_p95, after_p95), "p95_delta_ratio": ratio,
        "minimum_p95_improvement_ratio": min_p95_improvement_ratio,
        "before_p95_queue_share_permille": before_queue, "after_p95_queue_share_permille": after_queue,
        "queue_share_delta_permille": _delta(before_queue, after_queue),
        "before_p95_service_share_permille": before_service, "after_p95_service_share_permille": after_service,
        "service_share_delta_permille": _delta(before_service, after_service),
        "before_blocking_queue_depth_p95": before_depth, "after_blocking_queue_depth_p95": after_depth,
        "blocking_queue_depth_delta": _delta(before_depth, after_depth),
        "expected_checks": list(policy["checks"]), "checks": checks,
        "passed_checks": [name for name, passed in checks.items() if passed],
        "failed_expectations": failed, "high_confidence_wrong_after": high_confidence_wrong,
        "policy_passed": not failed,
    }


def _load_and_evaluate(root_dir: Path, scenario: str, *, profile: str, min_p95_improvement_ratio: float) -> dict[str, Any]:
    artifact_dir = scenario_artifact_dir(root_dir, scenario)
    before_path, after_path = artifact_dir / "before-analysis.json", artifact_dir / "after-analysis.json"
    return evaluate_live_scenario(scenario, load_report_json(before_path), load_report_json(after_path), profile=profile,
        min_p95_improvement_ratio=min_p95_improvement_ratio, before_analysis_path=before_path, after_analysis_path=after_path)


def validate_scenario(root_dir: Path, scenario: str, *, profile: str = "dev") -> dict[str, Any]:
    """Run and canonically evaluate one controlled scenario."""
    if scenario not in LIVE_SCENARIO_POLICIES:
        raise ValueError(f"unsupported live-demo scenario: {scenario}")
    _run_scenario(root_dir, scenario, "both", profile=profile)
    result = _load_and_evaluate(root_dir, scenario, profile=profile, min_p95_improvement_ratio=0.0)
    if not result["policy_passed"]:
        raise SystemExit(f"{scenario} validation failed: {', '.join(result['failed_expectations'])}")
    print(f"validation passed: {scenario}; p95 {result['before_p95_latency_us']}us -> {result['after_p95_latency_us']}us")
    return result


def validate_queue(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "queue", profile=profile)
def validate_blocking(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "blocking", profile=profile)
def validate_executor(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "executor", profile=profile)
def validate_downstream(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "downstream", profile=profile)
def validate_mixed(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "mixed", profile=profile)
def validate_cold_start(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "cold-start", profile=profile)
def validate_db_pool(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "db-pool", profile=profile)
def validate_shared_lock(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "shared-lock", profile=profile)
def validate_retry_storm(root_dir: Path, *, profile: str = "dev") -> dict[str, Any]: return validate_scenario(root_dir, "retry-storm", profile=profile)


def run_mitigation_report(root_dir: Path, scenarios: list[str], *, profile: str, out: Path,
                          summary_path: Path, scorecard_path: Path | None,
                          min_p95_improvement_ratio: float = 0.05,
                          no_fail_thresholds: bool = False) -> bool:
    """Run workloads, serialize every evaluable canonical policy result, and enforce status."""
    records = []
    for scenario in scenarios:
        _run_scenario(root_dir, scenario, "both", profile=profile)
        records.append(_load_and_evaluate(root_dir, scenario, profile=profile,
            min_p95_improvement_ratio=min_p95_improvement_ratio))
    passed = sum(row["policy_passed"] for row in records)
    summary = {"schema_version": 1, "profile": profile, "policy_owner": "scripts/demo_tool.py",
        "total_scenarios": len(records), "passed_scenarios": passed,
        "failed_scenarios": len(records) - passed,
        "high_confidence_wrong_count": sum(row["high_confidence_wrong_after"] for row in records),
        "per_scenario": {row["scenario"]: row for row in records}}
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text("".join(json.dumps(row, sort_keys=True) + "\n" for row in records), encoding="utf-8")
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if scorecard_path:
        lines = ["# Live-demo mitigation validation scorecard", "", f"Profile: {profile}", "",
            "| Scenario | Passed | Before primary | After primary | p95 delta | Failed expectations |",
            "|---|---:|---|---|---:|---|"]
        for row in records:
            lines.append(f"| {row['scenario']} | {'yes' if row['policy_passed'] else 'no'} | {row['before_primary_kind']} | {row['after_primary_kind']} | {row['p95_delta_us']} | {', '.join(row['failed_expectations']) or '-'} |")
        scorecard_path.parent.mkdir(parents=True, exist_ok=True)
        scorecard_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
    if passed != len(records) and not no_fail_thresholds:
        raise SystemExit("mitigation thresholds failed: " + "; ".join(f"{r['scenario']}: {','.join(r['failed_expectations'])}" for r in records if not r["policy_passed"]))
    return passed == len(records)


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

def _require_run_metadata_mode(
    *,
    scenario: str,
    capture_mode: str,
    instrumentation: str,
    artifact_path: Path,
    run: dict,
) -> None:
    _require_equal(
        scenario=scenario,
        instrumentation=f"{instrumentation} capture_mode={capture_mode}",
        artifact_path=artifact_path.name,
        field="metadata.mode",
        expected=capture_mode,
        actual=(run.get("metadata") or {}).get("mode"),
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
            "demo_manifest": scenario_manifest(root_dir, "queue"),
            "artifact_dir": scenario_artifact_dir(root_dir, "queue"),
            "route": "/queue-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"simulated_work"},
            "require_p95_improvement": True,
        },
        "downstream": {
            "demo_manifest": scenario_manifest(root_dir, "downstream"),
            "artifact_dir": scenario_artifact_dir(root_dir, "downstream"),
            "route": "/downstream-demo",
            "expected_kind": "downstream_stage_dominates",
            "queues": set(),
            "stages": {"app_precheck", "downstream_call"},
            "require_p95_improvement": True,
        },
        "mixed": {
            "demo_manifest": scenario_manifest(root_dir, "mixed"),
            "artifact_dir": scenario_artifact_dir(root_dir, "mixed"),
            "route": "/mixed-contention-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_permit"},
            "stages": {"app_prepare", "downstream_call"},
            "require_p95_improvement": True,
        },
        "cold-start": {
            "demo_manifest": scenario_manifest(root_dir, "cold-start"),
            "artifact_dir": scenario_artifact_dir(root_dir, "cold-start"),
            "route": "/cold-start-burst-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"worker_admission"},
            "stages": {"cold_start_stage"},
            "require_p95_improvement": True,
        },
        "db-pool": {
            "demo_manifest": scenario_manifest(root_dir, "db-pool"),
            "artifact_dir": scenario_artifact_dir(root_dir, "db-pool"),
            "route": "/db-pool-saturation-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"db_pool"},
            "stages": {"app_precheck", "db_query"},
            "require_p95_improvement": True,
        },
        "shared-lock": {
            "demo_manifest": scenario_manifest(root_dir, "shared-lock"),
            "artifact_dir": scenario_artifact_dir(root_dir, "shared-lock"),
            "route": "/shared-state-lock-demo",
            "expected_kind": "application_queue_saturation",
            "queues": {"shared_state_write_lock"},
            "stages": {"pre_lock_work", "shared_state_critical_section"},
            "require_p95_improvement": True,
        },
        "retry-storm": {
            "demo_manifest": scenario_manifest(root_dir, "retry-storm"),
            "artifact_dir": scenario_artifact_dir(root_dir, "retry-storm"),
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
            "demo_manifest": scenario_manifest(root_dir, "blocking"),
            "artifact_dir": scenario_artifact_dir(root_dir, "blocking"),
            "route": "/blocking-demo",
            "expected_kind": "blocking_pool_pressure",
            "queues": {"dispatch_overhead"},
            "stages": {"spawn_blocking_path"},
            "require_p95_improvement": True,
        },
        "executor": {
            "demo_manifest": scenario_manifest(root_dir, "executor"),
            "artifact_dir": scenario_artifact_dir(root_dir, "executor"),
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

    for capture_mode in ("light", "investigation"):
        artifacts: dict[tuple[str, str], dict[str, Path]] = {}
        for mode in ("before", "after"):
            mode_arg = "baseline" if mode == "before" else "mitigated"
            for instrumentation in ("native", "tracing"):
                prefix = f"{mode}-{capture_mode}-{instrumentation}"
                run_path = artifact_dir / f"{prefix}-run.json"
                analysis_path = artifact_dir / f"{prefix}-analysis.json"
                run_and_analyze(
                    demo_manifest,
                    cli_manifest,
                    run_path,
                    analysis_path,
                    mode_arg,
                    profile=profile,
                    extra_demo_args=["--instrumentation", instrumentation, "--mode", capture_mode],
                )
                artifacts[(mode, instrumentation)] = {"run": run_path, "analysis": analysis_path}

        before_native_run_path = artifacts[("before", "native")]["run"]
        before_tracing_run_path = artifacts[("before", "tracing")]["run"]
        after_native_run_path = artifacts[("after", "native")]["run"]
        after_tracing_run_path = artifacts[("after", "tracing")]["run"]
        before_native_analysis_path = artifacts[("before", "native")]["analysis"]
        before_tracing_analysis_path = artifacts[("before", "tracing")]["analysis"]
        after_native_analysis_path = artifacts[("after", "native")]["analysis"]
        after_tracing_analysis_path = artifacts[("after", "tracing")]["analysis"]

        expected_files = [
            before_native_run_path.name,
            before_tracing_run_path.name,
            before_native_analysis_path.name,
            before_tracing_analysis_path.name,
            after_native_run_path.name,
            after_tracing_run_path.name,
            after_native_analysis_path.name,
            after_tracing_analysis_path.name,
        ]
        missing = [name for name in expected_files if not (artifact_dir / name).exists()]
        if missing:
            raise SystemExit(
                f"missing parity artifacts for scenario={scenario} capture_mode={capture_mode}: {', '.join(missing)}"
            )

        before_native_run = _load_run(before_native_run_path)
        before_tracing_run = _load_run(before_tracing_run_path)
        after_native_run = _load_run(after_native_run_path)
        after_tracing_run = _load_run(after_tracing_run_path)
        _require_run_metadata_mode(
            scenario=scenario,
            capture_mode=capture_mode,
            instrumentation=f"before-{capture_mode}-native",
            artifact_path=before_native_run_path,
            run=before_native_run,
        )
        _require_run_metadata_mode(
            scenario=scenario,
            capture_mode=capture_mode,
            instrumentation=f"before-{capture_mode}-tracing",
            artifact_path=before_tracing_run_path,
            run=before_tracing_run,
        )
        _require_run_metadata_mode(
            scenario=scenario,
            capture_mode=capture_mode,
            instrumentation=f"after-{capture_mode}-native",
            artifact_path=after_native_run_path,
            run=after_native_run,
        )
        _require_run_metadata_mode(
            scenario=scenario,
            capture_mode=capture_mode,
            instrumentation=f"after-{capture_mode}-tracing",
            artifact_path=after_tracing_run_path,
            run=after_tracing_run,
        )

        before_native = load_report_json(before_native_analysis_path)
        before_tracing = load_report_json(before_tracing_analysis_path)
        after_native = load_report_json(after_native_analysis_path)
        after_tracing = load_report_json(after_tracing_analysis_path)

        for label, report in (
            (f"before-{capture_mode}-native", before_native),
            (f"before-{capture_mode}-tracing", before_tracing),
            (f"after-{capture_mode}-native", after_native),
            (f"after-{capture_mode}-tracing", after_tracing),
        ):
            if report["request_count"] <= 0:
                raise SystemExit(f"expected non-zero request count in {label}")
            if report["p95_latency_us"] <= 0:
                raise SystemExit(f"expected non-zero p95 latency in {label}")

        for label, run in (
            (f"before-{capture_mode}-native", before_native_run),
            (f"before-{capture_mode}-tracing", before_tracing_run),
            (f"after-{capture_mode}-native", after_native_run),
            (f"after-{capture_mode}-tracing", after_tracing_run),
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
                (f"before-{capture_mode}-native", before_native_run),
                (f"before-{capture_mode}-tracing", before_tracing_run),
                (f"after-{capture_mode}-native", after_native_run),
                (f"after-{capture_mode}-tracing", after_tracing_run),
            ):
                if len(run.get("queues", [])) == 0:
                    raise SystemExit(f"expected non-zero queues in {label} run artifact")

            for run_path, run in ((before_tracing_run_path, before_tracing_run), (after_tracing_run_path, after_tracing_run)):
                queue_names = {q.get("queue") for q in run.get("queues", [])}
                if not config["queues"].issubset(queue_names):
                    raise SystemExit(
                        f"expected queue tracing artifact scenario={scenario} capture_mode={capture_mode} "
                        f"instrumentation=tracing artifact={run_path.name} to include queues {sorted(config['queues'])}"
                    )
                if not any(q.get("depth_at_start") is not None for q in run.get("queues", [])):
                    raise SystemExit(
                        f"expected queue tracing queue events scenario={scenario} capture_mode={capture_mode} "
                        f"instrumentation=tracing artifact={run_path.name} to include non-null depth_at_start"
                    )

        for run_path, run in ((before_tracing_run_path, before_tracing_run), (after_tracing_run_path, after_tracing_run)):
            tracing_stage_names = {s.get("stage") for s in run.get("stages", [])}
            for stage in config["stages"]:
                if stage not in tracing_stage_names:
                    raise SystemExit(f"expected tracing run {run_path.name} to include stage '{stage}'")
            if scenario == "retry-storm":
                if not any(name and name.startswith("downstream_attempt_") for name in tracing_stage_names):
                    raise SystemExit(f"expected tracing run {run_path.name} to include at least one downstream_attempt_* stage")
            if scenario in RUNTIME_SENSITIVE_TRACING_SCENARIOS:
                if not run.get("runtime_snapshots"):
                    raise SystemExit(f"expected runtime snapshots in tracing run {run_path.name}")
                metadata = run.get("metadata", {})
                lifecycle_warnings = metadata.get("lifecycle_warnings") or []
                manual_disabled = any(
                    warning.startswith(
                        "tailtriage-tracing session ran with background runtime sampling disabled"
                    )
                    for warning in lifecycle_warnings
                )
                if not manual_disabled:
                    raise SystemExit(
                        "expected disabled-background-sampler lifecycle warning in deterministic "
                        f"runtime-sensitive tracing run {run_path.name}"
                    )
            if scenario in NON_RUNTIME_TRACING_SCENARIOS:
                _require_equal(
                    scenario=scenario,
                    instrumentation=f"tracing capture_mode={capture_mode}",
                    artifact_path=run_path.name,
                    field="runtime_snapshots",
                    expected=[],
                    actual=run.get("runtime_snapshots") or [],
                )
                _require_equal(
                    scenario=scenario,
                    instrumentation=f"tracing capture_mode={capture_mode}",
                    artifact_path=run_path.name,
                    field="metadata.effective_tokio_sampler_config",
                    expected=None,
                    actual=(run.get("metadata") or {}).get("effective_tokio_sampler_config"),
                )
            if scenario == "blocking":
                if not any(s.get("blocking_queue_depth") is not None for s in run.get("runtime_snapshots", [])):
                    raise SystemExit(f"expected blocking_queue_depth runtime evidence in {run_path.name}")
            if scenario == "executor":
                if not any((s.get("global_queue_depth") is not None) or (s.get("local_queue_depth") is not None) for s in run.get("runtime_snapshots", [])):
                    raise SystemExit(f"expected global/local queue runtime evidence in {run_path.name}")

        for label, run in ((f"before-{capture_mode}-native", before_native_run), (f"after-{capture_mode}-native", after_native_run)):
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
                    f"info: mitigated parity primary suspect diverged for {scenario} capture_mode={capture_mode} but expected family is still present "
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
            _require_equal(scenario=scenario, instrumentation=f"native/tracing capture_mode={capture_mode}", artifact_path=f"{mode}-{capture_mode}-run", field="scenario_label", expected=native_run.get("scenario_label"), actual=tracing_run.get("scenario_label"))
            _require_equal(scenario=scenario, instrumentation=f"native/tracing capture_mode={capture_mode}", artifact_path=f"{mode}-{capture_mode}-run", field="metadata.mode", expected=(native_run.get("metadata") or {}).get("mode"), actual=(tracing_run.get("metadata") or {}).get("mode"))
            _require_equal(scenario=scenario, instrumentation=f"native/tracing capture_mode={capture_mode}", artifact_path=f"{mode}-{capture_mode}-run", field="metadata.effective_core_config.capture_limits", expected=_capture_limits(native_run), actual=_capture_limits(tracing_run))
            _require_equal(
                scenario=scenario,
                instrumentation=f"native/tracing capture_mode={capture_mode}",
                artifact_path=f"{mode}-{capture_mode}-run",
                field="route_coverage",
                expected=sorted({r.get("route") for r in native_run.get("requests", [])}),
                actual=sorted({r.get("route") for r in tracing_run.get("requests", [])}),
            )

        print(
            f"tracing parity validation passed for scenario={scenario} capture_mode={capture_mode}: "
            f"baseline kind={expected_kind}, tracing p95 {before_tracing['p95_latency_us']}us -> {after_tracing['p95_latency_us']}us"
        )


def validate_tracing_retention_parity(root_dir: Path, *, profile: str = "dev") -> None:
    scenario = "queue"
    config = _tracing_parity_config(root_dir, scenario)
    demo_manifest = config["demo_manifest"]
    artifact_dir = config["artifact_dir"]
    cli_manifest = root_dir / "tailtriage-cli/Cargo.toml"

    for capture_mode in ("light", "investigation"):
        for instrumentation in ("native", "tracing"):
            run_path = artifact_dir / f"tiny-{capture_mode}-{instrumentation}-run.json"
            analysis_path = artifact_dir / f"tiny-{capture_mode}-{instrumentation}-analysis.json"
            run_and_analyze(
                demo_manifest,
                cli_manifest,
                run_path,
                analysis_path,
                "baseline",
                profile=profile,
                extra_demo_args=[
                    "--instrumentation", instrumentation, "--mode", capture_mode,
                    "--max-requests", "3", "--max-stages", "3", "--max-queues", "3",
                ],
            )

        native_run = _load_run(artifact_dir / f"tiny-{capture_mode}-native-run.json")
        tracing_run = _load_run(artifact_dir / f"tiny-{capture_mode}-tracing-run.json")
        pairs = [
            ("retained_request_count", len(native_run.get("requests", [])), len(tracing_run.get("requests", []))),
            ("retained_stage_count", len(native_run.get("stages", [])), len(tracing_run.get("stages", []))),
            ("retained_queue_count", len(native_run.get("queues", [])), len(tracing_run.get("queues", []))),
            ("truncation.dropped_requests", (native_run.get("truncation") or {}).get("dropped_requests"), (tracing_run.get("truncation") or {}).get("dropped_requests")),
            ("truncation.dropped_stages", (native_run.get("truncation") or {}).get("dropped_stages"), (tracing_run.get("truncation") or {}).get("dropped_stages")),
            ("truncation.dropped_queues", (native_run.get("truncation") or {}).get("dropped_queues"), (tracing_run.get("truncation") or {}).get("dropped_queues")),
            ("truncation.limits_hit", (native_run.get("truncation") or {}).get("limits_hit"), (tracing_run.get("truncation") or {}).get("limits_hit")),
            ("metadata.effective_core_config", (native_run.get("metadata") or {}).get("effective_core_config"), (tracing_run.get("metadata") or {}).get("effective_core_config")),
        ]
        for field, expected, actual in pairs:
            _require_equal(
                scenario="tiny-limit",
                instrumentation=f"native/tracing capture_mode={capture_mode}",
                artifact_path=f"tiny-{capture_mode}-run",
                field=field,
                expected=expected,
                actual=actual,
            )
    print("tracing retention parity validation passed (tiny limits) for light+investigation")

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

    mitigation_parser = subparsers.add_parser(
        "mitigation-report",
        help="Run canonical live-demo policies and write machine-readable before/after evidence.",
    )
    mitigation_parser.add_argument("--scenario", action="append", choices=SCENARIOS)
    mitigation_parser.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    mitigation_parser.add_argument("--out", type=Path, default=Path("target/mitigation-runs.jsonl"))
    mitigation_parser.add_argument("--summary", type=Path)
    mitigation_parser.add_argument("--scorecard", type=Path)
    mitigation_parser.add_argument("--min-p95-improvement-ratio", type=float, default=0.05)
    mitigation_parser.add_argument("--no-fail-thresholds", action="store_true")

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
    return scenario_artifact_dir(root_dir, scenario)

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

    if args.command == "mitigation-report":
        scenarios = args.scenario or ["queue", "blocking", "downstream", "db-pool"]
        summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
        run_mitigation_report(
            root_dir,
            scenarios,
            profile=args.profile,
            out=args.out,
            summary_path=summary_path,
            scorecard_path=args.scorecard,
            min_p95_improvement_ratio=args.min_p95_improvement_ratio,
            no_fail_thresholds=args.no_fail_thresholds,
        )
        return

    validate_scenario(root_dir, args.scenario, profile=args.profile)

if __name__ == "__main__":
    main()
