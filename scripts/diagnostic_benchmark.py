#!/usr/bin/env python3
"""Run the diagnostic corpus with separate execution, accuracy, and Report contracts."""

import argparse
import json
import subprocess
import tempfile
from collections import Counter, defaultdict
from pathlib import Path

KINDS = {
    "application_queue_saturation",
    "blocking_pool_pressure",
    "executor_pressure_suspected",
    "downstream_stage_dominates",
    "insufficient_evidence",
}
TYPES = {
    "run_artifact",
    "tracing_span_jsonl",
    "analysis_report",
    "synthetic_analysis_report",
}
TYPE_CLASS = {
    "run_artifact": "analyzer_execution",
    "tracing_span_jsonl": "analyzer_execution",
    "analysis_report": "report_contract",
    "synthetic_analysis_report": "report_contract",
}
COMMON = (
    "id",
    "artifact",
    "artifact_type",
    "validation_class",
    "accuracy_eligible",
    "tags",
    "notes",
    "expected_primary_kinds",
    "required_visible_suspects",
    "must_include_evidence",
    "must_include_next_checks",
    "expected_warnings",
    "allowed_warnings",
)
OLD = {"required_top2", "acceptable_primary", "top1_required"}
CONF_ORDER = {"low": 0, "medium": 1, "high": 2}
EVIDENCE_QUALITIES = {"strong", "partial", "weak"}
SIGNAL_FAMILIES = {
    "requests",
    "queues",
    "stages",
    "runtime_snapshots",
    "inflight_snapshots",
}
SIGNAL_STATUSES = {"present", "missing", "partial", "truncated"}
FAILURE_FIELDS = (
    "failure_stage",
    "expected_error_substrings",
    "forbidden_error_substrings",
    "stdout_expectation",
)


def load_json(path):
    with Path(path).open(encoding="utf-8") as f:
        return json.load(f)


def _strings(value, name, cid, nonempty=False):
    if (
        not isinstance(value, list)
        or any(not isinstance(x, str) or not x for x in value)
        or (nonempty and not value)
    ):
        raise ValueError(
            f"{name} must be a {'non-empty ' if nonempty else ''}list of non-empty strings for {cid}"
        )


