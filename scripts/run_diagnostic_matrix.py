#!/usr/bin/env python3
"""Run repeated demo scenarios and summarize diagnostic stability metrics."""

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
except ModuleNotFoundError:  # pragma: no cover
    from scripts._demo_runner import PROFILE_CHOICES, load_report_json, repo_root, run_and_analyze

CONF_HIGH = {"high", "very_high"}
DEFAULT_OUT = Path("target/diagnostic-runs.jsonl")

SCENARIO_MATRIX = {
    "queue": {
        "manifest": "demos/queue_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "application_queue_saturation",
        "acceptable_primary": ["application_queue_saturation"],
        "required_top2": ["application_queue_saturation"],
        "top1_required": True,
    },
    "blocking": {
        "manifest": "demos/blocking_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "blocking_pool_pressure",
        "acceptable_primary": ["blocking_pool_pressure"],
        "required_top2": ["blocking_pool_pressure"],
        "top1_required": True,
    },
    "executor": {
        "manifest": "demos/executor_pressure_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "executor_pressure_suspected",
        "acceptable_primary": ["executor_pressure_suspected"],
        "required_top2": ["executor_pressure_suspected"],
        "top1_required": True,
    },
    "downstream": {
        "manifest": "demos/downstream_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "downstream_stage_dominates",
        "acceptable_primary": ["downstream_stage_dominates", "application_queue_saturation"],
        "required_top2": ["downstream_stage_dominates"],
        "top1_required": True,
    },
    "mixed": {
        "manifest": "demos/mixed_contention_service/Cargo.toml",
        "variant": "before",
        "demo_mode": "baseline",
        "ground_truth": "application_queue_saturation",
        "acceptable_primary": ["application_queue_saturation", "executor_pressure_suspected"],
        "required_top2": ["application_queue_saturation"],
        "top1_required": False,
    },
}

DEFAULT_SCENARIOS = ["queue", "blocking", "executor", "downstream"]


def _iqr(sorted_values: list[float]) -> float:
    n = len(sorted_values)
    if n < 2:
        return 0.0
    mid = n // 2
    lower = sorted_values[:mid]
    upper = sorted_values[mid + 1 :] if n % 2 else sorted_values[mid:]
    q1 = statistics.median(lower)
    q3 = statistics.median(upper)
    return float(q3 - q1)


def latency_stats(values: list[int]) -> dict[str, int] | None:
    if not values:
        return None
    ordered = sorted(values)
    return {
        "median": int(statistics.median(ordered)),
        "iqr": int(_iqr([float(v) for v in ordered])),
        "min": ordered[0],
        "max": ordered[-1],
    }


def build_record(*, report: dict[str, Any], metadata: dict[str, Any], run_index: int, profile: str, artifact_path: Path, analysis_path: Path) -> dict[str, Any]:
    primary = report.get("primary_suspect") or {}
    secondaries = report.get("secondary_suspects") or []
    top2 = [s.get("kind") for s in [primary, *secondaries][:2] if s.get("kind")]
    primary_kind = primary.get("kind")
    top1_ok = primary_kind == metadata["ground_truth"]
    top2_ok = all(kind in top2 for kind in metadata["required_top2"])
    high_conf_wrong = primary.get("confidence") in CONF_HIGH and primary_kind not in metadata["acceptable_primary"]

    return {
        "schema_version": 1,
        "run_index": run_index,
        "scenario": metadata["name"],
        "variant": metadata["variant"],
        "profile": profile,
        "artifact_path": str(artifact_path),
        "analysis_path": str(analysis_path),
        "ground_truth": metadata["ground_truth"],
        "primary_kind": primary_kind,
        "primary_confidence": primary.get("confidence"),
        "primary_score": primary.get("score"),
        "top2_kinds": top2,
        "top1_ok": top1_ok,
        "top2_ok": top2_ok,
        "high_confidence_wrong": high_conf_wrong,
        "warnings": report.get("warnings") or [],
        "request_count": report.get("request_count"),
        "p95_latency_us": report.get("p95_latency_us"),
        "p99_latency_us": report.get("p99_latency_us"),
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
        "p95_service_share_permille": report.get("p95_service_share_permille"),
    }


def summarize(records: list[dict[str, Any]], *, runs: int, profile: str) -> dict[str, Any]:
    total = len(records)
    top1 = sum(1 for r in records if r["top1_ok"]) / total if total else 0.0
    top2 = sum(1 for r in records if r["top2_ok"]) / total if total else 0.0
    high_wrong = sum(1 for r in records if r["high_confidence_wrong"])
    per: dict[str, dict[str, Any]] = {}

    by_scenario: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for record in records:
        by_scenario[record["scenario"]].append(record)

    for scenario, rows in by_scenario.items():
        count = len(rows)
        primary_counts = Counter(r.get("primary_kind") for r in rows)
        bucket = defaultdict(lambda: {"records": 0, "top1_correct": 0})
        for row in rows:
            conf = row.get("primary_confidence") or "unknown"
            bucket[conf]["records"] += 1
            bucket[conf]["top1_correct"] += 1 if row["top1_ok"] else 0
        conf_summary = {
            k: {**v, "accuracy": (v["top1_correct"] / v["records"] if v["records"] else 0.0)} for k, v in sorted(bucket.items())
        }
        per[scenario] = {
            "records": count,
            "top1_accuracy": sum(1 for r in rows if r["top1_ok"]) / count if count else 0.0,
            "top2_recall": sum(1 for r in rows if r["top2_ok"]) / count if count else 0.0,
            "high_confidence_wrong_count": sum(1 for r in rows if r["high_confidence_wrong"]),
            "primary_kind_counts": {str(k): v for k, v in primary_counts.items() if k is not None},
            "primary_stability": (max(primary_counts.values()) / count) if count else 0.0,
            "p95_latency_us": latency_stats([r["p95_latency_us"] for r in rows if r.get("p95_latency_us") is not None]),
            "p99_latency_us": latency_stats([r["p99_latency_us"] for r in rows if r.get("p99_latency_us") is not None]),
            "confidence_bucket_accuracy": conf_summary,
        }

    return {
        "schema_version": 1,
        "runs": runs,
        "profile": profile,
        "total_records": total,
        "top1_accuracy": top1,
        "top2_recall": top2,
        "high_confidence_wrong_count": high_wrong,
        "per_scenario": dict(sorted(per.items())),
        "failed_thresholds": [],
    }


