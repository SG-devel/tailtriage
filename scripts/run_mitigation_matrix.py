#!/usr/bin/env python3
"""Run paired baseline/mitigated demo scenarios and validate mitigation movement."""
from __future__ import annotations

import argparse
import json
import re
import sys
from pathlib import Path
from typing import Any

try:
    from _demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze
except ModuleNotFoundError:  # pragma: no cover
    from scripts._demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze

CONF_HIGH = {"high", "very_high"}
DEFAULT_OUT = Path("target/mitigation-runs.jsonl")

SCENARIOS = {
    "queue": {
        "demo_manifest": "demos/queue_service/Cargo.toml",
        "targeted_suspect": "application_queue_saturation",
        "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening"],
        "expected_after_top2": ["downstream_stage_dominates", "application_queue_saturation"],
        "notes": "Queue mitigation should reduce queue-wait evidence and improve tail latency.",
    },
    "blocking": {
        "demo_manifest": "demos/blocking_service/Cargo.toml",
        "targeted_suspect": "blocking_pool_pressure",
        "expected_movements": ["p95_decreases", "blocking_queue_depth_decreases", "targeted_score_nonworsening"],
        "notes": "Blocking mitigation should reduce blocking queue pressure and improve latency.",
    },
    "downstream": {
        "demo_manifest": "demos/downstream_service/Cargo.toml",
        "targeted_suspect": "downstream_stage_dominates",
        "expected_movements": ["p95_decreases", "service_share_decreases", "targeted_score_nonworsening"],
        "notes": "Downstream mitigation should reduce stage/service contribution and improve latency.",
    },
    "db-pool": {
        "demo_manifest": "demos/db_pool_saturation_service/Cargo.toml",
        "targeted_suspect": "application_queue_saturation",
        "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening"],
        "notes": "DB/pool mitigation should reduce queueing pressure and improve latency.",
    },
}
DEFAULT_SCENARIOS = ["queue", "blocking", "downstream", "db-pool"]


def top2_kinds(report: dict[str, Any]) -> list[str]:
    suspects = [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]
    return [s.get("kind") for s in suspects[:2] if s.get("kind")]


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
    if before is None or after is None or before == 0:
        return None
    return (after - before) / float(before)


def build_pair_record(before_report: dict[str, Any], after_report: dict[str, Any], scenario_meta: dict[str, Any], *, before_artifact: Path, before_analysis: Path, after_artifact: Path, after_analysis: Path, profile: str, scenario: str) -> dict[str, Any]:
    targeted = scenario_meta["targeted_suspect"]
    record = {
        "schema_version": 1,
        "scenario": scenario,
        "profile": profile,
        "before_artifact_path": str(before_artifact),
        "before_analysis_path": str(before_analysis),
        "after_artifact_path": str(after_artifact),
        "after_analysis_path": str(after_analysis),
        "targeted_suspect": targeted,
        "before_primary_kind": (before_report.get("primary_suspect") or {}).get("kind"),
        "after_primary_kind": (after_report.get("primary_suspect") or {}).get("kind"),
        "before_primary_confidence": (before_report.get("primary_suspect") or {}).get("confidence"),
        "after_primary_confidence": (after_report.get("primary_suspect") or {}).get("confidence"),
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
        "high_confidence_wrong_after": False,
        "notes": scenario_meta.get("notes"),
    }
    return record


def evaluate_movements(record: dict[str, Any], scenario_meta: dict[str, Any], thresholds: dict[str, Any]) -> dict[str, Any]:
    checks: dict[str, bool] = {}
    failed: list[str] = []
    for key in scenario_meta.get("expected_movements", []):
        if key == "p95_decreases":
            ratio = record.get("p95_delta_ratio")
            passed = ratio is not None and ratio <= -thresholds["min_p95_improvement_ratio"]
        elif key == "p99_nonworsening":
            d = record.get("p99_delta_us")
            passed = d is None or d <= 0
        elif key == "queue_share_decreases":
            d = record.get("queue_share_delta_permille")
            passed = d is not None and d < 0
        elif key == "service_share_decreases":
            d = record.get("service_share_delta_permille")
            passed = d is not None and d < 0
        elif key == "blocking_queue_depth_decreases":
            d = record.get("blocking_queue_depth_delta")
            passed = d is not None and d < 0
        elif key == "targeted_score_nonworsening":
            d = record.get("targeted_score_delta")
            passed = d is None or d <= 0
        elif key == "targeted_score_decreases":
            d = record.get("targeted_score_delta")
            passed = d is not None and d < 0
        elif key == "top2_retains_target_or_expected_successor":
            expected = set([record["targeted_suspect"], *(scenario_meta.get("expected_after_top2") or [])])
            passed = any(kind in expected for kind in (record.get("after_top2_kinds") or []))
        elif key == "primary_changes_from_targeted":
            passed = record.get("before_primary_kind") == record["targeted_suspect"] and record.get("after_primary_kind") != record["targeted_suspect"]
        else:
            passed = True
        checks[key] = passed
        if not passed:
            failed.append(key)

    after_conf = record.get("after_primary_confidence")
    record["high_confidence_wrong_after"] = bool(after_conf in CONF_HIGH and record.get("after_primary_kind") not in set(scenario_meta.get("expected_after_top2") or [record["targeted_suspect"]]))
    if record["high_confidence_wrong_after"]:
        failed.append("high_confidence_wrong_after")

    if checks.get("targeted_score_decreases") and len(checks) == 1:
        failed.append("targeted_score_only_not_sufficient")

    record["expected_movements"] = checks
    record["failed_expectations"] = failed
    record["movement_passed"] = len(failed) == 0
    return record


