#!/usr/bin/env python3
"""Run baseline/mitigated demo pairs and summarize evidence movement."""
from __future__ import annotations

import argparse
import json
import re
from pathlib import Path
from typing import Any

try:
    from _demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze, variant_paths
except ModuleNotFoundError:  # pragma: no cover
    from scripts._demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze, variant_paths

DEFAULT_OUT = Path("target/mitigation-runs.jsonl")
CONF_HIGH = {"high", "very_high"}

SCENARIO_MATRIX = {
    "queue": {"manifest": "demos/queue_service/Cargo.toml", "before_mode": "baseline", "after_mode": "mitigated", "targeted_suspect": "application_queue_saturation", "acceptable_after_primary": ["application_queue_saturation", "downstream_stage_dominates"], "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening", "top2_retains_target_or_expected_successor"], "notes": "Queue mitigation should reduce queue wait evidence and improve tail latency."},
    "blocking": {"manifest": "demos/blocking_service/Cargo.toml", "before_mode": "baseline", "after_mode": "mitigated", "targeted_suspect": "blocking_pool_pressure", "acceptable_after_primary": ["blocking_pool_pressure", "downstream_stage_dominates"], "expected_movements": ["p95_decreases", "blocking_queue_depth_decreases", "targeted_score_nonworsening"], "notes": "Blocking mitigation should reduce blocking queue pressure signals."},
    "downstream": {"manifest": "demos/downstream_service/Cargo.toml", "before_mode": "baseline", "after_mode": "mitigated", "targeted_suspect": "downstream_stage_dominates", "acceptable_after_primary": ["downstream_stage_dominates", "application_queue_saturation"], "expected_movements": ["p95_decreases", "service_share_decreases", "targeted_score_nonworsening", "top2_retains_target_or_expected_successor"], "notes": "Downstream mitigation should reduce stage-dominance evidence."},
    "db-pool": {"manifest": "demos/db_pool_service/Cargo.toml", "before_mode": "baseline", "after_mode": "mitigated", "targeted_suspect": "application_queue_saturation", "acceptable_after_primary": ["application_queue_saturation", "downstream_stage_dominates"], "expected_movements": ["p95_decreases", "queue_share_decreases", "targeted_score_nonworsening"], "notes": "DB/pool mitigation should reduce resource-local waiting and tail latency."},
}
DEFAULT_SCENARIOS = ["queue", "blocking", "downstream", "db-pool"]


def top2_kinds(report: dict[str, Any]) -> list[str]:
    primary = report.get("primary_suspect") or {}
    secondary = report.get("secondary_suspects") or []
    return [s.get("kind") for s in [primary, *secondary][:2] if s.get("kind")]


def suspect_score(report: dict[str, Any], kind: str) -> int | float | None:
    for suspect in [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]:
        if suspect.get("kind") == kind:
            return suspect.get("score")
    return None


def extract_blocking_queue_depth_p95(report: dict[str, Any]) -> int | None:
    suspects = [report.get("primary_suspect") or {}, *(report.get("secondary_suspects") or [])]
    pattern = re.compile(r"blocking queue depth(?:[^0-9]+p95)?[^0-9]+([0-9]+)", re.IGNORECASE)
    for suspect in suspects:
        for evidence in suspect.get("evidence") or []:
            m = pattern.search(evidence)
            if m:
                return int(m.group(1))
    return None


def delta(before: Any, after: Any) -> Any:
    if before is None or after is None:
        return None
    return after - before


def ratio_delta(before: Any, after: Any) -> float | None:
    if before in (None, 0) or after is None:
        return None
    return (after - before) / before