def evaluate_thresholds(summary: dict[str, Any], scenario_defs: dict[str, dict[str, Any]], *, min_top1: float, min_top2: float, max_high_confidence_wrong: int) -> list[str]:
    failed: list[str] = []
    if summary["high_confidence_wrong_count"] > max_high_confidence_wrong:
        failed.append(f"overall high_confidence_wrong_count {summary['high_confidence_wrong_count']} exceeds max {max_high_confidence_wrong}")
    for scenario, metrics in summary["per_scenario"].items():
        scenario_def = scenario_defs[scenario]
        if scenario_def.get("top1_required", True) and metrics["top1_accuracy"] < min_top1:
            failed.append(f"{scenario}: top1_accuracy {metrics['top1_accuracy']:.3f} below {min_top1:.3f}")
        if metrics["top2_recall"] < min_top2:
            failed.append(f"{scenario}: top2_recall {metrics['top2_recall']:.3f} below {min_top2:.3f}")
        if metrics["high_confidence_wrong_count"] > max_high_confidence_wrong:
            failed.append(f"{scenario}: high_confidence_wrong_count {metrics['high_confidence_wrong_count']} exceeds max {max_high_confidence_wrong}")
    return failed


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps(r, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    lines = [
        "# Repeated-run diagnostic matrix scorecard",
        "",
        f"Profile: {summary['profile']}",
        f"Runs per scenario: {summary['runs']}",
        "",
        "| Scenario | Records | Top-1 | Top-2 | Primary stability | High-conf wrong | p95 median | p95 IQR |",
        "|---|---:|---:|---:|---:|---:|---:|---:|",
    ]
    for scenario, m in summary["per_scenario"].items():
        p95 = m.get("p95_latency_us") or {}
        lines.append(
            f"| {scenario} | {m['records']} | {m['top1_accuracy']:.3f} | {m['top2_recall']:.3f} | {m['primary_stability']:.3f} | {m['high_confidence_wrong_count']} | {p95.get('median', 'n/a')} | {p95.get('iqr', 'n/a')} |"
        )
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--runs", type=int, default=30)
    parser.add_argument("--out", type=Path, default=DEFAULT_OUT)
    parser.add_argument("--summary", type=Path)
    parser.add_argument("--scorecard", type=Path)
    parser.add_argument("--scenario", action="append", dest="scenarios")
    parser.add_argument("--profile", choices=PROFILE_CHOICES, default="dev")
    parser.add_argument("--artifact-root", type=Path, default=Path("target/diagnostic-matrix"))
    parser.add_argument("--keep-artifacts", action="store_true")
    parser.add_argument("--min-top1", type=float, default=0.95)
    parser.add_argument("--min-top2", type=float, default=1.0)
    parser.add_argument("--max-high-confidence-wrong", type=int, default=0)
    parser.add_argument("--no-fail-thresholds", action="store_true")
    return parser.parse_args()


def main() -> None:
    args = parse_args()
    if args.runs <= 0:
        raise SystemExit("--runs must be > 0")
    selected = args.scenarios or DEFAULT_SCENARIOS
    unknown = [s for s in selected if s not in SCENARIO_MATRIX]
    if unknown:
        raise SystemExit(f"unknown scenarios: {unknown}")
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")

    scenario_defs = {name: {**SCENARIO_MATRIX[name], "name": name} for name in selected}
    records: list[dict[str, Any]] = []
    for name, spec in scenario_defs.items():
        demo_manifest = root / spec["manifest"]
        run_dir = args.artifact_root / name / spec["variant"]
        run_dir.mkdir(parents=True, exist_ok=True)
        for i in range(1, args.runs + 1):
            run_path = run_dir / f"run-{i:03d}-run.json"
            analysis_path = run_dir / f"run-{i:03d}-analysis.json"
            run_and_analyze(demo_manifest, cli_manifest, run_path, analysis_path, spec["demo_mode"], profile=args.profile)
            report = load_report_json(analysis_path)
            records.append(build_record(report=report, metadata=spec, run_index=i, profile=args.profile, artifact_path=run_path, analysis_path=analysis_path))

    write_jsonl(args.out, records)
    summary = summarize(records, runs=args.runs, profile=args.profile)
    failed = evaluate_thresholds(summary, scenario_defs, min_top1=args.min_top1, min_top2=args.min_top2, max_high_confidence_wrong=args.max_high_confidence_wrong)
    summary["failed_thresholds"] = failed
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary)

    if not args.keep_artifacts and args.artifact_root.exists():
        shutil.rmtree(args.artifact_root)

    print(f"records={len(records)}")
    print(f"out={args.out}")
    print(f"summary={summary_path}")
    if args.scorecard:
        print(f"scorecard={args.scorecard}")
    if failed:
        for failure in failed:
            print(f"FAIL: {failure}")
        if not args.no_fail_thresholds:
            raise SystemExit(1)


if __name__ == "__main__":
    main()
