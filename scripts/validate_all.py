#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
from dataclasses import dataclass
from datetime import datetime, timezone
from pathlib import Path
from typing import Any


@dataclass
class CommandSpec:
    name: str
    track: str
    argv: list[str]


@dataclass
class CommandResult:
    spec: CommandSpec
    started_at_utc: str
    finished_at_utc: str
    duration_seconds: float
    exit_code: int
    stdout_log: str
    stderr_log: str


def utc_now() -> str:
    return datetime.now(timezone.utc).isoformat().replace("+00:00", "Z")


def default_out_dir(profile: str) -> Path:
    return Path("target") / "validation" / profile


def default_runs(profile: str) -> int:
    return 1 if profile in {"smoke", "ci"} else 5


def derive_publish_dir() -> Path:
    day = datetime.now(timezone.utc).strftime("%Y%m%d")
    sha = "unknown"
    try:
        sha = subprocess.run(["git", "rev-parse", "--short", "HEAD"], check=True, capture_output=True, text=True).stdout.strip()
    except Exception:
        pass
    return Path("validation") / "artifacts" / f"{day}-git-{sha}"


def _py(args: argparse.Namespace, *extra: str) -> list[str]:
    return [args.python, *extra]


def build_plan(args: argparse.Namespace) -> list[CommandSpec]:
    out = Path(args.out)
    runtime_cost_artifact_dir = out / "operational" / "runtime-cost"
    collector_limits_artifact_dir = out / "operational" / "collector-limits"
    deterministic_benchmark_args = [
        "scripts/diagnostic_benchmark.py",
        "--manifest",
        "validation/diagnostics/manifest.json",
        "--min-top1",
        "0.75",
        "--min-top2",
        "0.90",
        "--max-high-confidence-wrong",
        "0",
        "--output",
        str(out / "diagnostics/benchmark-summary.json"),
    ]
    cmds: list[CommandSpec] = [
        CommandSpec("deterministic benchmark", "diagnostics", _py(args, *deterministic_benchmark_args)),
        CommandSpec("docs contract", "docs", _py(args, "scripts/validate_docs_contracts.py")),
    ]
    nfts = ["--no-fail-thresholds"] if args.no_fail_thresholds else []
    mode = ["--profile", args.profile_mode]

    if args.profile == "smoke":
        cmds += [
            CommandSpec("mitigation smoke", "mitigation", _py(args, "scripts/demo_tool.py", "mitigation-report", "--scenario", "queue", "--out", str(out / "mitigation/runs.jsonl"), "--summary", str(out / "mitigation/summary.json"), "--scorecard", str(out / "mitigation/scorecard.md"), *mode, *nfts)),
            CommandSpec("runtime-cost smoke", "runtime_cost", _py(args, "scripts/measure_runtime_cost.py", "--requests", "800", "--concurrency", "32", "--work-ms", "2", "--rounds", "1", "--warmup-rounds", "0", "--artifact-dir", str(runtime_cost_artifact_dir))),
            CommandSpec("collector-limits smoke", "collector_limits", _py(args, "scripts/measure_collector_limits.py", "--profile", "smoke", "--repeats", "1", "--artifact-dir", str(collector_limits_artifact_dir))),
        ]
    if args.profile in {"ci", "full", "publish"}:
        cmds += [
            CommandSpec("benchmark tests", "diagnostics", _py(args, "-m", "unittest", "scripts.tests.test_diagnostic_benchmark")),
            CommandSpec("diag matrix tests", "diagnostic_matrix", _py(args, "-m", "unittest", "scripts.tests.test_run_diagnostic_matrix")),
            CommandSpec("live-demo policy tests", "mitigation", _py(args, "-m", "unittest", "scripts.tests.test_demo_scripts")),
            CommandSpec("runtime-cost tests", "runtime_cost", _py(args, "-m", "unittest", "scripts.tests.test_measure_runtime_cost")),
            CommandSpec("collector-limits tests", "collector_limits", _py(args, "-m", "unittest", "scripts.tests.test_measure_collector_limits")),
            CommandSpec("docs contract tests", "docs", _py(args, "-m", "unittest", "scripts.tests.test_validate_docs_contracts")),
            CommandSpec("fixture drift", "diagnostics", _py(args, "scripts/check_demo_fixture_drift.py", "--profile", args.profile_mode)),
        ]
    if args.profile in {"full", "publish"}:
        cmds += [
            CommandSpec("diag matrix full", "diagnostic_matrix", _py(args, "scripts/run_diagnostic_matrix.py", "--runs", str(args.runs), "--scenario", "queue", "--scenario", "blocking", "--scenario", "executor", "--scenario", "downstream", "--out", str(out / "diagnostic-matrix/runs.jsonl"), "--summary", str(out / "diagnostic-matrix/summary.json"), "--scorecard", str(out / "diagnostic-matrix/scorecard.md"), *mode, *nfts)),
            CommandSpec("mitigation full", "mitigation", _py(args, "scripts/demo_tool.py", "mitigation-report", "--scenario", "queue", "--scenario", "blocking", "--scenario", "downstream", "--scenario", "db-pool", "--out", str(out / "mitigation/runs.jsonl"), "--summary", str(out / "mitigation/summary.json"), "--scorecard", str(out / "mitigation/scorecard.md"), *mode, *nfts)),
            CommandSpec("runtime-cost full", "runtime_cost", _py(args, "scripts/measure_runtime_cost.py", "--rounds", str(args.runs), "--artifact-dir", str(runtime_cost_artifact_dir))),
            CommandSpec("collector-limits full", "collector_limits", _py(args, "scripts/measure_collector_limits.py", "--profile", "default", "--repeats", "1", "--artifact-dir", str(collector_limits_artifact_dir))),
        ]

    include_cargo = not args.skip_cargo
    if include_cargo:
        cmds += [
            CommandSpec("cargo fmt", "cargo", ["cargo", "fmt", "--check"]),
            CommandSpec("cargo clippy", "cargo", ["cargo", "clippy", "--workspace", "--all-targets", "--all-features", "--locked", "--", "-D", "warnings"]),
            CommandSpec("cargo test", "cargo", ["cargo", "test", "--workspace", "--all-targets", "--all-features", "--locked"]),
        ]
    return cmds


