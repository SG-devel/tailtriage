#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import statistics
from collections import Counter, defaultdict
from pathlib import Path
from typing import Any

from _demo_runner import load_report_json, repo_root, run_and_analyze

SCHEMA_VERSION = 1
SINGLE_CAUSE_TOP1_DEFAULT = 0.95
SINGLE_CAUSE_TOP2_DEFAULT = 1.0


def scenario_matrix(root: Path) -> dict[str, dict[str, Any]]:
    return {
        "queue": {
            "scenario": "queue",
            "variant": "before",
            "demo_manifest": root / "demos/queue_service/Cargo.toml",
            "ground_truth": "application_queue_saturation",
            "acceptable_primary": {"application_queue_saturation"},
            "required_top2": {"application_queue_saturation"},
            "top1_required": True,
            "tags": ["single_cause"],
            "notes": "Controlled queue-heavy baseline.",
        },
        "blocking": {
            "scenario": "blocking",
            "variant": "before",
            "demo_manifest": root / "demos/blocking_service/Cargo.toml",
            "ground_truth": "blocking_pool_pressure",
            "acceptable_primary": {"blocking_pool_pressure"},
            "required_top2": {"blocking_pool_pressure"},
            "top1_required": True,
            "tags": ["single_cause"],
            "notes": "Controlled blocking-heavy baseline.",
        },
        "executor": {
            "scenario": "executor",
            "variant": "before",
            "demo_manifest": root / "demos/executor_pressure_service/Cargo.toml",
            "ground_truth": "executor_pressure_suspected",
            "acceptable_primary": {"executor_pressure_suspected"},
            "required_top2": {"executor_pressure_suspected"},
            "top1_required": True,
            "tags": ["single_cause"],
            "notes": "Controlled executor-pressure baseline.",
        },
        "downstream": {
            "scenario": "downstream",
            "variant": "before",
            "demo_manifest": root / "demos/downstream_service/Cargo.toml",
            "ground_truth": "downstream_stage_dominates",
            "acceptable_primary": {"downstream_stage_dominates", "application_queue_saturation"},
            "required_top2": {"downstream_stage_dominates"},
            "top1_required": True,
            "tags": ["single_cause"],
            "notes": "Controlled downstream-heavy baseline.",
        },
        "mixed": {
            "scenario": "mixed",
            "variant": "baseline",
            "demo_manifest": root / "demos/mixed_contention_service/Cargo.toml",
            "ground_truth": "application_queue_saturation",
            "acceptable_primary": {"application_queue_saturation", "executor_pressure_suspected"},
            "required_top2": {"application_queue_saturation"},
            "top1_required": False,
            "tags": ["mixed"],
            "notes": "Mixed contention baseline emphasizes top-2 visibility.",
        },
    }


def percentile_quartiles(values: list[float]) -> tuple[float, float]:
    ordered = sorted(values)
    n = len(ordered)
    if n == 1:
        return ordered[0], ordered[0]
    mid = n // 2
    lower = ordered[:mid]
    upper = ordered[mid:] if n % 2 == 0 else ordered[mid + 1 :]
    return statistics.median(lower), statistics.median(upper)


def summarize_latency(values: list[int]) -> dict[str, Any] | None:
    if not values:
        return None
    q1, q3 = percentile_quartiles([float(v) for v in values])
    return {
        "median": int(statistics.median(values)),
        "iqr": int(q3 - q1),
        "min": min(values),
        "max": max(values),
    }


def extract_run_record(report: dict[str, Any], meta: dict[str, Any], run_index: int, profile: str, artifact_path: Path, analysis_path: Path) -> dict[str, Any]:
    primary = report.get("primary_suspect") or {}
    secondary = report.get("secondary_suspects") or []
    second_kind = secondary[0].get("kind") if secondary else None
    top2 = [k for k in [primary.get("kind"), second_kind] if k is not None]
    required_top2 = meta["required_top2"]
    top2_ok = required_top2.issubset(set(top2))
    top1_ok = primary.get("kind") == meta["ground_truth"]
    high_conf_wrong = (
        (primary.get("confidence") in {"high", "very_high"})
        and (primary.get("kind") not in meta["acceptable_primary"])
    )
    return {
        "schema_version": SCHEMA_VERSION,
        "run_index": run_index,
        "scenario": meta["scenario"],
        "variant": meta["variant"],
        "profile": profile,
        "artifact_path": str(artifact_path),
        "analysis_path": str(analysis_path),
        "ground_truth": meta["ground_truth"],
        "primary_kind": primary.get("kind"),
        "primary_confidence": primary.get("confidence"),
        "primary_score": primary.get("score"),
        "top2_kinds": top2,
        "top1_ok": top1_ok,
        "top2_ok": top2_ok,
        "high_confidence_wrong": high_conf_wrong,
        "warnings": report.get("warnings", []),
        "request_count": report.get("request_count"),
        "p95_latency_us": report.get("p95_latency_us"),
        "p99_latency_us": report.get("p99_latency_us"),
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
        "p95_service_share_permille": report.get("p95_service_share_permille"),
    }


def confidence_bucket_accuracy(records: list[dict[str, Any]]) -> dict[str, Any]:
    buckets: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for r in records:
        if r.get("primary_confidence") is not None:
            buckets[r["primary_confidence"]].append(r)
    out: dict[str, Any] = {}
    for conf, rows in buckets.items():
        correct = sum(1 for r in rows if r.get("top1_ok"))
        out[conf] = {"records": len(rows), "top1_correct": correct, "accuracy": correct / len(rows)}
    return out