def validate_manifest(manifest):
    if not isinstance(manifest, dict) or not isinstance(manifest.get("cases"), list):
        raise ValueError("manifest must be an object containing a cases list")
    if manifest.get("schema_version") != 2:
        raise ValueError("manifest schema_version must be 2")
    seen = set()
    labels = {}
    for c in manifest["cases"]:
        cid = c.get("id", "<unknown>")
        for key in COMMON:
            if key not in c:
                raise ValueError(f"case missing required field: {key}")
        stale = OLD & c.keys()
        if stale:
            raise ValueError(
                f"version-1 field is not allowed for {cid}: {sorted(stale)[0]}"
            )
        if not isinstance(cid, str) or not cid.strip():
            raise ValueError("case id must be a non-empty string")
        if cid in seen:
            raise ValueError(f"duplicate case id: {cid}")
        seen.add(cid)
        typ = c["artifact_type"]
        cls = c["validation_class"]
        if typ not in TYPES:
            raise ValueError(f"unknown artifact_type for {cid}: {typ}")
        if cls != TYPE_CLASS[typ]:
            raise ValueError(
                f"artifact_type {typ} requires validation_class {TYPE_CLASS[typ]} for {cid}"
            )
        if not isinstance(c["accuracy_eligible"], bool):
            raise ValueError(f"accuracy_eligible must be a bool for {cid}")
        for key in ("expected_primary_kinds", "required_visible_suspects"):
            _strings(c[key], key, cid, True)
            if any(x not in KINDS for x in c[key]):
                raise ValueError(f"{key} contains unknown diagnosis kind for {cid}")
        for key in (
            "tags",
            "must_include_evidence",
            "must_include_next_checks",
            "expected_warnings",
            "allowed_warnings",
        ):
            _strings(c[key], key, cid)
        if "*" in c["expected_warnings"] + c["allowed_warnings"]:
            raise ValueError(f"wildcard '*' is not allowed in warnings lists for {cid}")
        if "exact_primary_kind" in c:
            if (
                not isinstance(c["exact_primary_kind"], str)
                or c["exact_primary_kind"] not in KINDS
            ):
                raise ValueError(
                    f"exact_primary_kind must be an allowed diagnosis kind for {cid}"
                )
        if "max_primary_confidence" in c and (
            not isinstance(c["max_primary_confidence"], str)
            or c["max_primary_confidence"] not in CONF_ORDER
        ):
            raise ValueError(
                f"max_primary_confidence must be one of low/medium/high for {cid}"
            )
        if "expected_evidence_quality" in c and (
            not isinstance(c["expected_evidence_quality"], str)
            or c["expected_evidence_quality"] not in EVIDENCE_QUALITIES
        ):
            raise ValueError(
                f"expected_evidence_quality must be one of strong/partial/weak for {cid}"
            )
        if "expected_signal_statuses" in c:
            statuses = c["expected_signal_statuses"]
            if not isinstance(statuses, dict):
                raise ValueError(
                    f"expected_signal_statuses must be an object for {cid}"
                )
            if any(
                not isinstance(k, str) or k not in SIGNAL_FAMILIES for k in statuses
            ):
                raise ValueError(
                    f"expected_signal_statuses contains unknown signal family for {cid}"
                )
            if any(
                not isinstance(v, str) or v not in SIGNAL_STATUSES
                for v in statuses.values()
            ):
                raise ValueError(
                    f"expected_signal_statuses contains unknown signal status for {cid}"
                )
        for key in (
            "must_include_confidence_notes",
            "must_include_route_warning",
            "must_include_temporal_warning",
            "expected_top_level_warnings",
        ):
            if key in c:
                _strings(c[key], key, cid)
                if key != "must_include_confidence_notes" and "*" in c[key]:
                    raise ValueError(f"wildcard '*' is not allowed in {key} for {cid}")
        for key in ("expected_route_breakdowns", "expected_temporal_segments"):
            if key in c and (
                not isinstance(c[key], str) or c[key] not in {"empty", "non_empty"}
            ):
                raise ValueError(f"{key} must be one of empty/non_empty for {cid}")
        if (
            not isinstance(c["artifact"], str)
            or not c["artifact"]
            or not isinstance(c["notes"], str)
            or not c["notes"]
        ):
            raise ValueError(f"artifact and notes must be non-empty strings for {cid}")
        if cls == "report_contract":
            if c["accuracy_eligible"]:
                raise ValueError(
                    f"report_contract must be accuracy ineligible for {cid}"
                )
            for key in (
                "ground_truth",
                "observation_id",
                "execution_expectation",
                "artifact_policy",
                "command",
                "args",
                *FAILURE_FIELDS,
            ):
                if key in c:
                    raise ValueError(
                        f"{key} is not allowed on report_contract for {cid}"
                    )
        else:
            expectation = c.get("execution_expectation", "success")
            if expectation not in {"success", "failure"}:
                raise ValueError(f"unknown execution_expectation for {cid}")
            policy = c.get("artifact_policy", "strict")
            if policy not in {"strict", "allow_ambiguous"}:
                raise ValueError(f"unknown artifact_policy for {cid}")
            if "artifact_policy" in c and typ != "run_artifact":
                raise ValueError(
                    f"artifact_policy is allowed only on run_artifact for {cid}"
                )
            if "command" in c or "args" in c:
                raise ValueError(
                    f"arbitrary command arguments are not allowed for {cid}"
                )
            if expectation == "failure":
                if c["accuracy_eligible"]:
                    raise ValueError(
                        f"failure case must be accuracy ineligible for {cid}"
                    )
                for key in (
                    "failure_stage",
                    "expected_error_substrings",
                    "forbidden_error_substrings",
                    "stdout_expectation",
                ):
                    if key not in c:
                        raise ValueError(f"failure case missing {key} for {cid}")
                if c["failure_stage"] not in (
                    {"analyze"} if typ == "run_artifact" else {"import", "analyze"}
                ):
                    raise ValueError(f"invalid failure_stage for {cid}")
                _strings(
                    c["expected_error_substrings"], "expected_error_substrings", cid
                )
                _strings(
                    c["forbidden_error_substrings"], "forbidden_error_substrings", cid
                )
                if c["stdout_expectation"] not in {"empty", "non_empty", "ignore"}:
                    raise ValueError(f"invalid stdout_expectation for {cid}")
            else:
                for key in FAILURE_FIELDS:
                    if key in c:
                        raise ValueError(
                            f"{key} is allowed only on failure cases for {cid}"
                        )
            if c["accuracy_eligible"]:
                for key in ("observation_id", "ground_truth"):
                    if not isinstance(c.get(key), str) or not c[key].strip():
                        raise ValueError(
                            f"accuracy eligible case requires non-empty {key} for {cid}"
                        )
                if c["ground_truth"] not in KINDS:
                    raise ValueError(f"unknown ground_truth for {cid}")
                if c["ground_truth"] not in c["expected_primary_kinds"]:
                    raise ValueError(
                        f"ground_truth must be in expected_primary_kinds for {cid}"
                    )
                if c["ground_truth"] not in c["required_visible_suspects"]:
                    raise ValueError(
                        f"ground_truth must be in required_visible_suspects for {cid}"
                    )
                if (
                    "exact_primary_kind" in c
                    and c["exact_primary_kind"] != c["ground_truth"]
                ):
                    raise ValueError(
                        f"exact_primary_kind must equal ground_truth for {cid}"
                    )
                label = (
                    c["ground_truth"],
                    tuple(c["expected_primary_kinds"]),
                    tuple(c["required_visible_suspects"]),
                    c.get("exact_primary_kind"),
                )
                oid = c["observation_id"]
                if oid in labels and labels[oid] != label:
                    raise ValueError(f"observation labels disagree for {oid}")
                labels[oid] = label
            else:
                for key in ("ground_truth", "observation_id"):
                    if key in c:
                        raise ValueError(
                            f"{key} is allowed only on accuracy eligible cases for {cid}"
                        )


