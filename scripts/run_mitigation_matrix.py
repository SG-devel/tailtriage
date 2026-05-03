#!/usr/bin/env python3
"""Run paired baseline/mitigated demos and summarize mitigation movement signals."""

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

SCENARIO_MATRIX: dict[str, dict[str, Any]] = {
    "queue": {
        "manifest": "demos/queue_service/Cargo.toml",
        "before_mode": "baseline",
        "after_mode": "mitigated",
        "targeted_suspect": "application_queue_saturation",
        "acceptable_after_primary": ["application_queue_saturation", "downstream_stage_dominates"],
        "expected_movements": ["p95_decreases", "queue_share_decreases", "top2_retains_target_or_expected_successor"],
        "expected_successor": "downstream_stage_dominates",
        "notes": "Queue mitigation should reduce queue-wait evidence and improve tail latency.",
    },
    "blocking": {
        "manifest": "demos/blocking_service/Cargo.toml",
        "before_mode": "baseline",
        "after_mode": "mitigated",
        "targeted_suspect": "blocking_pool_pressure",
        "acceptable_after_primary": ["blocking_pool_pressure", "downstream_stage_dominates"],
        "expected_movements": ["p95_decreases", "blocking_queue_depth_decreases"],
        "notes": "Blocking mitigation should reduce blocking-pool queue pressure evidence.",
    },
    "downstream": {
        "manifest": "demos/downstream_service/Cargo.toml",
        "before_mode": "baseline",
        "after_mode": "mitigated",
        "targeted_suspect": "downstream_stage_dominates",
        "acceptable_after_primary": ["downstream_stage_dominates", "application_queue_saturation"],
        "expected_movements": ["p95_decreases", "service_share_decreases"],
        "notes": "Downstream mitigation should reduce stage-dominance evidence and latency.",
    },
    "db-pool": {
        "manifest": "demos/db_pool_service/Cargo.toml",
        "before_mode": "baseline",
        "after_mode": "mitigated",
        "targeted_suspect": "application_queue_saturation",
        "acceptable_after_primary": ["application_queue_saturation", "downstream_stage_dominates"],
        "expected_movements": ["p95_decreases", "queue_share_decreases"],
        "notes": "DB/pool mitigation should reduce resource-local wait evidence.",
    },
}
DEFAULT_SCENARIOS = ["queue", "blocking", "downstream", "db-pool"]


def top2_kinds(report: dict[str, Any]) -> list[str]:
    primary = report.get("primary_suspect") or {}
    secondaries = report.get("secondary_suspects") or []
    return [s.get("kind") for s in [primary, *secondaries][:2] if s.get("kind")]


def suspect_score(report: dict[str, Any], kind: str) -> int | None:
    for suspect in [(report.get("primary_suspect") or {}), *((report.get("secondary_suspects") or []))]:
        if suspect.get("kind") == kind:
            return suspect.get("score")
    return None


def extract_blocking_queue_depth_p95(report: dict[str, Any]) -> int | None:
    suspects = [(report.get("primary_suspect") or {}), *((report.get("secondary_suspects") or []))]
    for suspect in suspects:
        for evidence in suspect.get("evidence") or []:
            m = re.search(r"(?:p95|95th)[^0-9]{0,12}(\d+)", str(evidence), flags=re.IGNORECASE)
            if "blocking" in str(evidence).lower() and m:
                return int(m.group(1))
    return None


def delta(before: int | None, after: int | None) -> int | None:
    return None if before is None or after is None else after - before


def ratio_delta(before: int | None, after: int | None) -> float | None:
    if before in (None, 0) or after is None:
        return None
    return (after - before) / before