def summarize_records(records: list[dict[str, Any]], runs: int, profile: str) -> dict[str, Any]:
    total = len(records)
    top1 = sum(1 for r in records if r["top1_ok"]) / total if total else 0.0
    top2 = sum(1 for r in records if r["top2_ok"]) / total if total else 0.0
    high_wrong = sum(1 for r in records if r["high_confidence_wrong"])
    per_scenario: dict[str, Any] = {}
    grouped: dict[str, list[dict[str, Any]]] = defaultdict(list)
    for r in records:
        grouped[r["scenario"]].append(r)
    for scenario, rows in grouped.items():
        kinds = Counter(r.get("primary_kind") for r in rows)
        max_kind = kinds.most_common(1)[0][1] if kinds else 0
        per_scenario[scenario] = {
            "records": len(rows),
            "top1_accuracy": sum(1 for r in rows if r["top1_ok"]) / len(rows),
            "top2_recall": sum(1 for r in rows if r["top2_ok"]) / len(rows),
            "high_confidence_wrong_count": sum(1 for r in rows if r["high_confidence_wrong"]),
            "primary_kind_counts": dict(kinds),
            "primary_stability": max_kind / len(rows),
            "p95_latency_us": summarize_latency([r["p95_latency_us"] for r in rows if r.get("p95_latency_us") is not None]),
            "p99_latency_us": summarize_latency([r["p99_latency_us"] for r in rows if r.get("p99_latency_us") is not None]),
            "confidence_bucket_accuracy": confidence_bucket_accuracy(rows),
        }
    return {
        "schema_version": SCHEMA_VERSION,
        "runs": runs,
        "profile": profile,
        "total_records": total,
        "top1_accuracy": top1,
        "top2_recall": top2,
        "high_confidence_wrong_count": high_wrong,
        "per_scenario": per_scenario,
        "failed_thresholds": [],
    }


def evaluate_thresholds(summary: dict[str, Any], metas: dict[str, dict[str, Any]], min_top1: float, min_top2: float, max_high_conf_wrong: int) -> list[str]:
    failures: list[str] = []
    if summary["high_confidence_wrong_count"] > max_high_conf_wrong:
        failures.append("overall high_confidence_wrong_count exceeded")
    for scenario, data in summary["per_scenario"].items():
        meta = metas[scenario]
        if meta["top1_required"] and data["top1_accuracy"] < min_top1:
            failures.append(f"{scenario} top1_accuracy below {min_top1}")
        scenario_top2_min = 0.95 if "mixed" in meta["tags"] else min_top2
        if data["top2_recall"] < scenario_top2_min:
            failures.append(f"{scenario} top2_recall below {scenario_top2_min}")
        if data["high_confidence_wrong_count"] > max_high_conf_wrong:
            failures.append(f"{scenario} high_confidence_wrong_count exceeded")
    return failures


def write_jsonl(path: Path, records: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for r in records:
            f.write(json.dumps(r, sort_keys=True) + "\n")

def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    lines = ["# Repeated-run diagnostic matrix scorecard", "", f"Profile: {summary['profile']}", f"Runs per scenario: {summary['runs']}", "", "| Scenario | Records | Top-1 | Top-2 | Primary stability | High-conf wrong | p95 median | p95 IQR |", "|---|---:|---:|---:|---:|---:|---:|---:|"]
    for scenario, data in sorted(summary["per_scenario"].items()):
        p95 = data["p95_latency_us"] or {}
        lines.append(f"| {scenario} | {data['records']} | {data['top1_accuracy']:.3f} | {data['top2_recall']:.3f} | {data['primary_stability']:.3f} | {data['high_confidence_wrong_count']} | {p95.get('median','n/a')} | {p95.get('iqr','n/a')} |")
    path.write_text("\n".join(lines)+"\n", encoding="utf-8")


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
    ap.add_argument("--min-top1", type=float, default=SINGLE_CAUSE_TOP1_DEFAULT)
    ap.add_argument("--min-top2", type=float, default=SINGLE_CAUSE_TOP2_DEFAULT)
    ap.add_argument("--max-high-confidence-wrong", type=int, default=0)
    ap.add_argument("--no-fail-thresholds", action="store_true")
    args = ap.parse_args()
    summary_path = args.summary or args.out.with_name(f"{args.out.stem}-summary.json")
    root = repo_root(__file__)
    all_metas = scenario_matrix(root)
    selected = args.scenario or ["queue", "blocking", "executor", "downstream"]
    for s in selected:
        if s not in all_metas:
            raise SystemExit(f"unknown scenario: {s}")
    metas = {s: all_metas[s] for s in selected}
    cli_manifest = root / "tailtriage-cli/Cargo.toml"
    records = []
    for s, meta in metas.items():
        for run_index in range(1, args.runs + 1):
            run_dir = args.artifact_root / s / meta["variant"]
            run_path = run_dir / f"run-{run_index:03d}-run.json"
            analysis_path = run_dir / f"run-{run_index:03d}-analysis.json"
            mode_arg = "baseline" if meta["variant"] == "baseline" else meta["variant"]
            run_and_analyze(meta["demo_manifest"], cli_manifest, run_path, analysis_path, mode_arg, profile=args.profile)
            report = load_report_json(analysis_path)
            records.append(extract_run_record(report, meta, run_index, args.profile, run_path, analysis_path))
    write_jsonl(args.out, records)
    summary = summarize_records(records, args.runs, args.profile)
    failures = evaluate_thresholds(summary, metas, args.min_top1, args.min_top2, args.max_high_confidence_wrong)
    summary["failed_thresholds"] = failures
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(json.dumps(summary, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if args.scorecard:
        args.scorecard.parent.mkdir(parents=True, exist_ok=True)
        write_scorecard(args.scorecard, summary)
    if failures and not args.no_fail_thresholds:
        raise SystemExit("thresholds failed")
    return 0

if __name__ == "__main__":
    raise SystemExit(main())
