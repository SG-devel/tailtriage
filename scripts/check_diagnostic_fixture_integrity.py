#!/usr/bin/env python3
"""Check exact bytes and compact shapes of analyzer-executed fixtures."""

import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path
import sys


DIAGNOSTICS_ROOT = Path(__file__).resolve().parents[1] / "validation" / "diagnostics"
LOCK_NAME = "analyzer-fixtures.lock.json"
LOCK_FORMAT = "tailtriage.analyzer-fixture-lock.v1"
ARTIFACT_TYPES = {"run_artifact", "tracing_span_jsonl"}
LOCK_ENTRY_FIELDS = {"case_id", "artifact_type", "artifact", "sha256", "byte_length", "shape"}


def read_json(path, description, failures):
    try:
        return json.loads(path.read_text(encoding="utf-8"))
    except (OSError, UnicodeDecodeError, json.JSONDecodeError) as error:
        failures.append(f"invalid {description}: {error}")
        return None


def manifest_inventory(root, failures):
    manifest = read_json(root / "manifest.json", "manifest", failures)
    if not isinstance(manifest, dict):
        return []
    if manifest.get("schema_version") != 2:
        failures.append("manifest schema_version must be 2")
    cases = manifest.get("cases")
    if not isinstance(cases, list):
        failures.append("manifest cases must be a list")
        return []

    inventory = []
    root_resolved = root.resolve()
    for number, case in enumerate(cases):
        if not isinstance(case, dict) or case.get("validation_class") != "analyzer_execution":
            continue
        case_id = case.get("id")
        artifact_type = case.get("artifact_type")
        artifact = case.get("artifact")
        accuracy_eligible = case.get("accuracy_eligible") is True
        observation_id = case.get("observation_id")
        label = case_id if isinstance(case_id, str) and case_id else f"case {number}"
        readable = True
        if not isinstance(case_id, str) or not case_id:
            failures.append(f"non-empty case ID required for analyzer {label}")
            readable = False
        if not isinstance(artifact_type, str) or artifact_type not in ARTIFACT_TYPES:
            failures.append(f"invalid artifact type for {label}")
            readable = False
        if not isinstance(artifact, str) or not artifact:
            failures.append(f"non-empty artifact path required for {label}")
            readable = False
        else:
            try:
                candidate = Path(artifact)
                if candidate.is_absolute():
                    failures.append(f"absolute artifact path rejected for {label}: {artifact}")
                    readable = False
                else:
                    resolved = (root / candidate).resolve()
            except (OSError, RuntimeError, ValueError):
                failures.append(f"invalid artifact path for {label}: {artifact}")
                readable = False
            else:
                if not candidate.is_absolute():
                    try:
                        resolved.relative_to(root_resolved)
                    except ValueError:
                        failures.append(f"artifact path escapes diagnostics root for {label}: {artifact}")
                        readable = False
                    else:
                        if not resolved.is_file():
                            failures.append(f"artifact is not a regular file for {label}: {artifact}")
                            readable = False
        if accuracy_eligible and (not isinstance(observation_id, str) or not observation_id):
            failures.append(f"non-empty observation_id required for accuracy-eligible analyzer {label}")
        inventory.append({"case_id": case_id, "artifact_type": artifact_type,
                          "artifact": artifact, "readable": readable,
                          "accuracy_eligible": accuracy_eligible,
                          "observation_id": observation_id})

    ids = Counter(item["case_id"] for item in inventory
                  if isinstance(item["case_id"], str) and item["case_id"])
    paths = Counter(item["artifact"] for item in inventory
                    if isinstance(item["artifact"], str) and item["artifact"])
    for value, count in ids.items():
        if count > 1:
            failures.append(f"duplicate manifest case ID: {value}")
    for value, count in paths.items():
        if count > 1:
            failures.append(f"duplicate manifest artifact path: {value}")
    if not inventory:
        failures.append("manifest has no analyzer-execution cases")
    return inventory


def artifact_text(root, item, failures):
    artifact = item["artifact"]
    path = root / artifact
    try:
        data = path.read_bytes()
    except OSError as error:
        failures.append(f"cannot read {artifact}: {error}")
        return None, None
    if not data:
        failures.append(f"empty artifact: {artifact}")
    if b"\r" in data:
        failures.append(f"CR byte found in {artifact}")
    if not data.endswith(b"\n"):
        failures.append(f"missing final LF in {artifact}")
    elif data.endswith(b"\n\n"):
        failures.append(f"blank line after final content in {artifact}")
    try:
        text = data.decode("utf-8")
    except UnicodeDecodeError:
        failures.append(f"invalid UTF-8 in {artifact}")
        text = None
    return data, text


def require_object(value, label):
    if not isinstance(value, dict):
        raise ValueError(f"{label} must be an object")
    return value


