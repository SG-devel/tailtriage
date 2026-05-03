#!/usr/bin/env python3
import argparse, json, sys
from collections import Counter, defaultdict
from pathlib import Path

ALLOWED_GT = {
    "application_queue_saturation",
    "blocking_pool_pressure",
    "executor_pressure_suspected",
    "downstream_stage_dominates",
    "insufficient_evidence",
}


def fail(msg):
    raise ValueError(msg)


def load_json(path):
    with open(path, "r", encoding="utf-8") as f:
        return json.load(f)


def validate_manifest(manifest):
    if not isinstance(manifest, list) or not manifest:
        fail("manifest must be a non-empty list")
    seen = set()
    for i, c in enumerate(manifest):
        for k in ["id", "artifact", "artifact_type", "ground_truth", "acceptable_top2", "tags", "must_include_evidence", "allowed_warnings", "notes"]:
            if k not in c:
                fail(f"case[{i}] missing required field: {k}")
        cid = c["id"]
        if cid in seen:
            fail(f"duplicate case id: {cid}")
        seen.add(cid)
        if c["artifact_type"] != "analysis_report":
            fail(f"case {cid} has unsupported artifact_type: {c['artifact_type']}")
        gt = c["ground_truth"]
        if gt not in ALLOWED_GT:
            fail(f"case {cid} has unknown ground_truth: {gt}")
        top2 = c["acceptable_top2"]
        if gt not in top2:
            fail(f"case {cid} acceptable_top2 must include ground_truth")


def extract(report):
    primary = report.get("primary_suspect", {})
    secondaries = report.get("secondary_suspects", [])
    return {
        "primary_kind": primary.get("kind"),
        "primary_confidence": primary.get("confidence", "unknown"),
        "primary_score": primary.get("score", 0),
        "secondary_kinds": [s.get("kind") for s in secondaries if isinstance(s, dict)],
        "all_evidence": [*(primary.get("evidence") or []), *[e for s in secondaries if isinstance(s, dict) for e in (s.get("evidence") or [])]],
        "warnings": report.get("warnings", []),
    }


def bucket(conf):
    return conf if conf in {"high", "medium", "low"} else "unknown"


def run(manifest_path, min_top1, min_top2):
    manifest = load_json(manifest_path)
    validate_manifest(manifest)
    root = Path(manifest_path).parent

    total = len(manifest)
    top1_hits = top2_hits = evidence_pass = unexpected_warn_count = hc_wrong = 0
    gt_counts = Counter()
    confusion = defaultdict(Counter)
    bucket_stats = defaultdict(lambda: {"total": 0, "correct": 0})
    failed = []

    for c in manifest:
        gt = c["ground_truth"]
        gt_counts[gt] += 1
        report = load_json(root / c["artifact"])
        x = extract(report)
        predicted = x["primary_kind"]
        top2 = [predicted] + x["secondary_kinds"][:1]
        t1 = predicted == gt
        t2 = gt in c["acceptable_top2"] and any(k in c["acceptable_top2"] for k in top2)
        top1_hits += int(t1)
        top2_hits += int(t2)
        confusion[gt][predicted] += 1

        b = bucket(x["primary_confidence"])
        bucket_stats[b]["total"] += 1
        bucket_stats[b]["correct"] += int(t1)

        if b == "high" and not t1:
            hc_wrong += 1

        ev_blob = "\n".join(str(e) for e in x["all_evidence"])
        missing_ev = [req for req in c["must_include_evidence"] if req not in ev_blob]
        ev_ok = not missing_ev
        evidence_pass += int(ev_ok)

        unexpected = []
        for w in x["warnings"]:
            if not any(allow in w for allow in c["allowed_warnings"]):
                unexpected.append(w)
        unexpected_warn_count += len(unexpected)

        if (not t2) or (not ev_ok) or unexpected:
            failed.append({
                "id": c["id"], "ground_truth": gt, "predicted": predicted,
                "top2": top2, "missing_required_evidence": missing_ev,
                "unexpected_warnings": unexpected,
            })

    metrics = {
        "total_cases": total,
        "top1_accuracy": top1_hits / total,
        "top2_recall": top2_hits / total,
        "high_confidence_wrong_count": hc_wrong,
        "per_ground_truth_counts": dict(gt_counts),
        "confusion_matrix": {k: dict(v) for k, v in confusion.items()},
        "confidence_bucket_accuracy": {k: {"total": v["total"], "accuracy": (v["correct"]/v["total"] if v["total"] else 0.0)} for k, v in bucket_stats.items()},
        "required_evidence_pass_rate": evidence_pass / total,
        "unexpected_warning_count": unexpected_warn_count,
        "failed_cases": failed,
        "thresholds": {"min_top1": min_top1, "min_top2": min_top2},
    }
    return metrics


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", required=True)
    ap.add_argument("--output")
    ap.add_argument("--min-top1", type=float, default=0.75)
    ap.add_argument("--min-top2", type=float, default=0.90)
    args = ap.parse_args()

    try:
        m = run(args.manifest, args.min_top1, args.min_top2)
    except Exception as e:
        print(f"ERROR: {e}", file=sys.stderr)
        return 2

    print(f"cases={m['total_cases']} top1={m['top1_accuracy']:.3f} top2={m['top2_recall']:.3f}")
    print(f"high_confidence_wrong={m['high_confidence_wrong_count']} required_evidence_pass_rate={m['required_evidence_pass_rate']:.3f}")
    print(f"unexpected_warning_count={m['unexpected_warning_count']} failed_cases={len(m['failed_cases'])}")

    if args.output:
        Path(args.output).parent.mkdir(parents=True, exist_ok=True)
        with open(args.output, "w", encoding="utf-8") as f:
            json.dump(m, f, indent=2, sort_keys=True)

    if m["required_evidence_pass_rate"] < 1.0 or m["unexpected_warning_count"] > 0 or m["top1_accuracy"] < args.min_top1 or m["top2_recall"] < args.min_top2 or m["failed_cases"]:
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
