#!/usr/bin/env python3
"""Deterministically characterize analyzer behavior near selected numeric defaults.

This standard-library-only maintainer tool generates synthetic schema-v2 Runs and
asks the built tailtriage CLI for every observed report.  It deliberately does
not reproduce analyzer scoring formulas or assert expected scores.
"""

from __future__ import annotations

import argparse
import csv
import hashlib
import io
import json
import os
from pathlib import Path
import subprocess
import sys
from typing import Any, Iterable


REPO = Path(__file__).resolve().parents[1]
DEFAULT_OUTPUT = REPO / "target" / "analyzer-numeric-sensitivity"
RESULT_FIELDS = [
    "experiment_id", "family", "variant", "input_id", "input_sha256", "overrides",
    "request_count", "p95_latency_us", "p95_queue_share_permille", "evidence_quality",
    "primary_kind", "primary_score", "primary_confidence", "queue_present", "queue_score",
    "queue_confidence", "blocking_present", "blocking_score", "blocking_confidence",
    "executor_present", "executor_score", "executor_confidence", "downstream_present",
    "downstream_score", "downstream_confidence", "ambiguity_warning",
    "low_request_confidence_note", "route_breakdown_count", "temporal_segment_count",
    "report_sha256",
]
FAMILY_KIND = {
    "queue": "application_queue_pressure",
    "blocking": "blocking_pool_pressure",
    "executor": "executor_pressure",
    "downstream": "downstream_stage_dominance",
}


def canonical_json(value: Any) -> bytes:
    return (json.dumps(value, sort_keys=True, separators=(",", ":")) + "\n").encode()