def _command(stage, case, input_path, output_path=None):
    base = ["cargo", "run", "--quiet", "-p", "tailtriage-cli", "--"]
    if stage == "import":
        return base + [
            "import",
            "tracing-spans-jsonl",
            str(input_path),
            "--service",
            "validation-tracing",
            "--output",
            str(output_path),
        ]
    cmd = base + ["analyze", str(input_path), "--format", "json"]
    if case.get("artifact_policy", "strict") == "allow_ambiguous":
        cmd.append("--allow-ambiguous-artifact")
    return cmd


def _invoke(command):
    return subprocess.run(command, capture_output=True, text=True)


def _execute(case, path):
    stages = []
    with tempfile.TemporaryDirectory(prefix=f"tailtriage-{case['id']}-") as td:
        analyze_path = path
        if case["artifact_type"] == "tracing_span_jsonl":
            analyze_path = Path(td) / "imported-run.json"
            r = _invoke(_command("import", case, path, analyze_path))
            stages.append(("import", r))
            if r.returncode != 0:
                return None, stages
            if not analyze_path.exists():
                r.returncode = 1
                r.stderr += "\nimport did not create run artifact"
                return None, stages
        r = _invoke(_command("analyze", case, analyze_path))
        stages.append(("analyze", r))
        if r.returncode:
            return None, stages
        try:
            return json.loads(r.stdout), stages
        except json.JSONDecodeError:
            return None, stages


