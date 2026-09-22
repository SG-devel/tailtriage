#!/usr/bin/env python3
"""Record, verify, and compare real-CLI analyzer architecture evidence.

This standard-library-only runner deliberately records public behavior.  It does
not reproduce analyzer calculations or decide whether a diagnosis is correct.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path
from typing import Any


REPO = Path(__file__).resolve().parents[1]
TARGET = (REPO / "target").resolve()
MANIFEST = Path("validation/diagnostics/manifest.json")
SCHEMA_VERSION = 1
RUNNER_VERSION = "1"
FIXTURES = (
    "queue_saturation.json",
    "blocking_pressure.json",
    "executor_pressure.json",
    "downstream_stage.json",
    "insufficient_evidence.json",
    "mixed_queue_vs_blocking.json",
    "mixed_blocking_vs_downstream.json",
    "scoped_route.json",
    "scoped_temporal.json",
)
PROJECTION_KEYS = (
    "request_count",
    "p50_latency_us",
    "p95_latency_us",
    "p99_latency_us",
    "p95_queue_share_permille",
    "p95_service_share_permille",
    "inflight_trend",
    "warnings",
    "evidence_quality",
    "primary_suspect",
    "secondary_suspects",
    "route_breakdowns",
    "temporal_segments",
)


class EvidenceError(RuntimeError):
    """A deterministic evidence or compatibility check failed."""


def canonical_bytes(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def file_sha256(path: Path) -> str:
    return sha256(path.read_bytes())


def write_json(path: Path, value: Any) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(canonical_bytes(value))


def read_json(path: Path) -> Any:
    return json.loads(path.read_bytes())


def confined_output(value: str | Path, *, must_not_exist: bool = False) -> Path:
    raw = Path(value)
    if ".." in raw.parts:
        raise EvidenceError("output path must not contain '..'")
    path = (REPO / raw).resolve() if not raw.is_absolute() else raw.resolve()
    if path == TARGET or TARGET not in path.parents:
        raise EvidenceError(f"output must be an owned child directory below {TARGET}")
    # Existing parents are resolved above, so symlink escapes are rejected too.
    if must_not_exist and path.exists():
        raise EvidenceError(f"output already exists: {path}")
    return path


def repository_path(path: Path) -> str:
    resolved = path.resolve()
    try:
        return resolved.relative_to(REPO).as_posix()
    except ValueError:
        return str(resolved)


def load_manifest(path: Path = REPO / MANIFEST) -> dict[str, Any]:
    value = read_json(path)
    if value.get("schema_version") != 2 or not isinstance(value.get("cases"), list):
        raise EvidenceError("diagnostic manifest must use schema version 2")
    return value


def build_plan(manifest_path: Path = REPO / MANIFEST) -> dict[str, Any]:
    cases: list[dict[str, Any]] = []
    for name in FIXTURES:
        rel = Path("tailtriage-analyzer/tests/fixtures") / name
        cases.append({
            "id": f"analyzer-fixture:{name[:-5]}",
            "source_class": "analyzer_fixture",
            "family": name[:-5],
            "source_path": rel.as_posix(),
            "artifact_type": "run_artifact",
            "stages": ["analyze"],
            "artifact_policy": "strict",
            "analyzer_overrides": [],
            "source_sha256": file_sha256(REPO / rel),
        })
    manifest = load_manifest(manifest_path)
    base = manifest_path.parent
    for item in manifest["cases"]:
        if item.get("validation_class") != "analyzer_execution":
            continue
        typ = item["artifact_type"]
        if typ not in {"run_artifact", "tracing_span_jsonl"}:
            raise EvidenceError(f"unsupported executable artifact type: {typ}")
        source = (base / item["artifact"]).resolve()
        rel = source.relative_to(REPO).as_posix()
        cases.append({
            "id": f"diagnostic:{item['id']}",
            "source_class": "diagnostic_manifest",
            "family": item["id"],
            "source_path": rel,
            "artifact_type": typ,
            "stages": ["import", "analyze"] if typ == "tracing_span_jsonl" else ["analyze"],
            "artifact_policy": item.get("artifact_policy", "strict"),
            "analyzer_overrides": [],
            "source_sha256": file_sha256(source),
        })
    return {
        "schema_version": SCHEMA_VERSION,
        "runner_version": RUNNER_VERSION,
        "global_analyzer_config": {"config": None, "overrides": []},
        "cases": cases,
    }


def command_description(stage: str, case: dict[str, Any]) -> list[str]:
    if stage == "import":
        return ["$BINARY", "import", "tracing-spans-jsonl", "$INPUT", "--service", "validation-tracing", "--output", "$IMPORTED_RUN"]
    command = ["$BINARY", "analyze", "$IMPORTED_RUN" if case["artifact_type"] == "tracing_span_jsonl" else "$INPUT", "--format", "json"]
    if case["artifact_policy"] == "allow_ambiguous":
        command.append("--allow-ambiguous-artifact")
    for override in case["analyzer_overrides"]:
        command += ["--analyzer-set", override]
    return command


def actual_command(description: list[str], binary: Path, source: Path, imported: Path) -> list[str]:
    replacements = {"$BINARY": str(binary), "$INPUT": str(source), "$IMPORTED_RUN": str(imported)}
    return [replacements.get(part, part) for part in description]


def project_report(raw: bytes) -> dict[str, Any]:
    report = json.loads(raw)
    if not isinstance(report, dict):
        raise EvidenceError("successful analyze output is not a JSON object")
    return {key: report[key] for key in PROJECTION_KEYS if key in report}


def stage_identity(result: dict[str, Any]) -> dict[str, Any]:
    return {key: result[key] for key in ("stage", "command", "exit_code", "stdout_sha256", "stderr_sha256")}


def aggregate_payload(plan_hash: str, binary_hash: str, config: dict[str, Any], results: list[dict[str, Any]]) -> dict[str, Any]:
    return {
        "schema_version": SCHEMA_VERSION,
        "plan_sha256": plan_hash,
        "binary_sha256": binary_hash,
        "global_analyzer_config": config,
        "cases": [{
            "id": row["id"],
            "source_sha256": row["source_sha256"],
            "imported_run_sha256": row.get("imported_run_sha256"),
            "stages": [stage_identity(stage) for stage in row["stages"]],
            "projection_sha256": row["projection_sha256"],
        } for row in results],
    }


def git_value(*args: str) -> str:
    result = subprocess.run(["git", *args], cwd=REPO, capture_output=True, check=True)
    return result.stdout.decode("utf-8", "surrogateescape")


def prepare_binary(supplied: str | None) -> tuple[Path, str]:
    if supplied:
        binary = Path(supplied).expanduser().resolve()
        if not binary.is_file():
            raise EvidenceError(f"supplied binary does not exist: {binary}")
        return binary, "supplied"
    subprocess.run(["cargo", "build", "-p", "tailtriage-cli", "--locked"], cwd=REPO, check=True)
    binary = (REPO / "target/debug/tailtriage").resolve()
    if not binary.is_file():
        raise EvidenceError("cargo build did not produce target/debug/tailtriage")
    return binary, "built"


def safe_case_id(case_id: str) -> str:
    return case_id.replace(":", "__").replace("/", "_")


def record_stage(case_dir: Path, stage: str, description: list[str], argv: list[str]) -> dict[str, Any]:
    result = subprocess.run(argv, cwd=REPO, capture_output=True, check=False)
    stdout_path = case_dir / f"{stage}.stdout"
    stderr_path = case_dir / f"{stage}.stderr"
    stdout_path.write_bytes(result.stdout)
    stderr_path.write_bytes(result.stderr)
    recorded = {
        "stage": stage,
        "command": description,
        "exit_code": result.returncode,
        "stdout_path": stdout_path.relative_to(case_dir.parents[1]).as_posix(),
        "stderr_path": stderr_path.relative_to(case_dir.parents[1]).as_posix(),
        "stdout_sha256": sha256(result.stdout),
        "stderr_sha256": sha256(result.stderr),
    }
    write_json(case_dir / f"{stage}.process.json", recorded)
    return recorded


def record(output_value: str | Path, supplied_binary: str | None = None) -> dict[str, Any]:
    output = confined_output(output_value, must_not_exist=True)
    binary, mode = prepare_binary(supplied_binary)
    binary_hash = file_sha256(binary)
    plan = build_plan()
    plan_bytes = canonical_bytes(plan)
    plan_hash = sha256(plan_bytes)
    output.mkdir(parents=True)
    (output / "plan.json").write_bytes(plan_bytes)
    (output / "inputs").mkdir()
    (output / "cases").mkdir()
    results = []
    inventory = []
    for case in plan["cases"]:
        source = REPO / case["source_path"]
        local_input = output / "inputs" / (safe_case_id(case["id"]) + ".source")
        shutil.copyfile(source, local_input)
        inventory.append({"id": case["id"], "source_path": case["source_path"], "source_sha256": case["source_sha256"], "evidence_path": local_input.relative_to(output).as_posix()})
        case_dir = output / "cases" / safe_case_id(case["id"])
        case_dir.mkdir()
        imported = case_dir / "imported-run.json"
        stages = []
        projection: dict[str, Any]
        for stage in case["stages"]:
            description = command_description(stage, case)
            recorded = record_stage(case_dir, stage, description, actual_command(description, binary, local_input, imported))
            stages.append(recorded)
            if recorded["exit_code"] != 0:
                break
        imported_hash = file_sha256(imported) if imported.exists() else None
        analyze = next((item for item in stages if item["stage"] == "analyze"), None)
        if analyze and analyze["exit_code"] == 0:
            try:
                projection = {"status": "report", "report": project_report((output / analyze["stdout_path"]).read_bytes())}
            except (json.JSONDecodeError, EvidenceError) as error:
                projection = {"status": "invalid_report", "error": str(error), "process": stage_identity(analyze)}
        else:
            projection = {"status": "process_result", "stages": [stage_identity(item) for item in stages]}
        projection_bytes = canonical_bytes(projection)
        (case_dir / "projection.json").write_bytes(projection_bytes)
        row = {"id": case["id"], "source_sha256": case["source_sha256"], "imported_run_sha256": imported_hash, "stages": stages, "projection_path": (case_dir / "projection.json").relative_to(output).as_posix(), "projection_sha256": sha256(projection_bytes)}
        write_json(case_dir / "result.json", row)
        results.append(row)
    write_json(output / "input-inventory.json", inventory)
    aggregate = aggregate_payload(plan_hash, binary_hash, plan["global_analyzer_config"], results)
    aggregate_hash = sha256(canonical_bytes(aggregate))
    write_json(output / "aggregate.json", aggregate)
    (output / "aggregate-sha256.txt").write_text(aggregate_hash + "\n", encoding="ascii")
    manifest_path = REPO / MANIFEST
    provenance = {
        "schema_version": SCHEMA_VERSION,
        "runner_version": RUNNER_VERSION,
        "git_head": git_value("rev-parse", "HEAD").strip(),
        "git_status_at_start": git_value("status", "--short").splitlines(),
        "source_reproducible": not bool(git_value("status", "--short")),
        "binary": {"mode": mode, "path": repository_path(binary), "sha256": binary_hash},
        "plan_sha256": plan_hash,
        "global_analyzer_config": plan["global_analyzer_config"],
        "source_manifest": {"path": MANIFEST.as_posix(), "sha256": file_sha256(manifest_path)},
        "input_inventory": inventory,
        "numeric108": None,
    }
    write_json(output / "provenance.json", provenance)
    print(f"recorded {len(results)} cases; aggregate={aggregate_hash}")
    return provenance


def validate_saved(output: Path) -> tuple[dict[str, Any], list[dict[str, Any]]]:
    required = {"plan.json", "provenance.json", "input-inventory.json", "aggregate.json", "aggregate-sha256.txt", "inputs", "cases"}
    present = {path.name for path in output.iterdir()}
    missing = sorted(required - present)
    extra_inputs: list[str] = []
    if missing:
        raise EvidenceError(f"missing evidence entries: {missing}")
    plan_bytes = (output / "plan.json").read_bytes()
    plan = read_json(output / "plan.json")
    if canonical_bytes(plan) != plan_bytes:
        raise EvidenceError("plan bytes are not canonical")
    current_plan = canonical_bytes(build_plan())
    if current_plan != plan_bytes:
        raise EvidenceError("recorded plan or current source inputs changed")
    provenance = read_json(output / "provenance.json")
    if provenance["plan_sha256"] != sha256(plan_bytes):
        raise EvidenceError("recorded plan hash changed")
    manifest = provenance["source_manifest"]
    if manifest["path"] != MANIFEST.as_posix() or file_sha256(REPO / manifest["path"]) != manifest["sha256"]:
        raise EvidenceError("diagnostic source manifest bytes/hash changed")
    inventory = read_json(output / "input-inventory.json")
    expected_files = {item["evidence_path"] for item in inventory}
    actual_files = {path.relative_to(output).as_posix() for path in (output / "inputs").iterdir() if path.is_file()}
    extra_inputs = sorted(actual_files - expected_files)
    if expected_files != actual_files:
        raise EvidenceError(f"missing/extra input evidence: missing={sorted(expected_files-actual_files)}, extra={extra_inputs}")
    if [item["id"] for item in inventory] != [case["id"] for case in plan["cases"]]:
        raise EvidenceError("input inventory order or case identity changed")
    for item in inventory:
        local_hash = file_sha256(output / item["evidence_path"])
        source_hash = file_sha256(REPO / item["source_path"])
        if local_hash != item["source_sha256"] or source_hash != item["source_sha256"]:
            raise EvidenceError(f"input bytes/hash changed for {item['id']}")
    binary_path = Path(provenance["binary"]["path"])
    if not binary_path.is_absolute():
        binary_path = REPO / binary_path
    if file_sha256(binary_path) != provenance["binary"]["sha256"]:
        raise EvidenceError("binary bytes/hash changed")
    results = []
    expected_case_dirs = {safe_case_id(case["id"]) for case in plan["cases"]}
    actual_case_dirs = {path.name for path in (output / "cases").iterdir() if path.is_dir()}
    if expected_case_dirs != actual_case_dirs:
        raise EvidenceError("missing/extra case evidence")
    for case in plan["cases"]:
        case_dir = output / "cases" / safe_case_id(case["id"])
        row = read_json(case_dir / "result.json")
        for stage in row["stages"]:
            if stage["command"] != command_description(stage["stage"], case):
                raise EvidenceError(f"command definition changed for {case['id']}")
            if file_sha256(output / stage["stdout_path"]) != stage["stdout_sha256"] or file_sha256(output / stage["stderr_path"]) != stage["stderr_sha256"]:
                raise EvidenceError(f"saved raw process bytes/hash changed for {case['id']}")
            if read_json(case_dir / f"{stage['stage']}.process.json") != stage:
                raise EvidenceError(f"saved process metadata changed for {case['id']}")
        projection_bytes = (output / row["projection_path"]).read_bytes()
        if canonical_bytes(json.loads(projection_bytes)) != projection_bytes or sha256(projection_bytes) != row["projection_sha256"]:
            raise EvidenceError(f"saved projection changed for {case['id']}")
        if row.get("imported_run_sha256") is not None and file_sha256(case_dir / "imported-run.json") != row["imported_run_sha256"]:
            raise EvidenceError(f"imported Run changed for {case['id']}")
        results.append(row)
    aggregate = aggregate_payload(provenance["plan_sha256"], provenance["binary"]["sha256"], plan["global_analyzer_config"], results)
    if canonical_bytes(aggregate) != (output / "aggregate.json").read_bytes():
        raise EvidenceError("aggregate payload changed")
    recorded_hash = (output / "aggregate-sha256.txt").read_text(encoding="ascii").strip()
    if sha256(canonical_bytes(aggregate)) != recorded_hash:
        raise EvidenceError("aggregate hash changed")
    return provenance, results


def verify(output_value: str | Path) -> None:
    output = confined_output(output_value)
    provenance, baseline = validate_saved(output)
    binary_path = Path(provenance["binary"]["path"])
    if not binary_path.is_absolute():
        binary_path = REPO / binary_path
    with tempfile.TemporaryDirectory(prefix="architecture-verify-", dir=TARGET) as td:
        replay = Path(td) / "replay"
        record(replay, str(binary_path))
        _, reproduced = validate_saved(replay)
        baseline_compare = [{k: row[k] for k in ("id", "source_sha256", "imported_run_sha256", "stages", "projection_sha256")} for row in baseline]
        reproduced_compare = [{k: row[k] for k in ("id", "source_sha256", "imported_run_sha256", "stages", "projection_sha256")} for row in reproduced]
        if baseline_compare != reproduced_compare:
            raise EvidenceError("reproduced process outcome differs from recorded evidence")
    success = {"status": "verified", "aggregate_sha256": (output / "aggregate-sha256.txt").read_text().strip()}
    write_json(output / "verification-success.json", success)
    print(f"verified {len(baseline)} cases; aggregate={success['aggregate_sha256']}")


def field_differences(left: Any, right: Any, prefix: str = "") -> list[str]:
    if type(left) is not type(right):
        return [prefix or "$type"]
    if isinstance(left, dict):
        keys = sorted(set(left) | set(right))
        return [item for key in keys for item in field_differences(left.get(key), right.get(key), f"{prefix}.{key}" if prefix else key)]
    if isinstance(left, list):
        if len(left) != len(right):
            return [f"{prefix}.length"]
        return [item for index, (a, b) in enumerate(zip(left, right)) for item in field_differences(a, b, f"{prefix}[{index}]")]
    return [] if left == right else [prefix]


def compare(left_value: str | Path, right_value: str | Path, output_value: str | Path) -> dict[str, Any]:
    left = confined_output(left_value)
    right = confined_output(right_value)
    output = confined_output(output_value, must_not_exist=True)
    lp, lr = validate_saved(left)
    rp, rr = validate_saved(right)
    lplan, rplan = read_json(left / "plan.json"), read_json(right / "plan.json")
    identity = [(c["id"], c["source_path"], c["source_sha256"], c["stages"], c["artifact_policy"]) for c in lplan["cases"]]
    if identity != [(c["id"], c["source_path"], c["source_sha256"], c["stages"], c["artifact_policy"]) for c in rplan["cases"]]:
        raise EvidenceError("incompatible plan/case/input identity")
    rows = []
    changed = []
    normalized_changed = []
    for lrow, rrow in zip(lr, rr):
        lprojection = read_json(left / lrow["projection_path"])
        rprojection = read_json(right / rrow["projection_path"])
        raw_changed = [(a["stage"], a["stdout_sha256"] != b["stdout_sha256"], a["stderr_sha256"] != b["stderr_sha256"], a["exit_code"] != b["exit_code"]) for a, b in zip(lrow["stages"], rrow["stages"])] if len(lrow["stages"]) == len(rrow["stages"]) else [["stage_sequence", True, True, True]]
        projection_delta = field_differences(lprojection, rprojection)
        is_changed = bool(projection_delta or any(any(flags[1:]) for flags in raw_changed) or lrow.get("imported_run_sha256") != rrow.get("imported_run_sha256"))
        if is_changed:
            changed.append(lrow["id"])
        if projection_delta:
            normalized_changed.append(lrow["id"])
        rows.append({"id": lrow["id"], "changed": is_changed, "raw_stage_changes": raw_changed, "normalized_projection_changed": bool(projection_delta), "normalized_field_deltas": projection_delta})
    report = {
        "schema_version": SCHEMA_VERSION,
        "compatible_plan_input_identity": True,
        "left": {"binary": lp["binary"], "global_analyzer_config": lp["global_analyzer_config"]},
        "right": {"binary": rp["binary"], "global_analyzer_config": rp["global_analyzer_config"]},
        "case_count": len(rows),
        "unchanged_case_count": len(rows) - len(changed),
        "changed_case_count": len(changed),
        "changed_case_ids": changed,
        "normalized_projection_change_count": len(normalized_changed),
        "normalized_projection_changed_case_ids": normalized_changed,
        "cases": rows,
    }
    output.mkdir(parents=True)
    write_json(output / "comparison.json", report)
    print(f"compatible plan/input identity; changed cases={len(changed)}; normalized projection changes={len(normalized_changed)}")
    return report


def numeric108(output_value: str | Path, numeric_dir_value: str | Path, run_first: bool) -> None:
    output = confined_output(output_value, must_not_exist=True)
    numeric_dir = confined_output(numeric_dir_value)
    script = REPO / "scripts/analyzer_numeric_sensitivity.py"
    commands = []
    if run_first:
        commands.append([sys.executable, str(script), "--output", str(numeric_dir), "run"])
    commands.append([sys.executable, str(script), "--output", str(numeric_dir), "verify"])
    records = []
    for command in commands:
        result = subprocess.run(command, cwd=REPO, capture_output=True, check=False)
        records.append({"command": ["$PYTHON", "scripts/analyzer_numeric_sensitivity.py", "--output", repository_path(numeric_dir), command[-1]], "exit_code": result.returncode, "stdout_sha256": sha256(result.stdout), "stderr_sha256": sha256(result.stderr)})
        if result.returncode:
            raise EvidenceError(f"numeric108 {command[-1]} failed")
    aggregate = (numeric_dir / "aggregate-sha256.txt").read_text(encoding="ascii").strip()
    output.mkdir(parents=True)
    report = {"schema_version": SCHEMA_VERSION, "owner": "scripts/analyzer_numeric_sensitivity.py", "numeric108_output": repository_path(numeric_dir), "verified_aggregate_sha256": aggregate, "verification": records}
    write_json(output / "numeric108-link.json", report)
    print(f"numeric108 verified aggregate={aggregate}")


def parser() -> argparse.ArgumentParser:
    result = argparse.ArgumentParser(description=__doc__)
    commands = result.add_subparsers(dest="command", required=True)
    run = commands.add_parser("run", help="record a new real-CLI evidence directory")
    run.add_argument("--output", required=True)
    run.add_argument("--binary", help="exact compatible binary; omit to cargo-build the production CLI")
    verify_parser = commands.add_parser("verify", help="verify saved evidence and reproduce it")
    verify_parser.add_argument("--output", required=True)
    comparison = commands.add_parser("compare", help="compare two compatible evidence directories")
    comparison.add_argument("--left", required=True)
    comparison.add_argument("--right", required=True)
    comparison.add_argument("--output", required=True)
    numeric = commands.add_parser("numeric108", help="invoke the independent numeric108 owner and record linkage")
    numeric.add_argument("--output", required=True)
    numeric.add_argument("--numeric108-dir", required=True)
    numeric.add_argument("--run", action="store_true", help="run numeric108 before its mandatory independent verify")
    return result


def main(argv: list[str] | None = None) -> int:
    args = parser().parse_args(argv)
    try:
        if args.command == "run":
            record(args.output, args.binary)
        elif args.command == "verify":
            verify(args.output)
        elif args.command == "compare":
            compare(args.left, args.right, args.output)
        else:
            numeric108(args.output, args.numeric108_dir, args.run)
        return 0
    except (EvidenceError, OSError, subprocess.CalledProcessError, KeyError, ValueError, json.JSONDecodeError) as error:
        print(f"error: {error}", file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