def run_shape(text):
    try:
        run = json.loads(text)
    except json.JSONDecodeError as error:
        raise ValueError(f"malformed Run JSON: {error}") from error
    require_object(run, "Run")
    names = ("requests", "stages", "queues", "inflight", "runtime_snapshots")
    for name in names:
        if not isinstance(run.get(name), list):
            raise ValueError(f"Run {name} must be a list")
    requests, stages, queues = run["requests"], run["stages"], run["queues"]
    outcomes = Counter()
    request_ids = []
    for request in requests:
        require_object(request, "request")
        outcome = request.get("outcome")
        if not isinstance(outcome, str):
            raise ValueError("request outcome must be a string")
        request_id = request.get("request_id")
        if not isinstance(request_id, str):
            raise ValueError("request ID must be a string")
        outcomes[outcome] += 1
        request_ids.append(request_id)
    for event in stages:
        require_object(event, "stage")
    for event in queues:
        require_object(event, "queue")
    truncation = run.get("truncation")
    if "truncation" in run and not isinstance(truncation, dict):
        raise ValueError("Run truncation must be an object when present")
    return {
        "request_count": len(requests), "stage_count": len(stages),
        "queue_count": len(queues), "inflight_snapshot_count": len(run["inflight"]),
        "runtime_snapshot_count": len(run["runtime_snapshots"]),
        "partial_stage_count": sum(event.get("completed") is False for event in stages),
        "partial_queue_count": sum(event.get("completed") is False for event in queues),
        "outcomes": dict(sorted(outcomes.items())),
        "requests_with_run_interval": sum("started_at_run_us" in event and "finished_at_run_us" in event for event in requests),
        "stages_with_run_interval": sum("started_at_run_us" in event and "finished_at_run_us" in event for event in stages),
        "queues_with_run_interval": sum("waited_from_run_us" in event and "waited_until_run_us" in event for event in queues),
        "first_request_id": request_ids[0] if request_ids else None,
        "last_request_id": request_ids[-1] if request_ids else None,
        "truncation": truncation,
    }


def tracing_shape(text):
    physical_lines = text[:-1].split("\n") if text.endswith("\n") else text.split("\n")
    if any(not line for line in physical_lines):
        raise ValueError("blank JSONL line")
    counts = Counter()
    request_ids = []
    for number, line in enumerate(physical_lines, 1):
        try:
            record = json.loads(line)
        except json.JSONDecodeError as error:
            raise ValueError(f"malformed tracing JSONL line {number}: {error}") from error
        require_object(record, f"tracing line {number}")
        if record.get("format") != "tailtriage.tracing-span.v1":
            raise ValueError(f"wrong tracing format marker on line {number}")
        span = require_object(record.get("span"), f"span on line {number}")
        fields = require_object(span.get("fields"), f"span.fields on line {number}")
        kind = fields.get("tt.kind")
        if kind is not None and not isinstance(kind, str):
            raise ValueError(f"span tt.kind must be a string on line {number}")
        category = kind if kind in {"request", "stage", "queue"} else "other"
        counts[category] += 1
        if category == "request":
            request_id = fields.get("tt.request_id")
            if not isinstance(request_id, str):
                raise ValueError(f"request span ID must be a string on line {number}")
            request_ids.append(request_id)
    return {
        "line_count": len(physical_lines), "request_span_count": counts["request"],
        "stage_span_count": counts["stage"], "queue_span_count": counts["queue"],
        "other_span_count": counts["other"],
        "first_request_id": request_ids[0] if request_ids else None,
        "last_request_id": request_ids[-1] if request_ids else None,
    }


def calculate_entries(root, inventory, failures):
    entries = []
    for item in inventory:
        if not item["readable"]:
            continue
        data, text = artifact_text(root, item, failures)
        if data is None or text is None:
            continue
        try:
            shape = run_shape(text) if item["artifact_type"] == "run_artifact" else tracing_shape(text)
        except ValueError as error:
            failures.append(f"invalid {item['artifact']} for {item['case_id']}: {error}")
            continue
        entries.append({key: item[key] for key in ("case_id", "artifact_type", "artifact")}
                       | {"sha256": hashlib.sha256(data).hexdigest(),
                          "byte_length": len(data), "shape": shape})
    accuracy_by_hash = {}
    for item, entry in ((item, entry) for item in inventory for entry in entries
                        if item["case_id"] == entry["case_id"] and item["accuracy_eligible"]
                        and isinstance(item["observation_id"], str)
                        and item["observation_id"]):
        accuracy_by_hash.setdefault(entry["sha256"], []).append(
            (item["case_id"], item["observation_id"]))
    for digest, members in accuracy_by_hash.items():
        if len({observation_id for _, observation_id in members}) > 1:
            rendered = ", ".join(f"{case_id}/{observation_id}"
                                 for case_id, observation_id in sorted(members))
            failures.append("identical analyzer artifact bytes are assigned to distinct "
                            f"accuracy observations: {digest}: {rendered}")
    return sorted(entries, key=lambda entry: entry["case_id"])