def extract(report):
    if not isinstance(report, dict):
        raise ValueError("report must be a JSON object")
    if not isinstance(report.get("primary_suspect"), dict):
        raise ValueError("report.primary_suspect must be an object")
    if not isinstance(report.get("secondary_suspects"), list):
        raise ValueError("report.secondary_suspects must be a list")
    if not isinstance(report.get("warnings"), list) or any(
        not isinstance(x, str) for x in report["warnings"]
    ):
        raise ValueError("report.warnings must be a list of strings")

    def suspect(value, name, primary=False):
        if not isinstance(value, dict):
            raise ValueError(f"{name} must be an object")
        if primary and value.get("kind") not in KINDS:
            raise ValueError(f"{name}.kind must be an allowed diagnosis kind")
        if not primary and "kind" in value and value["kind"] not in KINDS:
            raise ValueError(
                f"{name}.kind must be an allowed diagnosis kind when present"
            )
        if primary and value.get("confidence") not in CONF_ORDER:
            raise ValueError(f"{name}.confidence must be one of low/medium/high")
        if "confidence" in value and value["confidence"] not in CONF_ORDER:
            raise ValueError(
                f"{name}.confidence must be one of low/medium/high when present"
            )
        if "score" in value and (
            isinstance(value["score"], bool)
            or not isinstance(value["score"], (int, float))
        ):
            raise ValueError(f"{name}.score must be numeric when present")
        if primary and (
            not isinstance(value.get("evidence"), list)
            or any(not isinstance(x, str) for x in value["evidence"])
        ):
            raise ValueError(f"{name}.evidence must be a list of strings")
        for field in ("evidence", "next_checks", "confidence_notes"):
            if field in value and (
                not isinstance(value[field], list)
                or any(not isinstance(x, str) for x in value[field])
            ):
                raise ValueError(
                    f"{name}.{field} must be a list of strings when present"
                )

    p = report["primary_suspect"]
    suspect(p, "report.primary_suspect", True)
    for s in report["secondary_suspects"]:
        suspect(s, "report.secondary_suspects entry")
    for field in ("route_breakdowns", "temporal_segments"):
        if field in report:
            items = report[field]
            if not isinstance(items, list):
                raise ValueError(f"report.{field} must be a list when present")
            for item in items:
                if not isinstance(item, dict):
                    raise ValueError(f"report.{field} entries must be objects")
                if "warnings" in item and (
                    not isinstance(item["warnings"], list)
                    or any(not isinstance(x, str) for x in item["warnings"])
                ):
                    raise ValueError(
                        f"report.{field} entry warnings must be a list of strings"
                    )
    if "evidence_quality" in report and not isinstance(
        report["evidence_quality"], dict
    ):
        raise ValueError("report.evidence_quality must be an object when present")
    kind = p["kind"]
    conf = p["confidence"]
    suspects = [p] + report["secondary_suspects"]

    def flatten(field):
        return [v for s in suspects for v in s.get(field, [])]

    return {
        "top1": kind,
        "top2": [s.get("kind") for s in suspects[:2] if s.get("kind")],
        "primary_confidence": conf,
        "evidence": flatten("evidence"),
        "next_checks": flatten("next_checks"),
        "confidence_notes": flatten("confidence_notes"),
        "warnings": report["warnings"],
        "evidence_quality": report.get("evidence_quality", {}),
        "route_breakdowns": report.get("route_breakdowns", []),
        "temporal_segments": report.get("temporal_segments", []),
    }


def _contains(required, actual):
    return all(any(r.lower() in a.lower() for a in actual) for r in required)


def _assert_case(case, ext):
    visible = ext["top2"]
    unexpected = [
        w
        for w in ext["warnings"]
        if not any(
            x.lower() in w.lower()
            for x in case["expected_warnings"] + case["allowed_warnings"]
        )
    ]
    missing = [
        x
        for x in case["expected_warnings"]
        if not any(x.lower() in w.lower() for w in ext["warnings"])
    ]
    checks = {
        "primary_ok": ext["top1"] in case["expected_primary_kinds"],
        "visible_suspects_ok": all(
            x in visible for x in case["required_visible_suspects"]
        ),
        "exact_primary_ok": case.get("exact_primary_kind", ext["top1"]) == ext["top1"],
        "evidence_ok": _contains(case["must_include_evidence"], ext["evidence"]),
        "next_check_ok": _contains(
            case["must_include_next_checks"], ext["next_checks"]
        ),
        "warnings_ok": not unexpected and not missing,
    }
    if "max_primary_confidence" in case:
        checks["confidence_ceiling_ok"] = (
            CONF_ORDER[ext["primary_confidence"]]
            <= CONF_ORDER[case["max_primary_confidence"]]
        )
    if "expected_evidence_quality" in case:
        checks["evidence_quality_ok"] = (
            ext["evidence_quality"].get("quality") == case["expected_evidence_quality"]
        )
    if "expected_signal_statuses" in case:
        checks["signal_status_ok"] = all(
            ext["evidence_quality"].get(k) == v
            for k, v in case["expected_signal_statuses"].items()
        )
    if "must_include_confidence_notes" in case:
        checks["confidence_notes_ok"] = _contains(
            case["must_include_confidence_notes"], ext["confidence_notes"]
        )
    if "expected_top_level_warnings" in case:
        checks["top_level_warnings_ok"] = _contains(
            case["expected_top_level_warnings"], ext["warnings"]
        )
    for prefix, field, shape_key, warning_key in (
        (
            "route",
            "route_breakdowns",
            "expected_route_breakdowns",
            "must_include_route_warning",
        ),
        (
            "temporal",
            "temporal_segments",
            "expected_temporal_segments",
            "must_include_temporal_warning",
        ),
    ):
        items = ext[field] if isinstance(ext[field], list) else []
        if shape_key in case:
            checks[prefix + "_shape_ok"] = bool(items) == (
                case[shape_key] == "non_empty"
            )
        if warning_key in case:
            nested = [
                w
                for x in items
                if isinstance(x, dict)
                for w in x.get("warnings", [])
                if isinstance(w, str)
            ]
            checks[prefix + "_warnings_ok"] = _contains(case[warning_key], nested)
    checks["unexpected_warnings"] = unexpected
    checks["missing_expected_warnings"] = missing
    return {
        "id": case["id"],
        "primary_kind": ext["top1"],
        "first_secondary_kind": ext["top2"][1] if len(ext["top2"]) > 1 else None,
        "primary_confidence": ext["primary_confidence"],
        **checks,
    }, all(v for k, v in checks.items() if k.endswith("_ok"))


