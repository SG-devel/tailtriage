#!/usr/bin/env python3
import argparse
import json
from collections import Counter, defaultdict
from pathlib import Path

ALLOWED_GROUND_TRUTH = {
    "application_queue_saturation",
    "blocking_pool_pressure",
    "executor_pressure_suspected",
    "downstream_stage_dominates",
    "insufficient_evidence",
}
CONF_HIGH = {"high"}
CONFIDENCE_ORDER = {"low": 0, "medium": 1, "high": 2}


def load_json(path):
    with path.open("r", encoding="utf-8") as f:
        return json.load(f)


def validate_manifest(manifest):
    if not isinstance(manifest, dict) or "cases" not in manifest or not isinstance(manifest["cases"], list):
        raise ValueError("manifest must be an object containing a cases list")
    if manifest.get("schema_version") != 1:
        raise ValueError("manifest schema_version must be 1")
    seen = set()
    for case in manifest["cases"]:
        for field in ["id", "artifact", "artifact_type", "ground_truth", "required_top2", "acceptable_primary", "tags", "must_include_evidence", "must_include_next_checks", "expected_warnings", "allowed_warnings", "top1_required", "notes"]:
            if field not in case:
                raise ValueError(f"case missing required field: {field}")
        cid = case["id"]
        if not isinstance(cid, str) or not cid.strip():
            raise ValueError("case id must be a non-empty string")
        if cid in seen:
            raise ValueError(f"duplicate case id: {cid}")
        seen.add(cid)
        if not isinstance(case["artifact"], str) or not case["artifact"].strip():
            raise ValueError(f"artifact must be a non-empty string for {cid}")
        if case["artifact_type"] not in {"analysis_report", "synthetic_analysis_report"}:
            raise ValueError(f"artifact_type must be analysis_report or synthetic_analysis_report for {cid}")
        gt = case["ground_truth"]
        if gt not in ALLOWED_GROUND_TRUTH:
            raise ValueError(f"unknown ground_truth for {cid}: {gt}")
        if not isinstance(case["required_top2"], list) or not case["required_top2"]:
            raise ValueError(f"required_top2 must be a non-empty list for {cid}")
        if any(kind not in ALLOWED_GROUND_TRUTH for kind in case["required_top2"]):
            raise ValueError(f"required_top2 contains unknown diagnosis kind for {cid}")
        if gt not in case["required_top2"]:
            raise ValueError(f"required_top2 must include ground_truth for {cid}")
        if not isinstance(case["acceptable_primary"], list) or not case["acceptable_primary"]:
            raise ValueError(f"acceptable_primary must be a non-empty list for {cid}")
        if any(kind not in ALLOWED_GROUND_TRUTH for kind in case["acceptable_primary"]):
            raise ValueError(f"acceptable_primary contains unknown diagnosis kind for {cid}")
        if gt not in case["acceptable_primary"]:
            raise ValueError(f"acceptable_primary must include ground_truth for {cid}")
        if not isinstance(case["tags"], list) or any((not isinstance(t, str) or not t.strip()) for t in case["tags"]):
            raise ValueError(f"tags must be a list of non-empty strings for {cid}")
        if not isinstance(case["must_include_evidence"], list) or any(not isinstance(e, str) for e in case["must_include_evidence"]):
            raise ValueError(f"must_include_evidence must be a list of strings for {cid}")
        if not isinstance(case["must_include_next_checks"], list) or any(not isinstance(e, str) for e in case["must_include_next_checks"]):
            raise ValueError(f"must_include_next_checks must be a list of strings for {cid}")
        if not isinstance(case["expected_warnings"], list) or any(not isinstance(w, str) for w in case["expected_warnings"]):
            raise ValueError(f"expected_warnings must be a list of strings for {cid}")
        if not isinstance(case["allowed_warnings"], list) or any(not isinstance(w, str) for w in case["allowed_warnings"]):
            raise ValueError(f"allowed_warnings must be a list of strings for {cid}")
        if "*" in case["expected_warnings"] or "*" in case["allowed_warnings"]:
            raise ValueError(f"wildcard '*' is not allowed in warnings lists for {cid}")
        if not isinstance(case["top1_required"], bool):
            raise ValueError(f"top1_required must be a bool for {cid}")
        if not isinstance(case["notes"], str) or not case["notes"].strip():
            raise ValueError(f"notes must be a non-empty string for {cid}")
        if "max_primary_confidence" in case:
            ceiling = case["max_primary_confidence"]
            if not isinstance(ceiling, str):
                raise ValueError(f"max_primary_confidence must be a string for {cid}")
            if ceiling not in CONFIDENCE_ORDER:
                raise ValueError(f"max_primary_confidence must be one of low/medium/high for {cid}")