def build_pair_record(before_report: dict[str, Any], after_report: dict[str, Any], scenario_meta: dict[str, Any], paths: dict[str, Path], profile: str) -> dict[str, Any]:
    t = scenario_meta["targeted_suspect"]
    return {
        "schema_version": 1, "scenario": scenario_meta["name"], "profile": profile,
        "before_artifact_path": str(paths["before_artifact_path"]), "before_analysis_path": str(paths["before_analysis_path"]),
        "after_artifact_path": str(paths["after_artifact_path"]), "after_analysis_path": str(paths["after_analysis_path"]),
        "targeted_suspect": t,
        "before_primary_kind": (before_report.get("primary_suspect") or {}).get("kind"),
        "after_primary_kind": (after_report.get("primary_suspect") or {}).get("kind"),
        "before_top2_kinds": top2_kinds(before_report), "after_top2_kinds": top2_kinds(after_report),
        "before_p95_latency_us": before_report.get("p95_latency_us"), "after_p95_latency_us": after_report.get("p95_latency_us"),
        "p95_delta_us": delta(before_report.get("p95_latency_us"), after_report.get("p95_latency_us")),
        "p95_delta_ratio": ratio_delta(before_report.get("p95_latency_us"), after_report.get("p95_latency_us")),
        "before_p99_latency_us": before_report.get("p99_latency_us"), "after_p99_latency_us": after_report.get("p99_latency_us"),
        "p99_delta_us": delta(before_report.get("p99_latency_us"), after_report.get("p99_latency_us")),
        "p99_delta_ratio": ratio_delta(before_report.get("p99_latency_us"), after_report.get("p99_latency_us")),
        "before_targeted_score": suspect_score(before_report, t), "after_targeted_score": suspect_score(after_report, t),
        "targeted_score_delta": delta(suspect_score(before_report, t), suspect_score(after_report, t)),
        "before_p95_queue_share_permille": before_report.get("p95_queue_share_permille"), "after_p95_queue_share_permille": after_report.get("p95_queue_share_permille"),
        "queue_share_delta_permille": delta(before_report.get("p95_queue_share_permille"), after_report.get("p95_queue_share_permille")),
        "before_p95_service_share_permille": before_report.get("p95_service_share_permille"), "after_p95_service_share_permille": after_report.get("p95_service_share_permille"),
        "service_share_delta_permille": delta(before_report.get("p95_service_share_permille"), after_report.get("p95_service_share_permille")),
        "before_blocking_queue_depth_p95": extract_blocking_queue_depth_p95(before_report), "after_blocking_queue_depth_p95": extract_blocking_queue_depth_p95(after_report),
        "blocking_queue_depth_delta": delta(extract_blocking_queue_depth_p95(before_report), extract_blocking_queue_depth_p95(after_report)),
        "expected_movements": {}, "movement_passed": False, "failed_expectations": [], "notes": scenario_meta["notes"],
    }


def evaluate_movements(record: dict[str, Any], scenario_meta: dict[str, Any], thresholds: dict[str, Any]) -> dict[str, bool]:
    expected = scenario_meta["expected_movements"]
    results: dict[str, bool] = {}
    results["p95_decreases"] = (record.get("p95_delta_ratio") is not None and record["p95_delta_ratio"] <= -thresholds["min_p95_improvement_ratio"])
    results["p99_nonworsening"] = record.get("p99_delta_ratio") is None or record["p99_delta_ratio"] <= thresholds["min_p99_improvement_ratio"]
    results["queue_share_decreases"] = record.get("queue_share_delta_permille") is not None and record["queue_share_delta_permille"] < 0
    results["service_share_decreases"] = record.get("service_share_delta_permille") is not None and record["service_share_delta_permille"] < 0
    results["blocking_queue_depth_decreases"] = record.get("blocking_queue_depth_delta") is not None and record["blocking_queue_depth_delta"] < 0
    results["targeted_score_nonworsening"] = record.get("targeted_score_delta") is not None and record["targeted_score_delta"] <= 0
    results["targeted_score_decreases"] = record.get("targeted_score_delta") is not None and record["targeted_score_delta"] < 0
    results["top2_retains_target_or_expected_successor"] = (record["targeted_suspect"] in record["after_top2_kinds"]) or (record["after_primary_kind"] in scenario_meta.get("acceptable_after_primary", []))
    results["primary_changes_from_targeted"] = record["before_primary_kind"] == record["targeted_suspect"] and record["after_primary_kind"] != record["targeted_suspect"]
    failed = [k for k in expected if not results.get(k, False)]
    # guard: targeted score movement cannot be sole passing signal
    if expected == ["targeted_score_decreases"] and not failed:
        failed.append("targeted_score_decreases_requires_concrete_companion")
    high_wrong = (record.get("after_primary_confidence") in CONF_HIGH and record.get("after_primary_kind") not in scenario_meta.get("acceptable_after_primary", []))
    if high_wrong:
        failed.append("high_confidence_wrong_after_mitigation")
    record["expected_movements"] = {k: results[k] for k in expected if k in results}
    record["failed_expectations"] = failed
    record["movement_passed"] = len(failed) == 0
    return results