def _failure_contract(case, stages):
    if not stages:
        return False, "command was not invoked"
    stage, res = stages[-1]
    if res.returncode == 0:
        return False, "expected execution failure succeeded"
    errors = []
    if stage != case["failure_stage"]:
        errors.append(f"failed at {stage}, expected {case['failure_stage']}")
    errors += [
        f"missing stderr diagnostic: {x}"
        for x in case["expected_error_substrings"]
        if x not in res.stderr
    ]
    errors += [
        f"forbidden stderr diagnostic: {x}"
        for x in case["forbidden_error_substrings"]
        if x in res.stderr
    ]
    want = case["stdout_expectation"]
    if want == "empty" and res.stdout:
        errors.append("stdout was not empty")
    if want == "non_empty" and not res.stdout:
        errors.append("stdout was empty")
    return not errors, "; ".join(errors)


def _expected_accuracy_members(cases):
    expected_members = defaultdict(list)
    for case in cases:
        if case["accuracy_eligible"]:
            expected_members[case["observation_id"]].append(case["id"])
    return expected_members


def _validate_report_contract_case(case, artifact_path):
    try:
        return _assert_case(case, extract(load_json(artifact_path)))
    except Exception as error:
        return {"id": case["id"], "error": str(error)}, False


def _validate_successful_execution(case, report):
    try:
        extracted_report = extract(report)
        row, passed = _assert_case(case, extracted_report)
        return row, passed, extracted_report
    except Exception as error:
        return {"id": case["id"], "error": str(error)}, False, None


def _group_equivalent_encodings(expected_members, usable_members, failed_cases):
    observations = []
    for observation_id, expected_case_ids in expected_members.items():
        group = usable_members[observation_id]
        usable_case_ids = [case["id"] for case, _ in group]
        unusable_case_ids = [
            case_id for case_id in expected_case_ids if case_id not in usable_case_ids
        ]
        if usable_case_ids != expected_case_ids:
            failed_cases.append(
                {
                    "observation_id": observation_id,
                    "expected_member_ids": expected_case_ids,
                    "usable_member_ids": usable_case_ids,
                    "unusable_member_ids": unusable_case_ids,
                    "error": "accuracy observation is incomplete because one or more declared encodings produced no usable Report",
                }
            )
            continue

        signatures = {
            (
                extracted_report["top1"],
                tuple(extracted_report["top2"]),
                extracted_report["primary_confidence"],
            )
            for _, extracted_report in group
        }
        if len(signatures) != 1:
            failed_cases.append(
                {
                    "observation_id": observation_id,
                    "member_case_ids": expected_case_ids,
                    "error": "equivalent encoding diagnosis or confidence disagreement",
                }
            )
            continue

        case, extracted_report = group[0]
        observations.append(
            {
                "observation_id": observation_id,
                "member_case_ids": expected_case_ids,
                "ground_truth": case["ground_truth"],
                "expected_primary_kinds": case["expected_primary_kinds"],
                "top1": extracted_report["top1"],
                "top2": extracted_report["top2"],
                "confidence": extracted_report["primary_confidence"],
            }
        )
    return observations