def confidence_bucket(conf):
    if conf == "high":
        return "high"
    if conf == "medium":
        return "medium"
    if conf == "low":
        return "low"
    raise ValueError("report.primary_suspect.confidence must be one of low/medium/high")


def extract(report):
    if not isinstance(report, dict):
        raise ValueError("report must be a JSON object")
    if "primary_suspect" not in report or not isinstance(report["primary_suspect"], dict):
        raise ValueError("report.primary_suspect must be an object")
    if "secondary_suspects" not in report or not isinstance(report["secondary_suspects"], list):
        raise ValueError("report.secondary_suspects must be a list")
    if "warnings" not in report or not isinstance(report["warnings"], list):
        raise ValueError("report.warnings must be a list")

    primary = report["primary_suspect"]
    kind = primary.get("kind")
    if kind not in ALLOWED_GROUND_TRUTH:
        raise ValueError("report.primary_suspect.kind must be an allowed diagnosis kind")
    conf = primary.get("confidence")
    if not isinstance(conf, str):
        raise ValueError("report.primary_suspect.confidence must be a string bucket")
    confidence_bucket(conf)
    if "score" in primary and not isinstance(primary["score"], (int, float)):
        raise ValueError("report.primary_suspect.score must be numeric when present")
    if not isinstance(primary.get("evidence"), list) or not all(isinstance(e, str) for e in primary["evidence"]):
        raise ValueError("report.primary_suspect.evidence must be a list of strings")
    if "next_checks" in primary and (not isinstance(primary["next_checks"], list) or not all(isinstance(n, str) for n in primary["next_checks"])):
        raise ValueError("report.primary_suspect.next_checks must be a list of strings when present")

    secondary = report["secondary_suspects"]
    if not all(isinstance(s, dict) for s in secondary):
        raise ValueError("report.secondary_suspects must be a list of objects")
    for s in secondary:
        if "kind" in s and s["kind"] not in ALLOWED_GROUND_TRUTH:
            raise ValueError("report.secondary_suspects.kind must be an allowed diagnosis kind when present")
        if "confidence" in s:
            if not isinstance(s["confidence"], str) or s["confidence"] not in {"low", "medium", "high"}:
                raise ValueError("report.secondary_suspects.confidence must be one of low/medium/high when present")
        if "score" in s and not isinstance(s["score"], (int, float)):
            raise ValueError("report.secondary_suspects.score must be numeric when present")
        if "evidence" in s and (not isinstance(s["evidence"], list) or not all(isinstance(e, str) for e in s["evidence"])):
            raise ValueError("report.secondary_suspects.evidence must be a list of strings when present")
        if "next_checks" in s and (not isinstance(s["next_checks"], list) or not all(isinstance(n, str) for n in s["next_checks"])):
            raise ValueError("report.secondary_suspects.next_checks must be a list of strings when present")
    if not all(isinstance(w, str) for w in report["warnings"]):
        raise ValueError("report.warnings must be a list of strings")

    all_suspects = [primary] + secondary
    return {
        "top1": kind,
        "top2": [s.get("kind") for s in all_suspects[:2] if s.get("kind")],
        "primary_confidence": conf,
        "evidence": [e for s in all_suspects for e in s.get("evidence", [])],
        "warnings": report["warnings"],
        "next_checks": [n for s in all_suspects for n in s.get("next_checks", [])],
    }