def summarize_records(records: list[dict[str, Any]], profile: str, max_high_confidence_wrong: int) -> dict[str, Any]:
    per = {}
    high_wrong = 0
    for r in records:
        if "high_confidence_wrong_after_mitigation" in r.get("failed_expectations", []):
            high_wrong += 1
        per[r["scenario"]] = {k: r.get(k) for k in ["movement_passed", "failed_expectations", "p95_delta_us", "p95_delta_ratio", "before_primary_kind", "after_primary_kind", "before_targeted_score", "after_targeted_score", "queue_share_delta_permille"]}
    passed = sum(1 for r in records if r.get("movement_passed"))
    summary = {"schema_version": 1, "profile": profile, "total_scenarios": len(records), "passed_scenarios": passed, "failed_scenarios": len(records)-passed, "movement_pass_rate": (passed/len(records) if records else 0.0), "high_confidence_wrong_count": high_wrong, "per_scenario": per, "failed_thresholds": []}
    if high_wrong > max_high_confidence_wrong:
        summary["failed_thresholds"].append(f"high_confidence_wrong_count {high_wrong} exceeds max {max_high_confidence_wrong}")
    return summary


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for rec in records:
            f.write(json.dumps(rec, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any], records: list[dict[str, Any]]) -> None:
    lines = ["# Mitigation validation scorecard", "", f"Profile: {summary['profile']}", "", "| Scenario | Passed | Targeted suspect | Before primary | After primary | p95 delta | Evidence movement | Notes |", "|---|---:|---|---|---|---:|---|---|"]
    for r in records:
        ratio = r.get("p95_delta_ratio")
        ratio_txt = "n/a" if ratio is None else f"{ratio*100:.1f}%"
        evidence = ", ".join([k for k,v in r.get("expected_movements",{}).items() if v]) or "none"
        lines.append(f"| {r['scenario']} | {'yes' if r['movement_passed'] else 'no'} | {r['targeted_suspect']} | {r['before_primary_kind']} | {r['after_primary_kind']} | {ratio_txt} | {evidence} | {r['notes']} |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines)+"\n", encoding="utf-8")


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
    p.add_argument("--require-expected-evidence-movement", action="store_true", default=True)
    return p.parse_args()


def main() -> None:
    args = parse_args()
    selected = args.scenarios or DEFAULT_SCENARIOS
    unknown = [s for s in selected if s not in SCENARIO_MATRIX]
    if unknown:
        raise SystemExit(f"unknown scenarios: {unknown}")
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    records = []
    thresholds = {"min_p95_improvement_ratio": args.min_p95_improvement_ratio, "min_p99_improvement_ratio": args.min_p99_improvement_ratio}
    for name in selected:
        spec = {**SCENARIO_MATRIX[name], "name": name}
        run_dir = args.artifact_root / name
        before_artifact, before_analysis = variant_paths(run_dir, "before")
        after_artifact, after_analysis = variant_paths(run_dir, "after")
        demo_manifest = root / spec["manifest"]
        run_and_analyze(demo_manifest, cli_manifest, before_artifact, before_analysis, spec["before_mode"], profile=args.profile)
        run_and_analyze(demo_manifest, cli_manifest, after_artifact, after_analysis, spec["after_mode"], profile=args.profile)
        br, ar = load_report_json(before_analysis), load_report_json(after_analysis)
        rec = build_pair_record(br, ar, spec, {"before_artifact_path": before_artifact, "before_analysis_path": before_analysis, "after_artifact_path": after_artifact, "after_analysis_path": after_analysis}, args.profile)
        rec["after_primary_confidence"] = (ar.get("primary_suspect") or {}).get("confidence")
        evaluate_movements(rec, spec, thresholds)
        records.append(rec)
    write_jsonl(args.out, records)
    summary = summarize_records(records, args.profile, args.max_high_confidence_wrong)
    for r in records:
        if not r["movement_passed"]:
            summary["failed_thresholds"].append(f"{r['scenario']}: failed expectations {r['failed_expectations']}")
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True)+"\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary, records)
    if summary["failed_thresholds"] and not args.no_fail_thresholds:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