def _aggregate_accuracy(observations, usable_members):
    per_ground_truth = Counter()
    confusion_matrix = defaultdict(Counter)
    confidence_buckets = defaultdict(lambda: {"total": 0, "correct": 0})
    high_confidence_wrong_count = 0

    for observation in observations:
        ground_truth = observation["ground_truth"]
        top1_is_correct = observation["top1"] == ground_truth
        per_ground_truth[ground_truth] += 1
        confusion_matrix[ground_truth][observation["top1"]] += 1
        confidence_bucket = confidence_buckets[observation["confidence"]]
        confidence_bucket["total"] += 1
        confidence_bucket["correct"] += top1_is_correct
        if (
            observation["confidence"] == "high"
            and observation["top1"] not in observation["expected_primary_kinds"]
        ):
            high_confidence_wrong_count += 1

    observation_count = len(observations)
    if observation_count:
        top1_accuracy = (
            sum(
                observation["top1"] == observation["ground_truth"]
                for observation in observations
            )
            / observation_count
        )
        top2_recall = (
            sum(
                observation["ground_truth"] in observation["top2"]
                for observation in observations
            )
            / observation_count
        )
    else:
        top1_accuracy = None
        top2_recall = None

    return {
        "observation_count": observation_count,
        "encoding_count": sum(len(group) for group in usable_members.values()),
        "top1_accuracy": top1_accuracy,
        "top2_recall": top2_recall,
        "high_confidence_wrong_count": high_confidence_wrong_count,
        "per_ground_truth_counts": dict(per_ground_truth),
        "confusion_matrix": {
            ground_truth: dict(predictions)
            for ground_truth, predictions in confusion_matrix.items()
        },
        "confidence_bucket_accuracy": {
            confidence: {
                **bucket,
                "accuracy": bucket["correct"] / bucket["total"],
            }
            for confidence, bucket in confidence_buckets.items()
        },
        "observations": observations,
    }


def _evaluate_thresholds(
    analyzer_cases,
    observations,
    accuracy,
    failed_analyzer_cases,
    failed_report_contract_cases,
    min_top1,
    min_top2,
    max_high_confidence_wrong,
):
    failures = []
    if failed_analyzer_cases:
        failures.append("one or more analyzer-execution cases failed")
    if failed_report_contract_cases:
        failures.append("one or more report-contract cases failed")
    if not analyzer_cases:
        failures.append("diagnostic corpus contains zero analyzer-executed cases")
    if not observations:
        failures.append(
            "diagnostic corpus contains zero accuracy-eligible analyzer observations"
        )
        return failures

    top1_accuracy = accuracy["top1_accuracy"]
    top2_recall = accuracy["top2_recall"]
    high_confidence_wrong_count = accuracy["high_confidence_wrong_count"]
    if top1_accuracy < min_top1:
        failures.append(
            f"top1_accuracy {top1_accuracy:.3f} below threshold {min_top1:.3f}"
        )
    if top2_recall < min_top2:
        failures.append(f"top2_recall {top2_recall:.3f} below threshold {min_top2:.3f}")
    if high_confidence_wrong_count > max_high_confidence_wrong:
        failures.append(
            f"high_confidence_wrong_count {high_confidence_wrong_count} exceeds max {max_high_confidence_wrong}"
        )
    return failures