def run(manifest_path, min_top1, min_top2, max_high_confidence_wrong):
    manifest_path = Path(manifest_path).resolve()
    root = manifest_path.parent
    manifest = load_json(manifest_path)
    validate_manifest(manifest)

    results = []
    failed_cases = []
    confusion = defaultdict(Counter)
    per_gt = Counter()
    evidence_pass = 0
    unexpected_warning_count = 0
    missing_expected_warning_count = 0
    high_conf_wrong = 0
    conf_buckets = defaultdict(lambda: {"total": 0, "correct": 0})
    next_check_required_cases = 0
    next_check_passed_cases = 0
    next_check_presence_cases = 0
    confidence_ceiling_cases = 0
    confidence_ceiling_passed_cases = 0

    for case in manifest["cases"]:
        report = load_json(root / case["artifact"])
        if case["artifact_type"] == "analysis_report" and "score" not in report.get("primary_suspect", {}):
            raise ValueError("analysis_report requires report.primary_suspect.score")
        ext = extract(report)

        gt = case["ground_truth"]
        per_gt[gt] += 1
        confusion[gt][ext["top1"]] += 1
        top1_ok = ext["top1"] == gt
        top2_ok = all(kind in ext["top2"] for kind in case["required_top2"])

        bucket = confidence_bucket(ext["primary_confidence"])
        conf_buckets[bucket]["total"] += 1
        if top1_ok:
            conf_buckets[bucket]["correct"] += 1
        if ext["primary_confidence"] in CONF_HIGH and ext["top1"] not in case["acceptable_primary"]:
            high_conf_wrong += 1

        ev_ok = all(any(req.lower() in ev.lower() for ev in ext["evidence"]) for req in case["must_include_evidence"])
        evidence_pass += 1 if ev_ok else 0

        required_next = case["must_include_next_checks"]
        next_check_ok = True
        if required_next:
            next_check_required_cases += 1
            next_check_ok = all(any(req.lower() in nc.lower() for nc in ext["next_checks"]) for req in required_next)
            if next_check_ok:
                next_check_passed_cases += 1
        if ext["next_checks"]:
            next_check_presence_cases += 1
        max_primary_confidence = case.get("max_primary_confidence")
        confidence_ceiling_ok = True
        if max_primary_confidence is not None:
            confidence_ceiling_cases += 1
            confidence_ceiling_ok = CONFIDENCE_ORDER[ext["primary_confidence"]] <= CONFIDENCE_ORDER[max_primary_confidence]
            if confidence_ceiling_ok:
                confidence_ceiling_passed_cases += 1

        unexpected = [w for w in ext["warnings"] if not any(exp.lower() in w.lower() for exp in (case["expected_warnings"] + case["allowed_warnings"]))]
        missing_expected = [exp for exp in case["expected_warnings"] if not any(exp.lower() in w.lower() for w in ext["warnings"])]
        unexpected_warning_count += len(unexpected)
        missing_expected_warning_count += len(missing_expected)

        case_failed = (not top2_ok) or (case["top1_required"] and not top1_ok) or (not ev_ok) or (not next_check_ok) or (not confidence_ceiling_ok) or bool(unexpected) or bool(missing_expected)
        row = {"id": case["id"], "top1_ok": top1_ok, "top2_ok": top2_ok, "evidence_ok": ev_ok, "next_check_ok": next_check_ok, "confidence_ceiling_ok": confidence_ceiling_ok, "max_primary_confidence": max_primary_confidence, "primary_confidence": ext["primary_confidence"], "unexpected_warnings": unexpected, "missing_expected_warnings": missing_expected}
        results.append(row)
        if case_failed:
            failed_cases.append({**row, "top1_required": case["top1_required"]})

    total = len(results)
    top1 = sum(1 for r in results if r["top1_ok"]) / total if total else 0.0
    top2 = sum(1 for r in results if r["top2_ok"]) / total if total else 0.0

    metrics = {
        "total_cases": total,
        "top1_accuracy": top1,
        "top2_recall": top2,
        "high_confidence_wrong_count": high_conf_wrong,
        "per_ground_truth_counts": dict(per_gt),
        "confusion_matrix": {k: dict(v) for k, v in confusion.items()},
        "confidence_bucket_accuracy": {k: {"accuracy": (v["correct"] / v["total"] if v["total"] else 0.0), **v} for k, v in conf_buckets.items()},
        "required_evidence_pass_rate": (evidence_pass / total) if total else 0.0,
        "next_check_required_cases": next_check_required_cases,
        "next_check_passed_cases": next_check_passed_cases,
        "next_check_presence_rate": (next_check_presence_cases / total) if total else 0.0,
        "next_check_pass_rate": (next_check_passed_cases / next_check_required_cases) if next_check_required_cases else None,
        "confidence_ceiling_cases": confidence_ceiling_cases,
        "confidence_ceiling_passed_cases": confidence_ceiling_passed_cases,
        "confidence_ceiling_pass_rate": (confidence_ceiling_passed_cases / confidence_ceiling_cases) if confidence_ceiling_cases else None,
        "unexpected_warning_count": unexpected_warning_count,
        "missing_expected_warning_count": missing_expected_warning_count,
        "failed_cases": failed_cases,
    }

    failures = []
    if failed_cases:
        failures.append("one or more per-case checks failed (top2/top1_required/evidence/warnings/next_checks)")
    if top1 < min_top1:
        failures.append(f"top1_accuracy {top1:.3f} below threshold {min_top1:.3f}")
    if top2 < min_top2:
        failures.append(f"top2_recall {top2:.3f} below threshold {min_top2:.3f}")
    if high_conf_wrong > max_high_confidence_wrong:
        failures.append(f"high_confidence_wrong_count {high_conf_wrong} exceeds max {max_high_confidence_wrong}")
    return metrics, failures


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", required=True)
    ap.add_argument("--output")
    ap.add_argument("--min-top1", type=float, default=0.75)
    ap.add_argument("--min-top2", type=float, default=0.90)
    ap.add_argument("--max-high-confidence-wrong", type=int, default=0)
    args = ap.parse_args()
    try:
        metrics, failures = run(args.manifest, args.min_top1, args.min_top2, args.max_high_confidence_wrong)
    except Exception as exc:
        print(f"ERROR: {exc}")
        raise SystemExit(1)
    if args.output:
        out = Path(args.output)
        out.parent.mkdir(parents=True, exist_ok=True)
        out.write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8")

    next_check_pass_rate = metrics["next_check_pass_rate"]
    next_check_pass_rate_text = "n/a" if next_check_pass_rate is None else f"{next_check_pass_rate:.3f}"
    print(f"total_cases={metrics['total_cases']}")
    print(f"top1_accuracy={metrics['top1_accuracy']:.3f}")
    print(f"top2_recall={metrics['top2_recall']:.3f}")
    print(f"high_confidence_wrong_count={metrics['high_confidence_wrong_count']}")
    print(f"required_evidence_pass_rate={metrics['required_evidence_pass_rate']:.3f}")
    confidence_ceiling_pass_rate = metrics["confidence_ceiling_pass_rate"]
    confidence_ceiling_pass_rate_text = "n/a" if confidence_ceiling_pass_rate is None else f"{confidence_ceiling_pass_rate:.3f}"
    print(f"confidence_ceiling_cases={metrics['confidence_ceiling_cases']}")
    print(f"confidence_ceiling_passed_cases={metrics['confidence_ceiling_passed_cases']}")
    print(f"confidence_ceiling_pass_rate={confidence_ceiling_pass_rate_text}")
    print(f"unexpected_warning_count={metrics['unexpected_warning_count']}")
    print(f"missing_expected_warning_count={metrics['missing_expected_warning_count']}")
    print(f"next_check_required_cases={metrics['next_check_required_cases']}")
    print(f"next_check_pass_rate={next_check_pass_rate_text}")
    print(f"next_check_presence_rate={metrics['next_check_presence_rate']:.3f}")
    print(f"failed_case_count={len(metrics['failed_cases'])}")

    if failures:
        for f in failures:
            print(f"FAIL: {f}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
