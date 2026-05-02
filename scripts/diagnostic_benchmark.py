#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

ALLOWED_KINDS = {
    "application_queue_saturation",
    "blocking_pool_pressure",
    "executor_pressure_suspected",
    "downstream_stage_dominates",
    "insufficient_evidence",
}
CONFIDENCE_BUCKETS = {"high", "medium", "low"}


def parse_args() -> argparse.Namespace:
    p = argparse.ArgumentParser(description="Run diagnostic validation benchmark corpus")
    p.add_argument("--manifest", required=True)
    p.add_argument("--output")
    p.add_argument("--min-top1", type=float, default=0.75)
    p.add_argument("--min-top2", type=float, default=0.90)
    return p.parse_args()


def _error(msg: str) -> SystemExit:
    return SystemExit(f"error: {msg}")


def load_manifest(path: Path) -> dict:
    data = json.loads(path.read_text(encoding="utf-8"))
    if not isinstance(data.get("cases"), list):
        raise _error("manifest must include cases array")
    seen = set()
    for case in data["cases"]:
        for k in ["id", "artifact", "artifact_type", "ground_truth", "acceptable_top2", "tags", "must_include_evidence", "allowed_warnings", "notes"]:
            if k not in case:
                raise _error(f"case missing required field: {k}")
        if case["id"] in seen:
            raise _error(f"duplicate case id: {case['id']}")
        seen.add(case["id"])
        if case["artifact_type"] != "analysis_report":
            raise _error(f"unsupported artifact_type for {case['id']}: {case['artifact_type']}")
        if case["ground_truth"] not in ALLOWED_KINDS:
            raise _error(f"unknown ground_truth for {case['id']}: {case['ground_truth']}")
        if case["ground_truth"] not in case["acceptable_top2"]:
            raise _error(f"acceptable_top2 must include ground_truth for {case['id']}")
    return data


def confidence_bucket(value: str) -> str:
    return value if value in CONFIDENCE_BUCKETS else "unknown"


def run(manifest_path: Path) -> dict:
    manifest = load_manifest(manifest_path)
    root = manifest_path.parent.parent.parent
    failed = []
    confusion = defaultdict(Counter)
    per_gt = defaultdict(lambda: {"cases": 0, "top1_correct": 0, "top2_correct": 0})
    conf_acc = defaultdict(lambda: {"cases": 0, "top1_correct": 0})
    top1 = 0
    top2 = 0
    req_evidence_pass = 0
    unexpected_warning_count = 0

    for case in manifest["cases"]:
        report = json.loads((root / case["artifact"]).read_text(encoding="utf-8"))
        primary = report["primary_suspect"]
        secondary = report.get("secondary_suspects", [])
        predicted = primary["kind"]
        top2_kinds = [predicted] + [s["kind"] for s in secondary[:1]]
        truths = case["acceptable_top2"]

        is_top1 = predicted == case["ground_truth"]
        is_top2 = any(k in truths for k in top2_kinds)
        top1 += int(is_top1)
        top2 += int(is_top2)

        per = per_gt[case["ground_truth"]]
        per["cases"] += 1
        per["top1_correct"] += int(is_top1)
        per["top2_correct"] += int(is_top2)
        confusion[case["ground_truth"]][predicted] += 1

        bucket = confidence_bucket(primary.get("confidence", "unknown"))
        conf_acc[bucket]["cases"] += 1
        conf_acc[bucket]["top1_correct"] += int(is_top1)

        all_evidence = list(primary.get("evidence", []))
        for s in secondary:
            all_evidence.extend(s.get("evidence", []))
        evidence_ok = all(any(needle in ev for ev in all_evidence) for needle in case["must_include_evidence"])
        req_evidence_pass += int(evidence_ok)

        warnings = report.get("warnings", [])
        allowed = case["allowed_warnings"]
        bad_warn = []
        for warning in warnings:
            if not any(token in warning for token in allowed):
                bad_warn.append(warning)
        unexpected_warning_count += len(bad_warn)

        if (not evidence_ok) or bad_warn:
            failed.append({"id": case["id"], "evidence_ok": evidence_ok, "unexpected_warnings": bad_warn})

    total = len(manifest["cases"])
    return {
        "total_cases": total,
        "top1_accuracy": top1 / total if total else 0.0,
        "top2_recall": top2 / total if total else 0.0,
        "required_evidence_pass_rate": req_evidence_pass / total if total else 0.0,
        "unexpected_warning_count": unexpected_warning_count,
        "per_ground_truth": dict(sorted(per_gt.items())),
        "confidence_bucket_accuracy": dict(sorted(conf_acc.items())),
        "confusion_matrix": {k: dict(v) for k, v in sorted(confusion.items())},
        "failed_cases": failed,
    }


def main() -> int:
    args = parse_args()
    metrics = run(Path(args.manifest))
    print(f"Cases: {metrics['total_cases']}")
    print(f"Top-1 accuracy: {metrics['top1_accuracy']:.3f}")
    print(f"Top-2 recall: {metrics['top2_recall']:.3f}")
    print(f"Required evidence pass rate: {metrics['required_evidence_pass_rate']:.3f}")
    print(f"Unexpected warnings: {metrics['unexpected_warning_count']}")
    if args.output:
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        Path(args.output).write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    if metrics["failed_cases"]:
        return 1
    if metrics["top1_accuracy"] < args.min_top1 or metrics["top2_recall"] < args.min_top2:
        return 1
    if metrics["unexpected_warning_count"] > 0:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