def run(manifest_path, min_top1=0.75, min_top2=0.90, max_high_confidence_wrong=0):
    manifest_file = Path(manifest_path).resolve()
    manifest = load_json(manifest_file)
    validate_manifest(manifest)

    analyzer_cases = []
    report_contract_cases = []
    failed_analyzer_cases = []
    failed_report_contract_cases = []
    usable_accuracy_members = defaultdict(list)
    validated_paths = Counter()
    execution_counts = Counter()
    expected_members = _expected_accuracy_members(manifest["cases"])

    for case in manifest["cases"]:
        validated_paths[case["artifact_type"]] += 1
        artifact_path = (manifest_file.parent / case["artifact"]).resolve()
        if case["validation_class"] == "report_contract":
            row, passed = _validate_report_contract_case(case, artifact_path)
            report_contract_cases.append(row)
            if not passed:
                failed_report_contract_cases.append(row)
            continue

        report, stages = _execute(case, artifact_path)
        if any(stage == "analyze" for stage, _ in stages):
            if case["artifact_type"] == "run_artifact":
                execution_counts["run_artifact"] += 1
            else:
                execution_counts["tracing_jsonl"] += 1

        if case.get("execution_expectation", "success") == "failure":
            passed, error = _failure_contract(case, stages)
            row = {
                "id": case["id"],
                "expected_failure": True,
                "passed": passed,
                "error": error,
            }
            analyzer_cases.append(row)
            if passed:
                execution_counts["expected_failure"] += 1
            else:
                execution_counts["unexpected_failure"] += 1
                failed_analyzer_cases.append(row)
            continue

        if report is None:
            execution_counts["unexpected_failure"] += 1
            row = {
                "id": case["id"],
                "error": "analyzer execution failed unexpectedly",
            }
            analyzer_cases.append(row)
            failed_analyzer_cases.append(row)
            continue

        execution_counts["success"] += 1
        row, passed, extracted_report = _validate_successful_execution(case, report)
        analyzer_cases.append(row)
        if not passed:
            failed_analyzer_cases.append(row)
        if case["accuracy_eligible"] and extracted_report is not None:
            usable_accuracy_members[case["observation_id"]].append(
                (case, extracted_report)
            )

    observations = _group_equivalent_encodings(
        expected_members,
        usable_accuracy_members,
        failed_analyzer_cases,
    )
    accuracy = _aggregate_accuracy(observations, usable_accuracy_members)
    metrics = {
        "schema_version": 2,
        "manifest_case_count": len(manifest["cases"]),
        "analyzer_execution": {
            "case_count": len(analyzer_cases),
            "success_count": execution_counts["success"],
            "expected_failure_count": execution_counts["expected_failure"],
            "unexpected_failure_count": execution_counts["unexpected_failure"],
            "run_artifact_count": execution_counts["run_artifact"],
            "tracing_jsonl_count": execution_counts["tracing_jsonl"],
            "cases": analyzer_cases,
        },
        "analyzer_accuracy": accuracy,
        "report_contract": {
            "case_count": len(report_contract_cases),
            "passed_count": len(report_contract_cases)
            - len(failed_report_contract_cases),
            "failed_count": len(failed_report_contract_cases),
            "analysis_report_count": validated_paths["analysis_report"],
            "synthetic_report_count": validated_paths["synthetic_analysis_report"],
            "cases": report_contract_cases,
        },
        "validated_paths": dict(validated_paths),
        "failed_analyzer_cases": failed_analyzer_cases,
        "failed_report_contract_cases": failed_report_contract_cases,
    }
    failures = _evaluate_thresholds(
        analyzer_cases,
        observations,
        accuracy,
        failed_analyzer_cases,
        failed_report_contract_cases,
        min_top1,
        min_top2,
        max_high_confidence_wrong,
    )
    return metrics, failures


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--manifest", required=True)
    p.add_argument("--output")
    p.add_argument("--min-top1", type=float, default=0.75)
    p.add_argument("--min-top2", type=float, default=0.90)
    p.add_argument("--max-high-confidence-wrong", type=int, default=0)
    a = p.parse_args()
    try:
        m, f = run(a.manifest, a.min_top1, a.min_top2, a.max_high_confidence_wrong)
    except Exception as e:
        print(f"ERROR: {e}")
        raise SystemExit(1)
    if a.output:
        Path(a.output).write_text(json.dumps(m, indent=2, sort_keys=True) + "\n")
    acc = m["analyzer_accuracy"]
    print(f"manifest_case_count={m['manifest_case_count']}")
    print(f"analyzer_execution_case_count={m['analyzer_execution']['case_count']}")
    print(f"accuracy_observation_count={acc['observation_count']}")
    print(
        "top1_accuracy="
        + ("n/a" if acc["top1_accuracy"] is None else f"{acc['top1_accuracy']:.3f}")
    )
    print(
        "top2_recall="
        + ("n/a" if acc["top2_recall"] is None else f"{acc['top2_recall']:.3f}")
    )
    print(f"high_confidence_wrong_count={acc['high_confidence_wrong_count']}")
    for x in f:
        print("FAIL:", x)
    if f:
        raise SystemExit(1)


if __name__ == "__main__":
    main()