def summarize_records(records: list[dict[str, Any]], profile: str) -> dict[str, Any]:
    total = len(records)
    passed = sum(1 for r in records if r["movement_passed"])
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
    return {
        "schema_version": 1,
        "profile": profile,
        "total_scenarios": total,
        "passed_scenarios": passed,
        "failed_scenarios": total - passed,
        "movement_pass_rate": (passed / total) if total else 0.0,
        "high_confidence_wrong_count": sum(1 for r in records if r.get("high_confidence_wrong_after")),
        "per_scenario": dict(sorted(per.items())),
        "failed_thresholds": [],
    }


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for row in records:
            f.write(json.dumps(row, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any], records: list[dict[str, Any]]) -> None:
    by_scenario = {r["scenario"]: r for r in records}
    lines = ["# Mitigation validation scorecard", "", f"Profile: {summary['profile']}", "", "| Scenario | Passed | Targeted suspect | Before primary | After primary | p95 delta | Evidence movement | Notes |", "|---|---:|---|---|---|---:|---|---|"]
    for scenario, metrics in summary["per_scenario"].items():
        row = by_scenario[scenario]
        p95 = metrics.get("p95_delta_ratio")
        p95_txt = "n/a" if p95 is None else f"{p95*100:.1f}%"
        movement = ", ".join(k for k, v in (row.get("expected_movements") or {}).items() if v) or "none"
        lines.append(f"| {scenario} | {'yes' if metrics['movement_passed'] else 'no'} | {row['targeted_suspect']} | {metrics['before_primary_kind']} | {metrics['after_primary_kind']} | {p95_txt} | {movement} | {row.get('notes') or ''} |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p.add_argument("--summary", type=Path)
    p.add_argument("--scorecard", type=Path)
    p.add_argument("--scenario", action="append", choices=sorted(SCENARIOS))
    p.add_argument("--profile", default="dev", choices=PROFILE_CHOICES)
    p.add_argument("--artifact-root", type=Path, default=Path("target/mitigation-matrix"))
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--min-p95-improvement-ratio", type=float, default=0.05)
    p.add_argument("--min-p99-improvement-ratio", type=float, default=0.0)
    p.add_argument("--max-high-confidence-wrong", type=int, default=0)
    args = p.parse_args()

    scenarios = args.scenario or DEFAULT_SCENARIOS
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"

    records = []
    thresholds = {"min_p95_improvement_ratio": args.min_p95_improvement_ratio, "min_p99_improvement_ratio": args.min_p99_improvement_ratio}
    for scenario in scenarios:
        meta = dict(SCENARIOS[scenario])
        demo_manifest = root / meta["demo_manifest"]
        out_dir = root / args.artifact_root / scenario
        before_artifact = out_dir / "before-run.json"
        before_analysis = out_dir / "before-analysis.json"
        after_artifact = out_dir / "after-run.json"
        after_analysis = out_dir / "after-analysis.json"
        run_and_analyze(demo_manifest, cli_manifest, before_artifact, before_analysis, "baseline", profile=args.profile)
        run_and_analyze(demo_manifest, cli_manifest, after_artifact, after_analysis, "mitigated", profile=args.profile)
        before = load_report_json(before_analysis)
        after = load_report_json(after_analysis)
        meta["after_report"] = after
        rec = build_pair_record(before, after, meta, before_artifact=before_artifact, before_analysis=before_analysis, after_artifact=after_artifact, after_analysis=after_analysis, profile=args.profile, scenario=scenario)
        evaluate_movements(rec, meta, thresholds)
        records.append(rec)

    write_jsonl(args.out, records)
    summary = summarize_records(records, args.profile)
    failures = []
    if summary["high_confidence_wrong_count"] > args.max_high_confidence_wrong:
        failures.append("high_confidence_wrong_count exceeded")
    failures.extend([f"{r['scenario']}: {','.join(r['failed_expectations'])}" for r in records if not r["movement_passed"]])
    summary["failed_thresholds"] = failures
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary, records)
    if failures and not args.no_fail_thresholds:
        for f in failures:
            print(f"threshold failure: {f}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