def build_pair_record(before_report: dict[str, Any], after_report: dict[str, Any], scenario_meta: dict[str, Any], *, profile: str, before_artifact_path: Path, before_analysis_path: Path, after_artifact_path: Path, after_analysis_path: Path) -> dict[str, Any]:
    targeted = scenario_meta["targeted_suspect"]
    before_top2 = top2_kinds(before_report)
    after_top2 = top2_kinds(after_report)
    bp95 = before_report.get("p95_latency_us")
    ap95 = after_report.get("p95_latency_us")
    bp99 = before_report.get("p99_latency_us")
    ap99 = after_report.get("p99_latency_us")
    bq = before_report.get("p95_queue_share_permille")
    aq = after_report.get("p95_queue_share_permille")
    bs = before_report.get("p95_service_share_permille")
    a_s = after_report.get("p95_service_share_permille")
    bbd = extract_blocking_queue_depth_p95(before_report)
    abd = extract_blocking_queue_depth_p95(after_report)
    return {
        "schema_version": 1,
        "scenario": scenario_meta["name"],
        "profile": profile,
        "before_artifact_path": str(before_artifact_path),
        "before_analysis_path": str(before_analysis_path),
        "after_artifact_path": str(after_artifact_path),
        "after_analysis_path": str(after_analysis_path),
        "targeted_suspect": targeted,
        "before_primary_kind": (before_report.get("primary_suspect") or {}).get("kind"),
        "after_primary_kind": (after_report.get("primary_suspect") or {}).get("kind"),
        "before_top2_kinds": before_top2,
        "after_top2_kinds": after_top2,
        "before_p95_latency_us": bp95,
        "after_p95_latency_us": ap95,
        "p95_delta_us": delta(bp95, ap95),
        "p95_delta_ratio": ratio_delta(bp95, ap95),
        "before_p99_latency_us": bp99,
        "after_p99_latency_us": ap99,
        "p99_delta_us": delta(bp99, ap99),
        "p99_delta_ratio": ratio_delta(bp99, ap99),
        "before_targeted_score": suspect_score(before_report, targeted),
        "after_targeted_score": suspect_score(after_report, targeted),
        "targeted_score_delta": delta(suspect_score(before_report, targeted), suspect_score(after_report, targeted)),
        "before_p95_queue_share_permille": bq,
        "after_p95_queue_share_permille": aq,
        "queue_share_delta_permille": delta(bq, aq),
        "before_p95_service_share_permille": bs,
        "after_p95_service_share_permille": a_s,
        "service_share_delta_permille": delta(bs, a_s),
        "before_blocking_queue_depth_p95": bbd,
        "after_blocking_queue_depth_p95": abd,
        "blocking_queue_depth_delta": delta(bbd, abd),
        "expected_movements": {},
        "movement_passed": False,
        "failed_expectations": [],
        "high_confidence_wrong_after": False,
        "notes": scenario_meta["notes"],
    }


def evaluate_movements(record: dict[str, Any], scenario_meta: dict[str, Any], thresholds: dict[str, Any]) -> dict[str, bool]:
    expected = scenario_meta.get("expected_movements") or []
    out: dict[str, bool] = {}
    for key in expected:
        if key == "p95_decreases":
            ratio = record.get("p95_delta_ratio")
            out[key] = ratio is not None and ratio <= -float(thresholds["min_p95_improvement_ratio"])
        elif key == "p99_nonworsening":
            d = record.get("p99_delta_us")
            out[key] = d is not None and d <= 0
        elif key == "queue_share_decreases":
            d = record.get("queue_share_delta_permille")
            out[key] = d is not None and d < 0
        elif key == "service_share_decreases":
            d = record.get("service_share_delta_permille")
            out[key] = d is not None and d < 0
        elif key == "blocking_queue_depth_decreases":
            d = record.get("blocking_queue_depth_delta")
            out[key] = d is not None and d < 0
        elif key == "targeted_score_nonworsening":
            d = record.get("targeted_score_delta")
            out[key] = d is not None and d <= 0
        elif key == "targeted_score_decreases":
            d = record.get("targeted_score_delta")
            out[key] = d is not None and d < 0
        elif key == "top2_retains_target_or_expected_successor":
            top2 = set(record.get("after_top2_kinds") or [])
            out[key] = record["targeted_suspect"] in top2 or scenario_meta.get("expected_successor") in top2
        elif key == "primary_changes_from_targeted":
            out[key] = record.get("before_primary_kind") == record["targeted_suspect"] and record.get("after_primary_kind") != record["targeted_suspect"]
    return out