def sha256(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def request(index: int, latency: int, route: str = "/a") -> dict[str, Any]:
    start = index * 2_000
    return {
        "request_id": f"request-{index:04d}", "route": route, "kind": None,
        "started_at_unix_ms": 1, "finished_at_unix_ms": 2, "latency_us": latency,
        "outcome": "ok", "started_at_run_us": start, "finished_at_run_us": start + latency,
    }


def base_run(input_id: str, latencies: Iterable[int]) -> dict[str, Any]:
    requests = [request(i, latency) for i, latency in enumerate(latencies)]
    return {
        "schema_version": 2,
        "metadata": {
            "run_id": input_id, "service_name": "numeric-sensitivity", "service_version": None,
            "started_at_unix_ms": 1, "finalized_at_unix_ms": 2, "mode": "investigation",
            "effective_core_config": None, "effective_tokio_sampler_config": None,
            "host": None, "pid": None, "lifecycle_warnings": [],
            "unfinished_requests": {"count": 0, "sample": []}, "run_end_reason": "shutdown",
        },
        "requests": requests, "stages": [], "queues": [], "inflight": [],
        "runtime_snapshots": [],
        "truncation": {
            "limits_hit": False, "dropped_requests": 0, "dropped_stages": 0,
            "dropped_queues": 0, "dropped_inflight_snapshots": 0,
            "dropped_runtime_snapshots": 0,
        },
    }


def add_queue(run: dict[str, Any], waits: Iterable[int], depth: int = 0) -> None:
    for i, wait in enumerate(waits):
        start = run["requests"][i]["started_at_run_us"]
        run["queues"].append({
            "request_id": f"request-{i:04d}", "queue": "work", "waited_from_unix_ms": 1,
            "waited_until_unix_ms": 2, "wait_us": wait, "depth_at_start": depth,
            "waited_from_run_us": start, "waited_until_run_us": start + wait,
        })


def queue_run(input_id: str, n: int, waits: Iterable[int], depth: int = 0,
              growth: bool = False, latencies: Iterable[int] | None = None) -> dict[str, Any]:
    waits = list(waits)
    run = base_run(input_id, latencies if latencies is not None else [1000] * n)
    add_queue(run, waits, depth)
    if growth:
        run["inflight"] = [
            {"gauge": "work", "at_unix_ms": 1, "at_run_us": 0, "count": 1},
            {"gauge": "work", "at_unix_ms": 2, "at_run_us": 1, "count": 2},
        ]
    return run


def runtime_run(input_id: str, blocking: list[int] | None = None,
                global_depth: int = 0, worker_count: int = 4, snapshots: int = 40) -> dict[str, Any]:
    run = base_run(input_id, [1000] * 40 if input_id.startswith("EXEC") else [1000] * 20)
    blocking = blocking if blocking is not None else [0] * snapshots
    run["runtime_snapshots"] = [
        {"at_unix_ms": 1, "at_run_us": i, "alive_tasks": 1, "worker_count": worker_count,
         "global_queue_depth": global_depth, "local_queue_depth": 0,
         "blocking_queue_depth": blocking[i], "remote_schedule_count": 0}
        for i in range(snapshots)
    ]
    return run


def downstream_run(input_id: str, k: int, stage_latency: int, total: int = 40) -> dict[str, Any]:
    positions: list[int] = []
    for offset in range((k + 1) // 2):
        positions.append(offset)
        if len(positions) < k:
            positions.append(total - 1 - offset)
    chosen = set(positions)
    run = base_run(input_id, [1000 if i in chosen else 1 for i in range(total)])
    for i in sorted(chosen):
        start = run["requests"][i]["started_at_run_us"]
        run["stages"].append({
            "request_id": f"request-{i:04d}", "stage": "db", "started_at_unix_ms": 1,
            "finished_at_unix_ms": 2, "latency_us": stage_latency, "success": True,
            "started_at_run_us": start, "finished_at_run_us": start + stage_latency,
        })
    return run


def temporal_run(input_id: str, n: int, early_latency: int, late_latency: int,
                 early_share: int, late_share: int) -> dict[str, Any]:
    split = n // 2
    latencies = [early_latency] * split + [late_latency] * (n - split)
    waits = [(latencies[i] * (early_share if i < split else late_share)) // 1000 for i in range(n)]
    return queue_run(input_id, n, waits, latencies=latencies)


def generate_plan() -> tuple[list[dict[str, Any]], dict[str, dict[str, Any]]]:
    experiments: list[dict[str, Any]] = []
    inputs: dict[str, dict[str, Any]] = {}

    def add(exp_id: str, family: str, variant: str, input_id: str,
            run: dict[str, Any], overrides: Iterable[str] = ()) -> None:
        inputs.setdefault(input_id, run)
        experiments.append({"experiment_id": exp_id, "family": family, "variant": variant,
                            "input_id": input_id, "overrides": list(overrides)})

    for n in (19, 20, 21):
        waits = [100] * n; waits[-1] = 900
        add(f"P95-{n}", "p95", f"n={n}", f"p95-{n}", queue_run(f"p95-{n}", n, waits))
    evid_run = inputs["p95-20"]
    for threshold in (19, 20, 21):
        add(f"EVID-{threshold}", "evidence", f"threshold={threshold}", "p95-20", evid_run,
            [f"evidence.low_completed_request_threshold={threshold}"])
    for n in (7, 8, 19, 20, 39, 40, 99, 100):
        iid = f"sample-{n:03d}"
        add(f"SAMPLE-{n:03d}", "sample_quality", f"n={n}", iid,
            queue_run(iid, n, [600] * n), ["evidence.low_completed_request_threshold=0"])
    for share in (250, 299, 300, 301, 350, 500, 700, 900):
        iid = f"qtrig-{share}"
        run = queue_run(iid, 40, [share] * 40)
        for trigger in (250, 300, 350):
            add(f"QTRIG-{share}-T{trigger}", "queue_trigger", f"share={share};trigger={trigger}",
                iid, run, [f"queueing.trigger_permille={trigger}"])
    qext = [("QEXT-CLEAN", 20, 985, 12, True), ("QEXT-SHARE984", 20, 984, 12, True),
            ("QEXT-DEPTH11", 20, 985, 11, True), ("QEXT-N19", 19, 985, 12, True),
            ("QEXT-NOGROWTH", 20, 985, 12, False)]
    for eid, n, share, depth, growth in qext:
        add(eid, "queue_extreme", eid.removeprefix("QEXT-"), eid.lower(),
            queue_run(eid.lower(), n, [share] * n, depth, growth))
    add("BLOCK-MAX", "blocking", "all_nonzero", "block-max",
        runtime_run("block-max", [24] * 100, snapshots=100))
    add("BLOCK-NZ890", "blocking", "nonzero=890", "block-nz890",
        runtime_run("block-nz890", [0] * 11 + [24] * 89, snapshots=100))
    for target in (499, 500, 999, 1000, 1999, 2000, 3999, 4000, 7999, 8000):
        iid = f"exec-{target}"
        add(f"EXEC-{target}", "executor", f"normalized={target}", iid,
            runtime_run(iid, global_depth=target, worker_count=1000),
            ["evidence.low_completed_request_threshold=0"])
    target_run = inputs["exec-500"]
    for threshold in (250, 500, 1000):
        add(f"EXEC-T{threshold}", "executor_trigger", f"threshold={threshold}", "exec-500",
            target_run, ["evidence.low_completed_request_threshold=0",
                         f"executor.min_runnable_queue_per_worker_p95_milli_for_signal={threshold}"])
    for k in (2, 3, 4, 5):
        iid = f"down-k{k}-extreme"; run = downstream_run(iid, k, 990)
        for minimum in (2, 3, 4, 5):
            add(f"DOWN-K{k}-M{minimum}", "downstream", f"k={k};minimum={minimum}", iid, run,
                [f"downstream.min_stage_samples={minimum}"])
    for label, latency in (("LOW", 300), ("MOD", 600)):
        iid = f"down-k3-{label.lower()}"
        add(f"DOWN-K3-{label}", "downstream", f"k=3;latency={latency}", iid,
            downstream_run(iid, 3, latency))
    for count in (19, 20):
        iid = f"dext-s{count}"
        add(f"DEXT-S{count}", "downstream_extreme", f"samples={count}", iid,
            downstream_run(iid, count, 990))
    configs = {"CDEF": (65, 85), "CLO": (60, 80), "CHI": (70, 90)}
    for anchor, wait in ((60, 420), (65, 490), (70, 560), (85, 770), (90, 840)):
        iid = f"conf-s{anchor}"; run = queue_run(iid, 100, [wait] * 100)
        for label, (medium, high) in configs.items():
            add(f"CONF-S{anchor}-{label}", "confidence", f"anchor={anchor};config={label}", iid,
                run, [f"confidence.medium_score_threshold={medium}",
                      f"confidence.high_score_threshold={high}"])
    for n in (19, 20, 21):
        iid = f"temp-n{n}"; run = temporal_run(iid, n, 1000, 1500, 100, 300)
        for threshold in (19, 20, 21):
            add(f"TEMP-N{n}-T{threshold}", "temporal_count", f"n={n};threshold={threshold}", iid,
                run, [f"temporal.min_request_count={threshold}"])
    share_run = temporal_run("temp-share", 20, 1000, 1000, 100, 300)
    for threshold in (150, 200, 250):
        add(f"TEMP-SHARE{threshold}", "temporal_share", f"threshold={threshold}", "temp-share",
            share_run, [f"temporal.share_shift_permille={threshold}"])
    p95_run = temporal_run("temp-p95", 20, 1000, 1500, 100, 100)
    for num, den in ((4, 3), (3, 2), (5, 3)):
        add(f"TEMP-P95-{num}-{den}", "temporal_p95", f"ratio={num}/{den}", "temp-p95",
            p95_run, [f"temporal.p95_shift_ratio_numerator={num}",
                      f"temporal.p95_shift_ratio_denominator={den}"])
    assert len(experiments) == 108 and len({e["experiment_id"] for e in experiments}) == 108
    return experiments, inputs


def csv_bytes(rows: list[dict[str, Any]], fields: list[str]) -> bytes:
    stream = io.StringIO(newline="")
    writer = csv.DictWriter(stream, fieldnames=fields, lineterminator="\n")
    writer.writeheader(); writer.writerows(rows)
    return stream.getvalue().encode()


def ensure_output(output: Path) -> Path:
    output = output.resolve()
    try:
        output.relative_to(REPO.resolve())
    except ValueError as error:
        raise SystemExit(f"--output must be under repository root {REPO}") from error
    return output


def write_plan(output: Path) -> tuple[list[dict[str, Any]], dict[str, dict[str, Any]]]:
    experiments, inputs = generate_plan()
    (output / "inputs").mkdir(parents=True, exist_ok=True)
    for input_id, run in sorted(inputs.items()):
        (output / "inputs" / f"{input_id}.json").write_bytes(canonical_json(run))
    rows = [{**e, "overrides": ";".join(e["overrides"])} for e in experiments]
    (output / "experiment-plan.csv").write_bytes(csv_bytes(
        rows, ["experiment_id", "family", "variant", "input_id", "overrides"]))
    (output / "README.md").write_text(
        "# Analyzer numeric sensitivity output\n\n"
        "Generated by `scripts/analyzer_numeric_sensitivity.py`. This manual/local deterministic "
        "characterization invokes the real CLI; it is not a scoring implementation, production "
        "calibration benchmark, root-cause proof, CI requirement, or release gate.\n\n"
        "`aggregate-sha256.txt` hashes, in experiment-plan order, each experiment ID plus its input "
        "and stdout SHA-256 using NUL separators. Environment metadata is excluded.\n",
        encoding="utf-8")
    return experiments, inputs


def binary_path() -> Path:
    name = "tailtriage.exe" if os.name == "nt" else "tailtriage"
    return REPO / "target" / "debug" / name


def command(binary: Path, input_path: Path, overrides: list[str]) -> list[str]:
    argv = [str(binary.relative_to(REPO)), "analyze", str(input_path.relative_to(REPO)),
            "--format", "json"]
    for override in overrides:
        argv.extend(["--analyzer-set", override])
    return argv


def all_suspects(report: dict[str, Any]) -> list[dict[str, Any]]:
    return [report["primary_suspect"], *report.get("secondary_suspects", [])]


def project(exp: dict[str, Any], input_hash: str, report: dict[str, Any], stdout_hash: str) -> dict[str, Any]:
    suspects = all_suspects(report)
    def candidate(family: str) -> dict[str, Any] | None:
        return next((s for s in suspects if s["kind"] == FAMILY_KIND[family]), None)
    row: dict[str, Any] = {
        **{key: exp[key] for key in ("experiment_id", "family", "variant", "input_id")},
        "input_sha256": input_hash, "overrides": ";".join(exp["overrides"]),
        "request_count": report["request_count"], "p95_latency_us": report["p95_latency_us"],
        "p95_queue_share_permille": report.get("p95_queue_share_permille"),
        "evidence_quality": report["evidence_quality"]["quality"],
        "primary_kind": report["primary_suspect"]["kind"],
        "primary_score": report["primary_suspect"]["score"],
        "primary_confidence": report["primary_suspect"]["confidence"],
        "ambiguity_warning": any("close in score" in w for w in report["warnings"]),
        "low_request_confidence_note": any("Low completed-request count" in note
            for s in suspects for note in s.get("confidence_notes", [])),
        "route_breakdown_count": len(report.get("route_breakdowns", [])),
        "temporal_segment_count": len(report.get("temporal_segments", [])),
        "report_sha256": stdout_hash,
    }
    for family in FAMILY_KIND:
        found = candidate(family)
        row[f"{family}_present"] = found is not None
        row[f"{family}_score"] = found["score"] if found else None
        row[f"{family}_confidence"] = found["confidence"] if found else None
    return row


def aggregate(records: list[dict[str, Any]]) -> str:
    digest = hashlib.sha256()
    for record in records:
        digest.update(record["experiment_id"].encode()); digest.update(b"\0")
        digest.update(record["input_sha256"].encode()); digest.update(b"\0")
        digest.update(record["stdout_sha256"].encode()); digest.update(b"\0")
    return digest.hexdigest()


def git_head() -> str:
    result = subprocess.run(["git", "rev-parse", "HEAD"], cwd=REPO, shell=False,
                            capture_output=True, text=True, check=False)
    return result.stdout.strip() if result.returncode == 0 else "unavailable"


def execute(output: Path, experiments: list[dict[str, Any]], verify: bool = False) -> None:
    binary = binary_path()
    if not binary.is_file():
        raise SystemExit(f"expected built CLI at {binary}")
    stdout_dir = output / "stdout"; stderr_dir = output / "stderr"
    if not verify:
        stdout_dir.mkdir(exist_ok=True); stderr_dir.mkdir(exist_ok=True)
    recorded_logs = {}
    recorded_rows: bytes | None = None
    recorded_aggregate: str | None = None
    if verify:
        for line in (output / "execution-log.jsonl").read_text(encoding="utf-8").splitlines():
            item = json.loads(line); recorded_logs[item["experiment_id"]] = item
        recorded_rows = (output / "results.csv").read_bytes()
        recorded_aggregate = (output / "aggregate-sha256.txt").read_text().strip()
    logs = []; raw = []; rows = []
    for exp in experiments:
        input_path = output / "inputs" / f"{exp['input_id']}.json"
        input_data = input_path.read_bytes(); input_hash = sha256(input_data)
        argv = command(binary, input_path, exp["overrides"])
        result = subprocess.run(argv, cwd=REPO, shell=False, capture_output=True, check=False)
        stdout_hash, stderr_hash = sha256(result.stdout), sha256(result.stderr)
        log = {"experiment_id": exp["experiment_id"], "argv": argv,
               "input_path": str(input_path.relative_to(REPO)), "input_sha256": input_hash,
               "overrides": exp["overrides"], "exit_code": result.returncode,
               "stdout_sha256": stdout_hash, "stderr_sha256": stderr_hash}
        if result.returncode != 0:
            raise SystemExit(f"{exp['experiment_id']} failed: {result.stderr.decode(errors='replace')}")
        report = json.loads(result.stdout)
        logs.append(log); raw.append({"experiment_id": exp["experiment_id"],
                                      "stdout_sha256": stdout_hash, "report": report})
        rows.append(project(exp, input_hash, report, stdout_hash))
        if verify:
            saved = recorded_logs.get(exp["experiment_id"])
            if saved != log:
                raise SystemExit(f"verification mismatch in execution log for {exp['experiment_id']}")
            if sha256((stdout_dir / f"{exp['experiment_id']}.json").read_bytes()) != stdout_hash:
                raise SystemExit(f"saved stdout hash mismatch for {exp['experiment_id']}")
            if sha256((stderr_dir / f"{exp['experiment_id']}.txt").read_bytes()) != stderr_hash:
                raise SystemExit(f"saved stderr hash mismatch for {exp['experiment_id']}")
        else:
            (stdout_dir / f"{exp['experiment_id']}.json").write_bytes(result.stdout)
            (stderr_dir / f"{exp['experiment_id']}.txt").write_bytes(result.stderr)
    rows_bytes = csv_bytes(rows, RESULT_FIELDS)
    computed_aggregate = aggregate(logs)
    if verify:
        if rows_bytes != recorded_rows or computed_aggregate != recorded_aggregate:
            raise SystemExit("normalized results or aggregate hash did not reproduce")
        (output / "verification.txt").write_text("verified 108 experiments\n", encoding="utf-8")
        return
    (output / "raw-results.jsonl").write_bytes(b"".join(canonical_json(x) for x in raw))
    (output / "results.csv").write_bytes(rows_bytes)
    (output / "execution-log.jsonl").write_bytes(b"".join(canonical_json(x) for x in logs))
    (output / "aggregate-sha256.txt").write_text(computed_aggregate + "\n", encoding="utf-8")
    (output / "environment.txt").write_text(f"git_head={git_head()}\npython={sys.version_info.major}.{sys.version_info.minor}\n",
                                             encoding="utf-8")
    # Run the same independent checks immediately, as part of `run`.
    execute(output, experiments, verify=True)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, default=DEFAULT_OUTPUT,
                        help="artifact directory under this repository (default: target/analyzer-numeric-sensitivity)")
    sub = parser.add_subparsers(dest="command", required=True)
    sub.add_parser("plan", help="generate the deterministic plan and schema-v2 inputs")
    run_parser = sub.add_parser("run", help="build once, execute all experiments, and verify output")
    run_parser.add_argument("--skip-build", action="store_true", help="use an existing debug CLI binary")
    sub.add_parser("verify", help="rerun recorded commands and verify hashes and normalized results")
    args = parser.parse_args(argv)
    output = ensure_output(args.output)
    if args.command == "plan":
        write_plan(output); print("generated 108 experiments"); return 0
    if args.command == "run":
        experiments, _ = write_plan(output)
        if not args.skip_build:
            subprocess.run(["cargo", "build", "-p", "tailtriage-cli", "--locked"], cwd=REPO,
                           shell=False, check=True)
        execute(output, experiments); print("ran and verified 108 experiments"); return 0
    experiments, _ = generate_plan()
    execute(output, experiments, verify=True); print("verified 108 experiments"); return 0


if __name__ == "__main__":
    raise SystemExit(main())