def compare_lock(lock, entries, failures):
    if not isinstance(lock, dict):
        failures.append("fixture lock must be an object")
        return
    missing = {"format", "fixtures"} - set(lock)
    unknown = set(lock) - {"format", "fixtures"}
    for field in sorted(missing):
        failures.append(f"missing lock field: {field}")
    for field in sorted(unknown):
        failures.append(f"unknown lock field: {field}")
    if "format" in lock:
        if not isinstance(lock["format"], str):
            failures.append("lock format must be a string")
        elif lock["format"] != LOCK_FORMAT:
            failures.append(f"unsupported lock format: {lock['format']}")
    fixtures = lock.get("fixtures")
    if not isinstance(fixtures, list):
        failures.append("lock fixtures must be a list")
        return
    valid_entries = []
    for number, entry in enumerate(fixtures):
        label = f"lock fixture {number}"
        if not isinstance(entry, dict):
            failures.append(f"{label} must be an object")
            continue
        for field in sorted(LOCK_ENTRY_FIELDS - set(entry)):
            failures.append(f"{label} missing field: {field}")
        for field in sorted(set(entry) - LOCK_ENTRY_FIELDS):
            failures.append(f"{label} unknown field: {field}")
        case_id = entry.get("case_id")
        artifact_type = entry.get("artifact_type")
        artifact = entry.get("artifact")
        sha256 = entry.get("sha256")
        byte_length = entry.get("byte_length")
        shape = entry.get("shape")
        if not isinstance(case_id, str) or not case_id:
            failures.append(f"{label} case_id must be a non-empty string")
        if not isinstance(artifact_type, str) or artifact_type not in ARTIFACT_TYPES:
            failures.append(f"{label} artifact_type must be run_artifact or tracing_span_jsonl")
        if not isinstance(artifact, str) or not artifact:
            failures.append(f"{label} artifact must be a non-empty string")
        if not isinstance(sha256, str):
            failures.append(f"{label} sha256 must be a string")
        elif len(sha256) != 64 or any(char not in "0123456789abcdef" for char in sha256):
            failures.append(f"{label} sha256 must be exactly 64 lowercase hexadecimal characters")
        if isinstance(byte_length, bool) or not isinstance(byte_length, int) or byte_length < 0:
            failures.append(f"{label} byte_length must be a non-negative integer")
        if not isinstance(shape, dict):
            failures.append(f"{label} shape must be an object")
        if set(entry) == LOCK_ENTRY_FIELDS and isinstance(case_id, str) and case_id:
            valid_entries.append(entry)
    case_ids = [entry["case_id"] for entry in valid_entries]
    for case_id, count in Counter(case_ids).items():
        if count > 1:
            failures.append(f"duplicate lock case ID: {case_id}")
    if case_ids != sorted(case_ids):
        failures.append("lock fixtures must be ordered by case ID")
    locked = {entry["case_id"]: entry for entry in valid_entries}
    current = {entry["case_id"]: entry for entry in entries}
    for case_id in current.keys() - locked.keys():
        failures.append(f"missing lock entry: {case_id}")
    for case_id in locked.keys() - current.keys():
        failures.append(f"unexpected lock entry: {case_id}")
    for case_id in current.keys() & locked.keys():
        expected, actual = locked[case_id], current[case_id]
        for field in ("artifact_type", "artifact", "sha256", "byte_length"):
            if expected.get(field) != actual[field]:
                label = {"artifact": "artifact path"}.get(field, field.replace("_", " "))
                failures.append(f"{label} mismatch for {case_id}")
        expected_shape = expected.get("shape")
        if not isinstance(expected_shape, dict):
            failures.append(f"shape mismatch for {case_id}")
        else:
            for field in sorted(set(expected_shape) | set(actual["shape"])):
                if expected_shape.get(field) != actual["shape"].get(field):
                    failures.append(f"shape mismatch for {case_id} at {field}")


def main(argv=None):
    parser = argparse.ArgumentParser()
    parser.add_argument("--refresh", action="store_true")
    args = parser.parse_args(argv)
    root = DIAGNOSTICS_ROOT
    failures = []
    inventory = manifest_inventory(root, failures)
    entries = calculate_entries(root, inventory, failures)
    lock_path = root / LOCK_NAME
    if args.refresh:
        if failures:
            for failure in sorted(set(failures)):
                print(failure, file=sys.stderr)
            return 1
        payload = {"format": LOCK_FORMAT, "fixtures": entries}
        lock_path.write_text(json.dumps(payload, indent=2, ensure_ascii=False) + "\n", encoding="utf-8")
        print("diagnostic analyzer fixture lock refreshed")
        return 0
    lock = read_json(lock_path, "fixture lock", failures)
    compare_lock(lock, entries, failures)
    if failures:
        for failure in sorted(set(failures)):
            print(failure, file=sys.stderr)
        return 1
    print("diagnostic analyzer fixtures match the integrity lock")
    return 0


if __name__ == "__main__":
    sys.exit(main())