def summarize_records(records: list[dict[str, Any]], profile: str) -> dict[str, Any]:
    total = len(records)
    passed = sum(1 for r in records if r.get("movement_passed"))
    high_wrong = sum(1 for r in records if r.get("high_confidence_wrong_after"))
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
    return {"schema_version": 1, "profile": profile, "total_scenarios": total, "passed_scenarios": passed, "failed_scenarios": total - passed, "movement_pass_rate": (passed / total if total else 0.0), "high_confidence_wrong_count": high_wrong, "per_scenario": dict(sorted(per.items())), "failed_thresholds": [], "targeted_suspect_by_scenario": {r['scenario']: r.get('targeted_suspect') for r in records}}


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as handle:
        for r in records:
            handle.write(json.dumps(r, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    lines = ["# Mitigation validation scorecard", "", f"Profile: {summary['profile']}", "", "| Scenario | Passed | Targeted suspect | Before primary | After primary | p95 delta | Evidence movement | Notes |", "|---|---:|---|---|---|---:|---|---|"]
    for name, row in summary["per_scenario"].items():
        passed = "yes" if row["movement_passed"] else "no"
        p95 = row.get("p95_delta_ratio")
        p95_label = "n/a" if p95 is None else f"{p95 * 100:.1f}%"
        movement = "ok" if row["movement_passed"] else ", ".join(row["failed_expectations"])
        lines.append(f"| {name} | {passed} | {summary.get('targeted_suspect_by_scenario', {}).get(name, 'n/a')} | {row.get('before_primary_kind') or 'n/a'} | {row.get('after_primary_kind') or 'n/a'} | {p95_label} | {movement} | mitigation movement check |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p.add_argument("--summary", type=Path)
    p.add_argument("--scorecard", type=Path)
    p.add_argument("--scenario", action="append", dest="scenarios")
    p.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    p.add_argument("--artifact-root", type=Path, default=Path("target/mitigation-matrix"))
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--min-p95-improvement-ratio", type=float, default=0.05)
    p.add_argument("--min-p99-improvement-ratio", type=float, default=0.0)
    p.add_argument("--max-high-confidence-wrong", type=int, default=0)
    p.add_argument("--require-expected-evidence-movement", action=argparse.BooleanOptionalAction, default=True)
    return p.parse_args()


def main() -> None:
    args = parse_args()
    selected = args.scenarios or DEFAULT_SCENARIOS
    unknown = [s for s in selected if s not in SCENARIO_MATRIX]
    if unknown:
        raise SystemExit(f"unknown scenarios: {unknown}")
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    records = []
    for name in selected:
        spec = {**SCENARIO_MATRIX[name], "name": name}
        run_dir = args.artifact_root / name
        run_dir.mkdir(parents=True, exist_ok=True)
        before_artifact = run_dir / "before-run.json"
        before_analysis = run_dir / "before-analysis.json"
        after_artifact = run_dir / "after-run.json"
        after_analysis = run_dir / "after-analysis.json"
        manifest = root / spec["manifest"]
        run_and_analyze(manifest, cli_manifest, before_artifact, before_analysis, spec["before_mode"], profile=args.profile)
        run_and_analyze(manifest, cli_manifest, after_artifact, after_analysis, spec["after_mode"], profile=args.profile)
        record = build_pair_record(load_report_json(before_analysis), load_report_json(after_analysis), spec, profile=args.profile, before_artifact_path=before_artifact, before_analysis_path=before_analysis, after_artifact_path=after_artifact, after_analysis_path=after_analysis)
        after_primary = load_report_json(after_analysis).get("primary_suspect") or {}
        record["high_confidence_wrong_after"] = after_primary.get("confidence") in CONF_HIGH and after_primary.get("kind") not in spec.get("acceptable_after_primary", [])
        checks = evaluate_movements(record, spec, {"min_p95_improvement_ratio": args.min_p95_improvement_ratio, "min_p99_improvement_ratio": args.min_p99_improvement_ratio})
        record["expected_movements"] = checks
        failed = [k for k, ok in checks.items() if not ok]
        if record["high_confidence_wrong_after"]:
            failed.append("high_confidence_wrong_after")
        record["failed_expectations"] = failed
        record["movement_passed"] = len(failed) == 0 if args.require_expected_evidence_movement else not record["high_confidence_wrong_after"]
        records.append(record)
    write_jsonl(args.out, records)
    summary = summarize_records(records, args.profile)
    summary["failed_thresholds"] = [f"{r['scenario']}: {', '.join(r['failed_expectations'])}" for r in records if not r["movement_passed"]]
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary)
    if summary["high_confidence_wrong_count"] > args.max_high_confidence_wrong and not args.no_fail_thresholds:
        raise SystemExit(1)
    if summary["failed_scenarios"] > 0 and not args.no_fail_thresholds:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
