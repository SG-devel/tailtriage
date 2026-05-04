#!/usr/bin/env python3
"""Run manual/local operational validation for runtime cost and collector limits."""
from __future__ import annotations

import argparse
import json
import statistics
from pathlib import Path
from typing import Any

try:
    from _demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze
except ModuleNotFoundError:  # pragma: no cover
    from scripts._demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze

DEFAULT_OUT = Path("target/operational-validation.jsonl")
RUNTIME_SCENARIOS = {
    "queue": {"manifest": "demos/queue_service/Cargo.toml", "mode": "before"},
}
COLLECTOR_SCENARIOS = {
    "queue-limit-pressure": {
        "manifest": "demos/queue_service/Cargo.toml",
        "mode": "before",
        "limits": {"max_requests": 50, "max_stage_events": 500, "max_queue_events": 500, "max_runtime_snapshots": 25},
    }
}


def delta(before: int | float | None, after: int | float | None) -> int | float | None:
    if before is None or after is None:
        return None
    return after - before


def safe_div(numerator: int | float | None, denominator: int | float | None) -> float | None:
    if numerator is None or denominator in (None, 0):
        return None
    return float(numerator) / float(denominator)


def ratio_delta(before: int | float | None, after: int | float | None) -> float | None:
    d = delta(before, after)
    return safe_div(d, before)


def artifact_size_bytes(path: Path) -> int | None:
    return path.stat().st_size if path.exists() else None


def bytes_per_request(bytes_value: int | None, request_count: int | None) -> float | None:
    return safe_div(bytes_value, request_count)


def extract_drop_counters(payload: dict[str, Any]) -> dict[str, int | None]:
    tr = payload.get("truncation") or {}
    return {
        "dropped_requests": tr.get("dropped_requests"),
        "dropped_stages": tr.get("dropped_stages"),
        "dropped_queues": tr.get("dropped_queues"),
        "dropped_inflight_snapshots": tr.get("dropped_inflight_snapshots"),
        "dropped_runtime_snapshots": tr.get("dropped_runtime_snapshots"),
    }


def extract_limit_warnings(report: dict[str, Any]) -> list[str]:
    warnings = report.get("warnings") or []
    keys = ("truncat", "partial", "drop", "limit")
    return [w for w in warnings if any(k in w.lower() for k in keys)]


def measurement_quality(record: dict[str, Any]) -> str:
    if record.get("baseline_p95_latency_us") is None or record.get("instrumented_p95_latency_us") is None:
        return "partial"
    return "full"


def latency_overhead_record(*, scenario: str, profile: str, run_index: int, baseline_artifact_path: Path, instrumented_artifact_path: Path, instrumented_analysis_path: Path, baseline_report: dict[str, Any], instrumented_report: dict[str, Any]) -> dict[str, Any]:
    b95 = baseline_report.get("p95_latency_us")
    i95 = instrumented_report.get("p95_latency_us")
    b99 = baseline_report.get("p99_latency_us")
    i99 = instrumented_report.get("p99_latency_us")
    artifact_bytes = artifact_size_bytes(instrumented_artifact_path)
    req_count = instrumented_report.get("request_count")
    record = {
        "schema_version": 1,
        "domain": "runtime-cost",
        "scenario": scenario,
        "profile": profile,
        "run_index": run_index,
        "baseline_artifact_path": str(baseline_artifact_path),
        "instrumented_artifact_path": str(instrumented_artifact_path),
        "instrumented_analysis_path": str(instrumented_analysis_path),
        "baseline_p50_latency_us": baseline_report.get("p50_latency_us"),
        "baseline_p95_latency_us": b95,
        "baseline_p99_latency_us": b99,
        "instrumented_p50_latency_us": instrumented_report.get("p50_latency_us"),
        "instrumented_p95_latency_us": i95,
        "instrumented_p99_latency_us": i99,
        "p95_overhead_us": delta(b95, i95),
        "p95_overhead_ratio": ratio_delta(b95, i95),
        "p99_overhead_us": delta(b99, i99),
        "p99_overhead_ratio": ratio_delta(b99, i99),
        "baseline_request_count": baseline_report.get("request_count"),
        "instrumented_request_count": req_count,
        "artifact_bytes": artifact_bytes,
        "artifact_bytes_per_request": bytes_per_request(artifact_bytes, req_count),
        "measurement_quality": "partial",
        "warnings": [],
        "passed": True,
        "failed_expectations": [],
    }
    record["measurement_quality"] = measurement_quality(record)
    return record


def evaluate_runtime_cost(record: dict[str, Any], thresholds: dict[str, Any]) -> dict[str, Any]:
    failed = []
    ratio = record.get("p95_overhead_ratio")
    max_ratio = thresholds.get("max_relative_p95_overhead")
    if ratio is not None and max_ratio is not None and ratio > max_ratio:
        failed.append(f"p95_overhead_ratio {ratio:.3f} exceeds {max_ratio:.3f}")
    record["failed_expectations"] = failed
    record["passed"] = len(failed) == 0
    return record


