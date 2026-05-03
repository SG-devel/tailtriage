#!/usr/bin/env python3
"""Run manual mitigation validation over paired baseline/mitigated demo scenarios."""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

try:
    from _demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze
except ModuleNotFoundError:  # pragma: no cover
    from scripts._demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze

CONF_HIGH = {"high", "very_high"}
DEFAULT_OUT = Path("target/mitigation-runs.jsonl")
DEFAULT_ARTIFACT_ROOT = Path("target/mitigation-matrix")
DEFAULT_SCENARIOS = ["queue", "blocking", "downstream", "db-pool"]

SCENARIOS = {
    "queue": {
        "demo_manifest": "demos/queue_service/Cargo.toml",
        "targeted_suspect": "application_queue_saturation",
        "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening"],
        "notes": "Queue mitigation should reduce queue-wait evidence and improve tail latency.",
    },
    "blocking": {
        "demo_manifest": "demos/blocking_service/Cargo.toml",
        "targeted_suspect": "blocking_pool_pressure",
        "expected_movements": ["p95_decreases", "blocking_queue_depth_decreases", "targeted_score_nonworsening"],
        "notes": "Blocking mitigation should reduce blocking backlog evidence.",
    },
    "downstream": {
        "demo_manifest": "demos/downstream_service/Cargo.toml",
        "targeted_suspect": "downstream_stage_dominates",
        "expected_movements": ["p95_decreases", "service_share_decreases", "targeted_score_nonworsening"],
        "notes": "Downstream mitigation should reduce stage/service dominance evidence.",
    },
    "db-pool": {
        "demo_manifest": "demos/db_pool_saturation_service/Cargo.toml",
        "targeted_suspect": "application_queue_saturation",
        "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening"],
        "notes": "DB/pool mitigation should reduce wait evidence represented as queue or stage contribution.",
    },
}


def top2_kinds(report: dict[str, Any]) -> list[str]:
    primary = report.get("primary_suspect") or {}
    secondaries = report.get("secondary_suspects") or []
    return [s.get("kind") for s in [primary, *secondaries][:2] if s.get("kind")]


def suspect_score(report: dict[str, Any], kind: str) -> int | None:
    suspects = [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]
    for suspect in suspects:
        if suspect.get("kind") == kind:
            return suspect.get("score")
    return None


def extract_blocking_queue_depth_p95(report: dict[str, Any]) -> int | None:
    suspects = [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]
    for suspect in suspects:
        for evidence in suspect.get("evidence") or []:
            match = re.search(r"Blocking queue depth p95 is (\d+)", evidence)
            if match:
                return int(match.group(1))
    return None


def delta(before: int | None, after: int | None) -> int | None:
    if before is None or after is None:
        return None
    return after - before


def ratio_delta(before: int | None, after: int | None) -> float | None:
    if before in (None, 0) or after is None:
        return None
    return (after - before) / before