def run_command(spec: CommandSpec, log_dir: Path, env: dict[str, str]) -> CommandResult:
    log_dir.mkdir(parents=True, exist_ok=True)
    safe = spec.name.replace(" ", "-")
    outp = log_dir / f"{safe}.stdout.log"
    errp = log_dir / f"{safe}.stderr.log"
    start = utc_now()
    t0 = datetime.now(timezone.utc)
    proc = subprocess.run(spec.argv, capture_output=True, text=True, env=env)
    t1 = datetime.now(timezone.utc)
    outp.write_text(proc.stdout or "", encoding="utf-8")
    errp.write_text(proc.stderr or "", encoding="utf-8")
    return CommandResult(spec, start, utc_now(), (t1 - t0).total_seconds(), proc.returncode, str(outp), str(errp))


def write_commands_jsonl(path: Path, results: list[CommandResult]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for r in results:
            f.write(json.dumps({"name": r.spec.name, "track": r.spec.track, "argv": r.spec.argv, "started_at_utc": r.started_at_utc, "finished_at_utc": r.finished_at_utc, "duration_seconds": r.duration_seconds, "exit_code": r.exit_code, "stdout_log": r.stdout_log, "stderr_log": r.stderr_log}) + "\n")


def collect_environment(profile_mode: str) -> dict[str, Any]:
    def best(cmd: list[str]) -> str | None:
        try:
            return subprocess.run(cmd, check=True, capture_output=True, text=True).stdout.strip()
        except Exception:
            return None
    return {"schema_version": 1, "git_sha": best(["git", "rev-parse", "HEAD"]), "git_branch": best(["git", "rev-parse", "--abbrev-ref", "HEAD"]), "rustc": best(["rustc", "--version"]), "cargo": best(["cargo", "--version"]), "python": sys.version.split("\n", 1)[0], "target": platform.machine(), "os": platform.system(), "kernel": platform.release(), "cpu_model": platform.processor() or None, "physical_cores": None, "logical_cores": os.cpu_count() or 0, "memory_gb": None, "build_profile": profile_mode, "features": [], "tokio_unstable": False, "timestamp_utc": utc_now()}


def summarize_results(results: list[CommandResult], profile: str, profile_mode: str, out_dir: Path, started: str, finished: str) -> dict[str, Any]:
    failed = [r for r in results if r.exit_code != 0]
    tracks: dict[str, dict[str, Any]] = {}
    for t in ["diagnostics", "diagnostic_matrix", "mitigation", "runtime_cost", "collector_limits", "docs", "cargo"]:
        tr = [r for r in results if r.spec.track == t]
        tracks[t] = {"status": "skipped" if not tr else ("passed" if all(x.exit_code == 0 for x in tr) else "failed")}
    duration_seconds = sum(r.duration_seconds for r in results)
    return {"schema_version": 1, "profile": profile, "profile_mode": profile_mode, "out_dir": str(out_dir), "started_at_utc": started, "finished_at_utc": finished, "duration_seconds": duration_seconds, "status": "passed" if not failed else "failed", "commands": {"total": len(results), "passed": len(results) - len(failed), "failed": len(failed)}, "tracks": tracks, "failed_commands": [{"name": r.spec.name, "argv": r.spec.argv, "exit_code": r.exit_code} for r in failed]}


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    lines = ["# Tailtriage validation scorecard", "", f"Profile: {summary['profile']}", f"Build profile: {summary['profile_mode']}", f"Status: {summary['status']}", f"Generated: {summary['finished_at_utc']}", "", "| Track | Status | Output | Notes |", "|---|---|---|---|", "| Deterministic diagnostics | {} | diagnostics/benchmark-summary.json | corpus benchmark |".format(summary["tracks"]["diagnostics"]["status"]), "| Repeated-run diagnostic matrix | {} | diagnostic-matrix/summary.json | machine/workload scoped |".format(summary["tracks"]["diagnostic_matrix"]["status"]), "| Live-demo mitigation | {} | mitigation/summary.json | baseline vs mitigated evidence movement |".format(summary["tracks"]["mitigation"]["status"]), "| Runtime cost | {} | operational/runtime-cost/runtime-cost-summary.json | measured, not universal |".format(summary["tracks"].get("runtime_cost", {}).get("status", "skipped")), "| Collector limits | {} | operational/collector-limits/collector-limits-*-summary.json | retained/truncation/drop evidence + measured onset/resource behavior |".format(summary["tracks"].get("collector_limits", {}).get("status", "skipped")), "| Docs contracts | {} | logs/commands.jsonl | docs consistency |".format(summary["tracks"]["docs"]["status"]), "| Cargo checks | {} | logs/commands.jsonl | profile/config dependent |".format(summary["tracks"]["cargo"]["status"]), "", "Root cause is not proven by this triage validation.", "Runtime-cost numbers are machine/workload/profile scoped.", "Collector-limit checks do not claim no drops.", "Generated outputs are local unless explicitly published."]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--profile", choices=["smoke", "ci", "full", "publish"], default="smoke")
    p.add_argument("--out")
    p.add_argument("--runs", type=int, help="diagnostic-matrix runs and runtime-cost rounds (default: 1 for smoke/ci, 5 for full/publish); does not change collector repeats")
    p.add_argument("--profile-mode", choices=["dev", "release"], default="dev")
    p.add_argument("--skip-cargo", action="store_true")
    p.add_argument("--no-fail-fast", action="store_true")
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--python", default=sys.executable)
    args = p.parse_args()
    if args.runs is None:
        args.runs = default_runs(args.profile)
    args.out = args.out or str(derive_publish_dir() if args.profile == "publish" else default_out_dir(args.profile))
    out = Path(args.out)
    plan = build_plan(args)
    if args.dry_run:
        for c in plan:
            print(" ".join(c.argv))
        return 0
    out.mkdir(parents=True, exist_ok=True)
    env = os.environ.copy()
    env_meta = collect_environment(args.profile_mode)
    (out / "environment.json").write_text(json.dumps(env_meta, indent=2) + "\n", encoding="utf-8")
    started = utc_now()
    results: list[CommandResult] = []
    for spec in plan:
        res = run_command(spec, out / "logs", env)
        results.append(res)
        if res.exit_code != 0 and not args.no_fail_fast:
            break
    finished = utc_now()
    write_commands_jsonl(out / "logs/commands.jsonl", results)
    summary = summarize_results(results, args.profile, args.profile_mode, out, started, finished)
    (out / "summary.json").write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")
    write_scorecard(out / "scorecard.md", summary)
    return 0 if summary["status"] == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main())