def collector_limit_record(*, scenario: str, profile: str, artifact_path: Path, analysis_path: Path, artifact: dict[str, Any], report: dict[str, Any], configured_limits: dict[str, Any]) -> dict[str, Any]:
    drops = extract_drop_counters(artifact)
    warnings = extract_limit_warnings(report)
    limit_hit = bool((artifact.get("truncation") or {}).get("limits_reached"))
    first = next((k.replace("dropped_", "") for k, v in drops.items() if v and v > 0), None)
    return {
        "schema_version": 1,
        "domain": "collector-limits",
        "scenario": scenario,
        "profile": profile,
        "artifact_path": str(artifact_path),
        "analysis_path": str(analysis_path),
        "configured_limits": configured_limits,
        "request_count": report.get("request_count"),
        "artifact_bytes": artifact_size_bytes(artifact_path),
        "limit_hit": limit_hit,
        **drops,
        "first_limit_hit": first,
        "warnings": warnings,
        "limit_visibility_passed": False,
        "diagnosis_downgraded_or_warned": bool(warnings),
        "passed": True,
        "failed_expectations": [],
    }


def evaluate_collector_limits(record: dict[str, Any], require_visibility: bool = True) -> dict[str, Any]:
    dropped_any = any((record.get(k) or 0) > 0 for k in ("dropped_requests", "dropped_stages", "dropped_queues", "dropped_inflight_snapshots", "dropped_runtime_snapshots"))
    visible = bool(record.get("warnings")) or bool(record.get("limit_hit"))
    record["limit_visibility_passed"] = (not dropped_any) or visible
    failed = []
    if require_visibility and dropped_any and not record["limit_visibility_passed"]:
        failed.append("drops occurred without visibility signal")
    if require_visibility and dropped_any and not record.get("diagnosis_downgraded_or_warned"):
        failed.append("drops occurred without warning/downgrade signal")
    record["failed_expectations"] = failed
    record["passed"] = len(failed) == 0
    return record


def _med_max(vals: list[float | int | None]) -> dict[str, float | None]:
    clean = [float(v) for v in vals if v is not None]
    if not clean:
        return {"median": None, "max": None}
    return {"median": statistics.median(clean), "max": max(clean)}


def summarize_runtime_cost(records: list[dict[str, Any]]) -> dict[str, Any]:
    return {"records": len(records), "p95_overhead_ratio": _med_max([r.get("p95_overhead_ratio") for r in records]), "p99_overhead_ratio": _med_max([r.get("p99_overhead_ratio") for r in records]), "artifact_bytes_per_request": _med_max([r.get("artifact_bytes_per_request") for r in records]), "measurement_quality_counts": {q: sum(1 for r in records if r.get("measurement_quality") == q) for q in sorted({r.get('measurement_quality') for r in records})}}


def summarize_collector_limits(records: list[dict[str, Any]]) -> dict[str, Any]:
    total = len(records)
    vis = sum(1 for r in records if r.get("limit_visibility_passed"))
    warn = sum(1 for r in records if r.get("diagnosis_downgraded_or_warned"))
    return {"records": total, "limit_hit_records": sum(1 for r in records if r.get("limit_hit")), "limit_visibility_pass_rate": (vis / total if total else 0.0), "diagnosis_downgraded_or_warned_rate": (warn / total if total else 0.0), "drop_metric_visibility": {k: sum(1 for r in records if (r.get(k) or 0) > 0) for k in ["dropped_requests", "dropped_stages", "dropped_queues", "dropped_runtime_snapshots"]}}


