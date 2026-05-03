#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import statistics
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

try:
    from _demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze
except ModuleNotFoundError:
    from scripts._demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze

CONF_HIGH = {"high", "very_high"}

SCENARIO_MATRIX = {
    "queue": {
        "demo_manifest": "demos/queue_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "application_queue_saturation",
        "acceptable_primary": ["application_queue_saturation"],
        "required_top2": ["application_queue_saturation"],
        "top1_required": True,
    },
    "blocking": {
        "demo_manifest": "demos/blocking_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "blocking_pool_pressure",
        "acceptable_primary": ["blocking_pool_pressure"],
        "required_top2": ["blocking_pool_pressure"],
        "top1_required": True,
    },
    "executor": {
        "demo_manifest": "demos/executor_pressure_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "executor_pressure_suspected",
        "acceptable_primary": ["executor_pressure_suspected"],
        "required_top2": ["executor_pressure_suspected"],
        "top1_required": True,
    },
    "downstream": {
        "demo_manifest": "demos/downstream_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "downstream_stage_dominates",
        "acceptable_primary": ["downstream_stage_dominates", "application_queue_saturation"],
        "required_top2": ["downstream_stage_dominates"],
        "top1_required": True,
    },
    "mixed": {
        "demo_manifest": "demos/mixed_contention_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "application_queue_saturation",
        "acceptable_primary": ["application_queue_saturation", "executor_pressure_suspected"],
        "required_top2": ["application_queue_saturation"],
        "top1_required": False,
    },
}
DEFAULT_SCENARIOS = ["queue", "blocking", "executor", "downstream"]


def top2_kinds(report: dict[str, Any]) -> list[str]:
    primary = report.get("primary_suspect") or {}
    secondary = report.get("secondary_suspects") or []
    suspects = [primary, *secondary]
    return [s.get("kind") for s in suspects[:2] if isinstance(s, dict) and s.get("kind")]


def extract_run_record(report: dict[str, Any], *, scenario_name: str, scenario: dict[str, Any], run_index: int, profile: str, artifact_path: Path, analysis_path: Path) -> dict[str, Any]:
    primary = report.get("primary_suspect") or {}
    primary_kind = primary.get("kind")
    confidence = primary.get("confidence")
    required_top2 = scenario["required_top2"]
    top2 = top2_kinds(report)
    top1_ok = primary_kind == scenario["ground_truth"]
    top2_ok = all(kind in top2 for kind in required_top2)
    high_conf_wrong = confidence in CONF_HIGH and primary_kind not in scenario["acceptable_primary"]

    return {
        "schema_version": 1,
        "run_index": run_index,
        "scenario": scenario_name,
        "variant": scenario["variant"],
        "profile": profile,
        "artifact_path": str(artifact_path),
        "analysis_path": str(analysis_path),
        "ground_truth": scenario["ground_truth"],
        "primary_kind": primary_kind,
        "primary_confidence": confidence,
        "primary_score": primary.get("score"),
        "top2_kinds": top2,
        "top1_ok": top1_ok,
        "top2_ok": top2_ok,
        "high_confidence_wrong": high_conf_wrong,
        "warnings": report.get("warnings") if isinstance(report.get("warnings"), list) else [],
        "request_count": report.get("request_count"),
        "p95_latency_us": report.get("p95_latency_us"),
        "p99_latency_us": report.get("p99_latency_us"),
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
        "p95_service_share_permille": report.get("p95_service_share_permille"),
    }