def build_pair_record(before_report: dict[str, Any], after_report: dict[str, Any], scenario_meta: dict[str, Any], *, scenario: str, profile: str, before_artifact_path: Path, before_analysis_path: Path, after_artifact_path: Path, after_analysis_path: Path) -> dict[str, Any]:
    targeted = scenario_meta["targeted_suspect"]
    return {
        "schema_version": 1,
        "scenario": scenario,
        "profile": profile,
        "before_artifact_path": str(before_artifact_path),
        "before_analysis_path": str(before_analysis_path),
        "after_artifact_path": str(after_artifact_path),
        "after_analysis_path": str(after_analysis_path),
        "targeted_suspect": targeted,
        "before_primary_kind": (before_report.get("primary_suspect") or {}).get("kind"),
        "after_primary_kind": (after_report.get("primary_suspect") or {}).get("kind"),
        "before_top2_kinds": top2_kinds(before_report),
        "after_top2_kinds": top2_kinds(after_report),
        "before_p95_latency_us": before_report.get("p95_latency_us"),
        "after_p95_latency_us": after_report.get("p95_latency_us"),
        "p95_delta_us": delta(before_report.get("p95_latency_us"), after_report.get("p95_latency_us")),
        "p95_delta_ratio": ratio_delta(before_report.get("p95_latency_us"), after_report.get("p95_latency_us")),
        "before_p99_latency_us": before_report.get("p99_latency_us"),
        "after_p99_latency_us": after_report.get("p99_latency_us"),
        "p99_delta_us": delta(before_report.get("p99_latency_us"), after_report.get("p99_latency_us")),
        "p99_delta_ratio": ratio_delta(before_report.get("p99_latency_us"), after_report.get("p99_latency_us")),
        "before_targeted_score": suspect_score(before_report, targeted),
        "after_targeted_score": suspect_score(after_report, targeted),
        "targeted_score_delta": delta(suspect_score(before_report, targeted), suspect_score(after_report, targeted)),
        "before_p95_queue_share_permille": before_report.get("p95_queue_share_permille"),
        "after_p95_queue_share_permille": after_report.get("p95_queue_share_permille"),
        "queue_share_delta_permille": delta(before_report.get("p95_queue_share_permille"), after_report.get("p95_queue_share_permille")),
        "before_p95_service_share_permille": before_report.get("p95_service_share_permille"),
        "after_p95_service_share_permille": after_report.get("p95_service_share_permille"),
        "service_share_delta_permille": delta(before_report.get("p95_service_share_permille"), after_report.get("p95_service_share_permille")),
        "before_blocking_queue_depth_p95": extract_blocking_queue_depth_p95(before_report),
        "after_blocking_queue_depth_p95": extract_blocking_queue_depth_p95(after_report),
        "blocking_queue_depth_delta": delta(extract_blocking_queue_depth_p95(before_report), extract_blocking_queue_depth_p95(after_report)),
        "expected_movements": {},
        "movement_passed": False,
        "failed_expectations": [],
        "high_confidence_wrong": False,
        "notes": scenario_meta["notes"],
    }


def evaluate_movements(record: dict[str, Any], scenario_meta: dict[str, Any], thresholds: dict[str, Any]) -> dict[str, Any]:
    checks = {}
    checks["p95_decreases"] = (record["p95_delta_ratio"] is not None) and (record["p95_delta_ratio"] <= -thresholds["min_p95_improvement_ratio"])
    checks["p99_nonworsening"] = (record["p99_delta_ratio"] is None) or (record["p99_delta_ratio"] <= 0.0)
    checks["queue_share_decreases"] = (record["queue_share_delta_permille"] is None) or (record["queue_share_delta_permille"] < 0)
    checks["service_share_decreases"] = (record["service_share_delta_permille"] is None) or (record["service_share_delta_permille"] < 0)
    checks["blocking_queue_depth_decreases"] = (record["blocking_queue_depth_delta"] is None) or (record["blocking_queue_depth_delta"] < 0)
    checks["targeted_score_nonworsening"] = (record["targeted_score_delta"] is None) or (record["targeted_score_delta"] <= 0)
    checks["targeted_score_decreases"] = (record["targeted_score_delta"] is not None) and (record["targeted_score_delta"] < 0)
    checks["primary_changes_from_targeted"] = record["before_primary_kind"] == record["targeted_suspect"] and record["after_primary_kind"] != record["targeted_suspect"]
    checks["top2_retains_target_or_expected_successor"] = record["targeted_suspect"] in record["after_top2_kinds"]

    after_primary = record["after_primary_kind"]
    after_conf = scenario_meta.get("after_primary_confidence", "")
    record["high_confidence_wrong"] = bool(after_primary and after_primary not in [record["targeted_suspect"], *record["after_top2_kinds"][:1]] and after_conf in CONF_HIGH)

    expected = scenario_meta.get("expected_movements", [])
    record["expected_movements"] = {name: checks.get(name, False) for name in expected}
    failed = [name for name in expected if not checks.get(name, False)]

    if "targeted_score_decreases" in expected and len(expected) == 1:
        failed.append("targeted_score_decreases_requires_additional_concrete_movement")

    if record["high_confidence_wrong"]:
        failed.append("high_confidence_wrong_after_mitigation")

    record["failed_expectations"] = sorted(set(failed))
    record["movement_passed"] = len(record["failed_expectations"]) == 0
    return record