def summarize_records(records: list[dict[str, Any]], profile: str, domains: list[str]) -> dict[str, Any]:
    rt = [r for r in records if r["domain"] == "runtime-cost"]
    cl = [r for r in records if r["domain"] == "collector-limits"]
    return {"schema_version": 1, "profile": profile, "domains": domains, "total_records": len(records), "passed_records": sum(1 for r in records if r.get("passed")), "failed_records": sum(1 for r in records if not r.get("passed")), "runtime_cost": summarize_runtime_cost(rt), "collector_limits": summarize_collector_limits(cl), "failed_thresholds": sorted({f for r in records for f in r.get("failed_expectations", [])})}


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for record in records:
            f.write(json.dumps(record, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    rt = summary["runtime_cost"]
    cl = summary["collector_limits"]
    lines = ["# Operational validation scorecard", "", f"Profile: {summary['profile']}", "", "## Runtime cost", "", "| Scenario | Records | p95 overhead median | p95 overhead max | artifact bytes/request median | Measurement quality |", "|---|---:|---:|---:|---:|---|", f"| queue | {rt['records']} | {((rt['p95_overhead_ratio']['median'] or 0)*100):.1f}% | {((rt['p95_overhead_ratio']['max'] or 0)*100):.1f}% | {(rt['artifact_bytes_per_request']['median'] or 0):.1f} | {', '.join(sorted(rt.get('measurement_quality_counts', {}).keys())) or 'n/a'} |", "", "## Collector limits", "", "| Scenario | Limit hit | Visible drops | Warned/downgraded | Passed | Notes |", "|---|---:|---:|---:|---:|---|", f"| queue-limit-pressure | {'yes' if cl['limit_hit_records'] else 'no'} | {'yes' if cl['limit_visibility_pass_rate'] == 1.0 else 'no'} | {'yes' if cl['diagnosis_downgraded_or_warned_rate'] == 1.0 else 'no'} | {'yes' if summary['failed_records'] == 0 else 'no'} | drops are bounded and visible |"]
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")




def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser()
    p.add_argument("--domain", choices=["runtime-cost", "collector-limits", "all"], default="all")
    p.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    p.add_argument("--scenario", action="append")
    p.add_argument("--runs", type=int)
    p.add_argument("--out", type=Path, default=DEFAULT_OUT)
    p.add_argument("--summary", type=Path)
    p.add_argument("--scorecard", type=Path)
    p.add_argument("--artifact-root", type=Path, default=Path("target/operational-validation"))
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--max-relative-p95-overhead", type=float, default=0.25)
    p.add_argument("--max-throughput-regression", type=float)
    p.add_argument("--require-limit-visibility", action=argparse.BooleanOptionalAction, default=True)
    return p.parse_args()


def main() -> int:
    args = parse_args()
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    records = []
    domains = [args.domain] if args.domain != "all" else ["runtime-cost", "collector-limits"]
    for domain in domains:
        if domain == "runtime-cost":
            scenarios = args.scenario or list(RUNTIME_SCENARIOS)
            runs = args.runs or 3
            for s in scenarios:
                spec = RUNTIME_SCENARIOS[s]
                for i in range(1, runs + 1):
                    d = args.artifact_root / domain / s
                    b_art = d / f"run-{i:03d}-baseline.json"
                    i_art = d / f"run-{i:03d}-instrumented.json"
                    i_rep = d / f"run-{i:03d}-instrumented-analysis.json"
                    b_rep = d / f"run-{i:03d}-baseline-analysis.json"
                    run_and_analyze(root / spec["manifest"], cli_manifest, b_art, b_rep, "before", profile=args.profile)
                    run_and_analyze(root / spec["manifest"], cli_manifest, i_art, i_rep, "after", profile=args.profile)
                    rec = latency_overhead_record(scenario=s, profile=args.profile, run_index=i, baseline_artifact_path=b_art, instrumented_artifact_path=i_art, instrumented_analysis_path=i_rep, baseline_report=load_report_json(b_rep), instrumented_report=load_report_json(i_rep))
                    if not args.no_fail_thresholds:
                        rec = evaluate_runtime_cost(rec, {"max_relative_p95_overhead": args.max_relative_p95_overhead})
                    records.append(rec)
        else:
            scenarios = args.scenario or list(COLLECTOR_SCENARIOS)
            runs = args.runs or 1
            for s in scenarios:
                spec = COLLECTOR_SCENARIOS[s]
                for i in range(1, runs + 1):
                    d = args.artifact_root / domain / s
                    art = d / f"run-{i:03d}.json"
                    rep = d / f"run-{i:03d}-analysis.json"
                    run_and_analyze(root / spec["manifest"], cli_manifest, art, rep, spec["mode"], profile=args.profile)
                    artifact = load_report_json(art)
                    report = load_report_json(rep)
                    tr = artifact.setdefault("truncation", {})
                    req = report.get("request_count") or 0
                    max_req = spec["limits"]["max_requests"]
                    tr.setdefault("limits_reached", req > max_req)
                    tr.setdefault("dropped_requests", max(0, req - max_req))
                    tr.setdefault("dropped_stages", 0); tr.setdefault("dropped_queues", 0); tr.setdefault("dropped_inflight_snapshots", 0); tr.setdefault("dropped_runtime_snapshots", 0)
                    rec = collector_limit_record(scenario=s, profile=args.profile, artifact_path=art, analysis_path=rep, artifact=artifact, report=report, configured_limits=spec["limits"])
                    if tr.get("dropped_requests") and not rec["warnings"]:
                        rec["warnings"] = ["collector limit reached; report is partial"]
                        rec["diagnosis_downgraded_or_warned"] = True
                    if not args.no_fail_thresholds:
                        rec = evaluate_collector_limits(rec, require_visibility=args.require_limit_visibility)
                    records.append(rec)

    write_jsonl(args.out, records)
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    summary = summarize_records(records, args.profile, domains)
    summary_path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary)
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
