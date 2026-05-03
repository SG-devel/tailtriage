#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import shutil
import statistics
from collections import Counter, defaultdict
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from _demo_runner import load_report_json, repo_root, run_and_analyze

CONF_HIGH = {"high", "very_high"}


@dataclass(frozen=True)
class ScenarioDef:
    name: str
    manifest_path: str
    variant: str
    ground_truth: str
    acceptable_primary: tuple[str, ...]
    required_top2: tuple[str, ...]
    top1_required: bool
    min_top1: float | None = None
    min_top2: float | None = None


def default_scenarios() -> dict[str, ScenarioDef]:
    return {
        "queue": ScenarioDef("queue", "demos/queue_service/Cargo.toml", "before", "application_queue_saturation", ("application_queue_saturation",), ("application_queue_saturation",), True),
        "blocking": ScenarioDef("blocking", "demos/blocking_service/Cargo.toml", "before", "blocking_pool_pressure", ("blocking_pool_pressure",), ("blocking_pool_pressure",), True),
        "executor": ScenarioDef("executor", "demos/executor_pressure_service/Cargo.toml", "before", "executor_pressure_suspected", ("executor_pressure_suspected",), ("executor_pressure_suspected",), True),
        "downstream": ScenarioDef("downstream", "demos/downstream_service/Cargo.toml", "before", "downstream_stage_dominates", ("downstream_stage_dominates",), ("downstream_stage_dominates",), True),
        "mixed": ScenarioDef("mixed", "demos/mixed_contention_service/Cargo.toml", "baseline", "application_queue_saturation", ("application_queue_saturation", "executor_pressure_suspected"), ("application_queue_saturation", "executor_pressure_suspected"), False, min_top2=0.95),
    }


def top2_kinds(report: dict[str, Any]) -> list[str]:
    suspects = [report.get("primary_suspect", {})] + report.get("secondary_suspects", [])
    return [s.get("kind") for s in suspects[:2] if isinstance(s, dict) and s.get("kind")]


def extract_run_record(report: dict[str, Any], scenario: ScenarioDef, run_index: int, profile: str, artifact_path: Path, analysis_path: Path) -> dict[str, Any]:
    primary = report.get("primary_suspect") or {}
    primary_kind = primary.get("kind")
    primary_conf = primary.get("confidence")
    kinds = top2_kinds(report)
    top1_ok = primary_kind == scenario.ground_truth
    top2_ok = all(req in kinds for req in scenario.required_top2)
    high_wrong = primary_conf in CONF_HIGH and primary_kind not in scenario.acceptable_primary
    return {
        "schema_version": 1,
        "run_index": run_index,
        "scenario": scenario.name,
        "variant": scenario.variant,
        "profile": profile,
        "artifact_path": str(artifact_path),
        "analysis_path": str(analysis_path),
        "ground_truth": scenario.ground_truth,
        "primary_kind": primary_kind,
        "primary_confidence": primary_conf,
        "primary_score": primary.get("score"),
        "top2_kinds": kinds,
        "top1_ok": top1_ok,
        "top2_ok": top2_ok,
        "high_confidence_wrong": high_wrong,
        "warnings": report.get("warnings", []),
        "request_count": report.get("request_count"),
        "p95_latency_us": report.get("p95_latency_us"),
        "p99_latency_us": report.get("p99_latency_us"),
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
        "p95_service_share_permille": report.get("p95_service_share_permille"),
    }


def iqr_stats(values: list[float | int]) -> dict[str, float | int] | None:
    if not values:
        return None
    data = sorted(values)
    n = len(data)
    mid = n // 2
    lower = data[:mid]
    upper = data[mid:] if n % 2 == 0 else data[mid + 1 :]
    q1 = statistics.median(lower) if lower else data[0]
    q3 = statistics.median(upper) if upper else data[-1]
    return {"median": statistics.median(data), "iqr": q3 - q1, "min": data[0], "max": data[-1]}


def summarize_records(records: list[dict[str, Any]], runs: int, profile: str) -> dict[str, Any]:
    total = len(records)
    top1 = (sum(1 for r in records if r["top1_ok"]) / total) if total else 0.0
    top2 = (sum(1 for r in records if r["top2_ok"]) / total) if total else 0.0
    high_wrong = sum(1 for r in records if r["high_confidence_wrong"])
    by_scenario = defaultdict(list)
    for r in records:
        by_scenario[r["scenario"]].append(r)
    per = {}
    for name, group in by_scenario.items():
        gtotal = len(group)
        pk = Counter(r["primary_kind"] for r in group)
        p95 = iqr_stats([r["p95_latency_us"] for r in group if r.get("p95_latency_us") is not None])
        p99 = iqr_stats([r["p99_latency_us"] for r in group if r.get("p99_latency_us") is not None])
        conf = defaultdict(lambda: {"records": 0, "top1_correct": 0})
        for r in group:
            bucket = r.get("primary_confidence") or "unknown"
            conf[bucket]["records"] += 1
            conf[bucket]["top1_correct"] += 1 if r["top1_ok"] else 0
        conf_out = {k: {**v, "accuracy": (v["top1_correct"] / v["records"]) if v["records"] else 0.0} for k, v in conf.items()}
        per[name] = {
            "records": gtotal,
            "top1_accuracy": sum(1 for r in group if r["top1_ok"]) / gtotal,
            "top2_recall": sum(1 for r in group if r["top2_ok"]) / gtotal,
            "high_confidence_wrong_count": sum(1 for r in group if r["high_confidence_wrong"]),
            "primary_kind_counts": dict(pk),
            "primary_stability": (max(pk.values()) / gtotal) if pk else 0.0,
            "p95_latency_us": p95,
            "p99_latency_us": p99,
            "confidence_bucket_accuracy": conf_out,
        }
    return {"schema_version": 1, "runs": runs, "profile": profile, "total_records": total, "top1_accuracy": top1, "top2_recall": top2, "high_confidence_wrong_count": high_wrong, "per_scenario": per, "failed_thresholds": []}