def summarize_records(records: list[dict[str, Any]], profile: str, max_high_confidence_wrong: int) -> dict[str, Any]:
    total = len(records)
    passed = sum(1 for r in records if r["movement_passed"])
    high_wrong = sum(1 for r in records if r.get("high_confidence_wrong"))
    per = {}
    for r in records:
        per[r["scenario"]] = {
            "movement_passed": r["movement_passed"],
            "failed_expectations": r["failed_expectations"],
            "p95_delta_us": r["p95_delta_us"],
            "p95_delta_ratio": r["p95_delta_ratio"],
            "before_primary_kind": r["before_primary_kind"],
            "after_primary_kind": r["after_primary_kind"],
            "before_targeted_score": r["before_targeted_score"],
            "after_targeted_score": r["after_targeted_score"],
            "queue_share_delta_permille": r["queue_share_delta_permille"],
        }
    failed_thresholds = []
    if high_wrong > max_high_confidence_wrong:
        failed_thresholds.append(f"high_confidence_wrong_count {high_wrong} exceeds max {max_high_confidence_wrong}")
    return {
        "schema_version": 1,
        "profile": profile,
        "total_scenarios": total,
        "passed_scenarios": passed,
        "failed_scenarios": total - passed,
        "movement_pass_rate": (passed / total) if total else 0.0,
        "high_confidence_wrong_count": high_wrong,
        "per_scenario": per,
        "failed_thresholds": failed_thresholds,
    }


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as fh:
        for r in records:
            fh.write(json.dumps(r, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any], records: list[dict[str, Any]]) -> None:
    lines = [
        "# Mitigation validation scorecard",
        "",
        f"Profile: {summary['profile']}",
        "",
        "| Scenario | Passed | Targeted suspect | Before primary | After primary | p95 delta | Evidence movement | Notes |",
        "|---|---:|---|---|---|---:|---|---|",
    ]
    for r in records:
        p95 = r.get("p95_delta_ratio")
        p95_str = "n/a" if p95 is None else f"{p95 * 100:.1f}%"
        moves = ", ".join(sorted(k for k, v in (r.get("expected_movements") or {}).items() if v)) or "none"
        lines.append(f"| {r['scenario']} | {'yes' if r['movement_passed'] else 'no'} | {r['targeted_suspect']} | {r['before_primary_kind']} | {r['after_primary_kind']} | {p95_str} | {moves} | {r['notes']} |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p.add_argument("--summary", type=Path)
    p.add_argument("--scorecard", type=Path)
    p.add_argument("--scenario", action="append", choices=sorted(SCENARIOS.keys()))
    p.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    p.add_argument("--artifact-root", type=Path, default=DEFAULT_ARTIFACT_ROOT)
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--min-p95-improvement-ratio", type=float, default=0.05)
    p.add_argument("--min-p99-improvement-ratio", type=float, default=0.0)
    p.add_argument("--max-high-confidence-wrong", type=int, default=0)
    p.add_argument("--require-expected-evidence-movement", action=argparse.BooleanOptionalAction, default=True)
    return p.parse_args()


def main() -> int:
    args = parse_args()
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    scenarios = args.scenario or DEFAULT_SCENARIOS
    records = []
    for scenario in scenarios:
        meta = SCENARIOS[scenario]
        artifact_dir = args.artifact_root / scenario
        before_artifact = artifact_dir / "before-run.json"
        before_analysis = artifact_dir / "before-analysis.json"
        after_artifact = artifact_dir / "after-run.json"
        after_analysis = artifact_dir / "after-analysis.json"
        run_and_analyze(root / meta["demo_manifest"], cli_manifest, before_artifact, before_analysis, "baseline", profile=args.profile)
        run_and_analyze(root / meta["demo_manifest"], cli_manifest, after_artifact, after_analysis, "mitigated", profile=args.profile)
        rec = build_pair_record(load_report_json(before_analysis), load_report_json(after_analysis), meta, scenario=scenario, profile=args.profile, before_artifact_path=before_artifact, before_analysis_path=before_analysis, after_artifact_path=after_artifact, after_analysis_path=after_analysis)
        rec = evaluate_movements(rec, meta, {"min_p95_improvement_ratio": args.min_p95_improvement_ratio, "min_p99_improvement_ratio": args.min_p99_improvement_ratio})
        records.append(rec)
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    write_jsonl(args.out, records)
    summary = summarize_records(records, args.profile, args.max_high_confidence_wrong)
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary, records)

    any_fail = any(not r["movement_passed"] for r in records) or bool(summary["failed_thresholds"])
    if args.no_fail_thresholds:
        return 0
    return 1 if any_fail else 0


if __name__ == "__main__":
    raise SystemExit(main())