def quartiles(values: list[int]) -> tuple[float, float]:
    vals = sorted(values)
    n = len(vals)
    if n == 1:
        return float(vals[0]), float(vals[0])
    lower = vals[: n // 2]
    upper = vals[n // 2 :] if n % 2 == 0 else vals[(n // 2) + 1 :]
    return float(statistics.median(lower)), float(statistics.median(upper))


def latency_stats(values: list[int]) -> dict[str, float | int] | None:
    if not values:
        return None
    q1, q3 = quartiles(values)
    return {
        "median": int(statistics.median(values)),
        "iqr": int(q3 - q1),
        "min": min(values),
        "max": max(values),
    }


def confidence_bucket_accuracy(records: list[dict[str, Any]]) -> dict[str, dict[str, Any]]:
    grouped: dict[str, dict[str, int]] = defaultdict(lambda: {"records": 0, "top1_correct": 0})
    for row in records:
        bucket = row.get("primary_confidence")
        if bucket is None:
            continue
        grouped[bucket]["records"] += 1
        if row.get("top1_ok"):
            grouped[bucket]["top1_correct"] += 1
    out = {}
    for bucket, data in grouped.items():
        recs = data["records"]
        out[bucket] = {
            "records": recs,
            "top1_correct": data["top1_correct"],
            "accuracy": (data["top1_correct"] / recs) if recs else 0.0,
        }
    return out


def summarize_records(records: list[dict[str, Any]], *, runs: int, profile: str) -> dict[str, Any]:
    by_scenario: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for row in records:
        by_scenario[row["scenario"]].append(row)

    per_scenario = {}
    for name, rows in by_scenario.items():
        primary_counts = Counter(row.get("primary_kind") for row in rows)
        p95_values = [row["p95_latency_us"] for row in rows if isinstance(row.get("p95_latency_us"), int)]
        p99_values = [row["p99_latency_us"] for row in rows if isinstance(row.get("p99_latency_us"), int)]
        per_scenario[name] = {
            "records": len(rows),
            "top1_accuracy": sum(1 for row in rows if row["top1_ok"]) / len(rows),
            "top2_recall": sum(1 for row in rows if row["top2_ok"]) / len(rows),
            "high_confidence_wrong_count": sum(1 for row in rows if row["high_confidence_wrong"]),
            "primary_kind_counts": dict(primary_counts),
            "primary_stability": (max(primary_counts.values()) / len(rows)) if rows else 0.0,
            "p95_latency_us": latency_stats(p95_values),
            "p99_latency_us": latency_stats(p99_values),
            "confidence_bucket_accuracy": confidence_bucket_accuracy(rows),
        }

    total = len(records)
    return {
        "schema_version": 1,
        "runs": runs,
        "profile": profile,
        "total_records": total,
        "top1_accuracy": (sum(1 for row in records if row["top1_ok"]) / total) if total else 0.0,
        "top2_recall": (sum(1 for row in records if row["top2_ok"]) / total) if total else 0.0,
        "high_confidence_wrong_count": sum(1 for row in records if row["high_confidence_wrong"]),
        "per_scenario": per_scenario,
        "failed_thresholds": [],
    }


def evaluate_thresholds(summary: dict[str, Any], scenarios: list[str], min_top1: float, min_top2: float, max_high_conf_wrong: int) -> list[str]:
    failures = []
    if summary["high_confidence_wrong_count"] > max_high_conf_wrong:
        failures.append(f"overall high_confidence_wrong_count {summary['high_confidence_wrong_count']} exceeds {max_high_conf_wrong}")
    for name in scenarios:
        info = summary["per_scenario"][name]
        spec = SCENARIO_MATRIX[name]
        if spec["top1_required"] and info["top1_accuracy"] < min_top1:
            failures.append(f"{name} top1_accuracy {info['top1_accuracy']:.3f} below {min_top1:.3f}")
        top2_req = 0.95 if name == "mixed" else min_top2
        if info["top2_recall"] < top2_req:
            failures.append(f"{name} top2_recall {info['top2_recall']:.3f} below {top2_req:.3f}")
        if info["high_confidence_wrong_count"] > max_high_conf_wrong:
            failures.append(f"{name} high_confidence_wrong_count {info['high_confidence_wrong_count']} exceeds {max_high_conf_wrong}")
    return failures


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for row in records:
            f.write(json.dumps(row, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    lines = ["# Repeated-run diagnostic matrix scorecard", "", f"Profile: {summary['profile']}", f"Runs per scenario: {summary['runs']}", "", "| Scenario | Records | Top-1 | Top-2 | Primary stability | High-conf wrong | p95 median | p95 IQR |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
    for scenario, data in summary["per_scenario"].items():
        p95 = data.get("p95_latency_us") or {}
        lines.append(
            f"| {scenario} | {data['records']} | {data['top1_accuracy']:.3f} | {data['top2_recall']:.3f} | {data['primary_stability']:.3f} | {data['high_confidence_wrong_count']} | {p95.get('median', 'n/a')} | {p95.get('iqr', 'n/a')} |"
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> None:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=30)
    ap.add_argument("--out", default="target/diagnostic-runs.jsonl")
    ap.add_argument("--summary")
    ap.add_argument("--scorecard")
    ap.add_argument("--scenario", action="append", choices=sorted(SCENARIO_MATRIX.keys()))
    ap.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    ap.add_argument("--artifact-root", default="target/diagnostic-matrix")
    ap.add_argument("--keep-artifacts", action="store_true")
    ap.add_argument("--min-top1", type=float, default=0.95)
    ap.add_argument("--min-top2", type=float, default=1.0)
    ap.add_argument("--max-high-confidence-wrong", type=int, default=0)
    ap.add_argument("--no-fail-thresholds", action="store_true")
    args = ap.parse_args()

    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    selected = args.scenario or DEFAULT_SCENARIOS
    artifact_root = Path(args.artifact_root)
    out_path = Path(args.out)
    summary_path = Path(args.summary) if args.summary else out_path.with_name(f"{out_path.stem}-summary.json")

    records = []
    for scenario_name in selected:
        scenario = SCENARIO_MATRIX[scenario_name]
        demo_manifest = root / scenario["demo_manifest"]
        scenario_artifact_dir = artifact_root / scenario_name / scenario["variant"]
        for run_index in range(1, args.runs + 1):
            run_path = scenario_artifact_dir / f"run-{run_index:03d}-run.json"
            analysis_path = scenario_artifact_dir / f"run-{run_index:03d}-analysis.json"
            run_and_analyze(demo_manifest, cli_manifest, run_path, analysis_path, scenario["demo_mode"], profile=args.profile)
            report = load_report_json(analysis_path)
            records.append(extract_run_record(report, scenario_name=scenario_name, scenario=scenario, run_index=run_index, profile=args.profile, artifact_path=run_path, analysis_path=analysis_path))

    write_jsonl(out_path, records)
    summary = summarize_records(records, runs=args.runs, profile=args.profile)
    failures = evaluate_thresholds(summary, selected, args.min_top1, args.min_top2, args.max_high_confidence_wrong)
    summary["failed_thresholds"] = failures
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(Path(args.scorecard), summary)

    if not args.keep_artifacts and artifact_root.exists():
        shutil.rmtree(artifact_root)

    if failures and not args.no_fail_thresholds:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