def evaluate_thresholds(summary: dict[str, Any], scenarios: list[ScenarioDef], min_top1: float, min_top2: float, max_high_wrong: int) -> list[str]:
    failures = []
    if summary["high_confidence_wrong_count"] > max_high_wrong:
        failures.append(f"overall high_confidence_wrong_count {summary['high_confidence_wrong_count']} exceeds max {max_high_wrong}")
    for s in scenarios:
        ps = summary["per_scenario"].get(s.name)
        if not ps:
            continue
        s_top1 = s.min_top1 if s.min_top1 is not None else (min_top1 if s.top1_required else None)
        s_top2 = s.min_top2 if s.min_top2 is not None else min_top2
        if s_top1 is not None and ps["top1_accuracy"] < s_top1:
            failures.append(f"{s.name} top1_accuracy {ps['top1_accuracy']:.3f} below threshold {s_top1:.3f}")
        if ps["top2_recall"] < s_top2:
            failures.append(f"{s.name} top2_recall {ps['top2_recall']:.3f} below threshold {s_top2:.3f}")
        if ps["high_confidence_wrong_count"] > max_high_wrong:
            failures.append(f"{s.name} high_confidence_wrong_count {ps['high_confidence_wrong_count']} exceeds max {max_high_wrong}")
    return failures


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps(r, sort_keys=True) + "\n")


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    lines = ["# Repeated-run diagnostic matrix scorecard", "", f"Profile: {summary['profile']}", f"Runs per scenario: {summary['runs']}", "", "| Scenario | Records | Top-1 | Top-2 | Primary stability | High-conf wrong | p95 median | p95 IQR |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
    for name, row in sorted(summary["per_scenario"].items()):
        p95 = row.get("p95_latency_us") or {}
        lines.append(f"| {name} | {row['records']} | {row['top1_accuracy']:.3f} | {row['top2_recall']:.3f} | {row['primary_stability']:.3f} | {row['high_confidence_wrong_count']} | {p95.get('median', 'n/a')} | {p95.get('iqr', 'n/a')} |")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--runs", type=int, default=30)
    ap.add_argument("--out", type=Path, default=Path("target/diagnostic-runs.jsonl"))
    ap.add_argument("--summary", type=Path)
    ap.add_argument("--scorecard", type=Path)
    ap.add_argument("--scenario", action="append", default=[])
    ap.add_argument("--profile", choices=["dev", "release"], default="dev")
    ap.add_argument("--artifact-root", type=Path, default=Path("target/diagnostic-matrix"))
    ap.add_argument("--keep-artifacts", action="store_true")
    ap.add_argument("--min-top1", type=float, default=0.95)
    ap.add_argument("--min-top2", type=float, default=1.0)
    ap.add_argument("--max-high-confidence-wrong", type=int, default=0)
    ap.add_argument("--no-fail-thresholds", action="store_true")
    args = ap.parse_args()
    scenario_map = default_scenarios()
    selected_names = args.scenario or ["queue", "blocking", "executor", "downstream"]
    selected = [scenario_map[n] for n in selected_names]
    root = repo_root(__file__)
    cli_manifest = root / "tailtriage-cli" / "Cargo.toml"
    records = []
    if not args.keep_artifacts and args.artifact_root.exists():
        shutil.rmtree(args.artifact_root)
    for s in selected:
        for idx in range(1, args.runs + 1):
            run_path = args.artifact_root / s.name / s.variant / f"run-{idx:03d}-run.json"
            analysis_path = args.artifact_root / s.name / s.variant / f"run-{idx:03d}-analysis.json"
            run_and_analyze(root / s.manifest_path, cli_manifest, run_path, analysis_path, s.variant, profile=args.profile)
            report = load_report_json(analysis_path)
            records.append(extract_run_record(report, s, idx, args.profile, run_path, analysis_path))
    write_jsonl(args.out, records)
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    summary = summarize_records(records, args.runs, args.profile)
    failures = evaluate_thresholds(summary, selected, args.min_top1, args.min_top2, args.max_high_confidence_wrong)
    summary["failed_thresholds"] = failures
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.scorecard:
        write_scorecard(args.scorecard, summary)
    if failures and not args.no_fail_thresholds:
        for f in failures:
            print(f"FAIL: {f}")
        return 1
    print(f"wrote {len(records)} records to {args.out}")
    print(f"wrote summary to {summary_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
