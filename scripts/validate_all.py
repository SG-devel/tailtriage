#!/usr/bin/env python3
from __future__ import annotations

import argparse
import json
import os
import platform
import subprocess
import sys
from dataclasses import dataclass, asdict
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
    name: str
    track: str
    argv: list[str]
    started_at_utc: str
    finished_at_utc: str
    duration_seconds: float
    exit_code: int
    stdout_path: str
    stderr_path: str


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def default_out_dir(profile: str) -> Path:
    return Path("target") / "validation" / profile


def derive_publish_dir() -> Path:
    sha = safe_cmd(["git", "rev-parse", "--short", "HEAD"], default="unknown")
    return Path("validation") / "artifacts" / f"{datetime.now(timezone.utc):%Y%m%d}-git-{sha}"


def safe_cmd(argv: list[str], default: str = "unknown") -> str:
    try:
        p = subprocess.run(argv, check=True, capture_output=True, text=True)
        return p.stdout.strip() or default
    except Exception:
        return default


def add_cargo(specs: list[CommandSpec]) -> None:
    specs.extend([
        CommandSpec("cargo_fmt", "cargo", ["cargo", "fmt", "--check"]),
        CommandSpec("cargo_clippy", "cargo", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
        CommandSpec("cargo_test", "cargo", ["cargo", "test", "--workspace"]),
    ])


def build_plan(args: argparse.Namespace, out: Path) -> list[CommandSpec]:
    py = args.python
    specs: list[CommandSpec] = []
    specs.append(CommandSpec("diagnostic_benchmark", "diagnostics", [py, "scripts/diagnostic_benchmark.py", "--manifest", "validation/diagnostics/manifest.json", "--output", str(out / "diagnostics/benchmark-summary.json")]))

    if args.profile in {"smoke", "ci", "full", "publish"}:
        specs.append(CommandSpec("validate_docs_contracts", "docs", [py, "scripts/validate_docs_contracts.py"]))

    if args.profile == "smoke":
        specs.extend([
            CommandSpec("diagnostic_matrix_smoke", "diagnostic_matrix", [py, "scripts/run_diagnostic_matrix.py", "--runs", "1", "--scenario", "queue", "--profile", args.profile_mode, "--out", str(out / "diagnostic-matrix/runs.jsonl"), "--summary", str(out / "diagnostic-matrix/summary.json"), "--scorecard", str(out / "diagnostic-matrix/scorecard.md"), "--no-fail-thresholds"]),
            CommandSpec("mitigation_smoke", "mitigation", [py, "scripts/run_mitigation_matrix.py", "--scenario", "queue", "--profile", args.profile_mode, "--out", str(out / "mitigation/runs.jsonl"), "--summary", str(out / "mitigation/summary.json"), "--scorecard", str(out / "mitigation/scorecard.md"), "--no-fail-thresholds"]),
            CommandSpec("runtime_cost_smoke", "runtime_cost", [py, "scripts/run_operational_validation.py", "--domain", "runtime-cost", "--scenario", "queue", "--runs", "1", "--profile", args.profile_mode, "--out", str(out / "operational/runtime-cost.jsonl"), "--summary", str(out / "operational/runtime-cost-summary.json"), "--scorecard", str(out / "operational/runtime-cost-scorecard.md"), "--no-fail-thresholds"]),
            CommandSpec("collector_limits_smoke", "collector_limits", [py, "scripts/run_operational_validation.py", "--domain", "collector-limits", "--scenario", "queue-limit-pressure", "--profile", args.profile_mode, "--out", str(out / "operational/collector-limits.jsonl"), "--summary", str(out / "operational/collector-limits-summary.json"), "--scorecard", str(out / "operational/collector-limits-scorecard.md"), "--no-fail-thresholds"]),
        ])
    if args.profile == "ci":
        specs.extend([
            CommandSpec("test_diagnostic_benchmark", "diagnostics", [py, "-m", "unittest", "scripts.tests.test_diagnostic_benchmark"]),
            CommandSpec("test_validate_docs_contracts", "docs", [py, "-m", "unittest", "scripts.tests.test_validate_docs_contracts"]),
            CommandSpec("test_run_diagnostic_matrix", "diagnostic_matrix", [py, "-m", "unittest", "scripts.tests.test_run_diagnostic_matrix"]),
            CommandSpec("test_run_mitigation_matrix", "mitigation", [py, "-m", "unittest", "scripts.tests.test_run_mitigation_matrix"]),
            CommandSpec("test_run_operational_validation", "operational", [py, "-m", "unittest", "scripts.tests.test_run_operational_validation"]),
            CommandSpec("check_demo_fixture_drift", "diagnostics", [py, "scripts/check_demo_fixture_drift.py", "--profile", args.profile_mode]),
        ])
    if args.profile in {"full", "publish"}:
        runs = str(args.runs)
        specs.extend([
            CommandSpec("diagnostic_matrix_full", "diagnostic_matrix", [py, "scripts/run_diagnostic_matrix.py", "--runs", runs, "--scenario", "queue", "--scenario", "blocking", "--scenario", "executor", "--scenario", "downstream", "--profile", args.profile_mode, "--out", str(out / "diagnostic-matrix/runs.jsonl"), "--summary", str(out / "diagnostic-matrix/summary.json"), "--scorecard", str(out / "diagnostic-matrix/scorecard.md")]),
            CommandSpec("mitigation_full", "mitigation", [py, "scripts/run_mitigation_matrix.py", "--scenario", "queue", "--scenario", "blocking", "--scenario", "downstream", "--scenario", "db-pool", "--profile", args.profile_mode, "--out", str(out / "mitigation/runs.jsonl"), "--summary", str(out / "mitigation/summary.json"), "--scorecard", str(out / "mitigation/scorecard.md")]),
            CommandSpec("operational_all", "operational", [py, "scripts/run_operational_validation.py", "--domain", "all", "--runs", runs, "--profile", args.profile_mode, "--out", str(out / "operational/operational-validation.jsonl"), "--summary", str(out / "operational/operational-validation-summary.json"), "--scorecard", str(out / "operational/operational-validation-scorecard.md")]),
        ])

    if args.no_fail_thresholds:
        for s in specs:
            if s.argv[1].endswith(("run_diagnostic_matrix.py", "run_mitigation_matrix.py", "run_operational_validation.py")) and "--no-fail-thresholds" not in s.argv:
                s.argv.append("--no-fail-thresholds")

    if args.profile in {"full", "publish"} and not args.skip_cargo:
        add_cargo(specs)
    if args.profile in {"smoke", "ci"} and args.include_cargo and not args.skip_cargo:
        add_cargo(specs)
    return specs


def run_command(spec: CommandSpec, log_dir: Path) -> CommandResult:
    started = datetime.now(timezone.utc)
    log_dir.mkdir(parents=True, exist_ok=True)
    outp = log_dir / f"{spec.name}.stdout.log"
    errp = log_dir / f"{spec.name}.stderr.log"
    p = subprocess.run(spec.argv, text=True, capture_output=True)
    outp.write_text(p.stdout or "", encoding="utf-8")
    errp.write_text(p.stderr or "", encoding="utf-8")
    finished = datetime.now(timezone.utc)
    return CommandResult(spec.name, spec.track, spec.argv, started.isoformat(), finished.isoformat(), (finished - started).total_seconds(), p.returncode, str(outp), str(errp))


def write_commands_jsonl(path: Path, results: list[CommandResult]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    with path.open("w", encoding="utf-8") as f:
        for r in results:
            f.write(json.dumps(asdict(r), sort_keys=True) + "\n")


def collect_environment(profile_mode: str) -> dict[str, Any]:
    return {"schema_version": 1, "git_sha": safe_cmd(["git", "rev-parse", "HEAD"]), "git_branch": safe_cmd(["git", "rev-parse", "--abbrev-ref", "HEAD"]), "rustc": safe_cmd(["rustc", "--version"]), "cargo": safe_cmd(["cargo", "--version"]), "python": sys.executable, "target": platform.machine(), "os": platform.system(), "kernel": platform.release(), "cpu_model": platform.processor() or "unknown", "physical_cores": None, "logical_cores": os.cpu_count() or 0, "memory_gb": None, "build_profile": profile_mode, "features": [], "tokio_unstable": False, "timestamp_utc": utc_now()}


def summarize_results(results: list[CommandResult], profile: str, profile_mode: str, out: Path, started: str, finished: str) -> dict[str, Any]:
    failed = [r for r in results if r.exit_code != 0]
    tracks = {}
    for track in ["diagnostics", "diagnostic_matrix", "mitigation", "runtime_cost", "collector_limits", "docs", "cargo", "operational"]:
        rows = [r for r in results if r.track == track]
        tracks[track] = {"status": "skipped" if not rows else ("failed" if any(r.exit_code != 0 for r in rows) else "passed")}
    return {"schema_version": 1, "profile": profile, "profile_mode": profile_mode, "out_dir": str(out), "started_at_utc": started, "finished_at_utc": finished, "duration_seconds": None, "status": "failed" if failed else "passed", "commands": {"total": len(results), "passed": len(results)-len(failed), "failed": len(failed)}, "tracks": tracks, "failed_commands": [asdict(r) for r in failed]}


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    t=summary["tracks"]
    lines=["# Tailtriage validation scorecard","",f"Profile: {summary['profile']}",f"Build profile: {summary['profile_mode']}",f"Status: {summary['status']}",f"Generated: {summary['finished_at_utc']}","","| Track | Status | Output | Notes |","|---|---|---|---|",
           f"| Deterministic diagnostics | {t['diagnostics']['status']} | diagnostics/benchmark-summary.json | corpus benchmark |",
           f"| Repeated-run diagnostic matrix | {t['diagnostic_matrix']['status']} | diagnostic-matrix/summary.json | machine/workload scoped |",
           f"| Mitigation matrix | {t['mitigation']['status']} | mitigation/summary.json | baseline vs mitigated evidence movement |",
           f"| Runtime cost | {t['runtime_cost']['status']} | operational/runtime-cost-summary.json | measured, not universal |",
           f"| Collector limits | {t['collector_limits']['status']} | operational/collector-limits-summary.json | visible bounded drops + warnings/downgrades |",
           f"| Docs contracts | {t['docs']['status']} | logs/commands.jsonl | docs consistency |",
           f"| Cargo checks | {t['cargo']['status']} | logs/commands.jsonl | skipped by profile/config when not selected |",
           "","> Suspects are leads, not proof of root cause.","> Runtime-cost values are machine/workload/profile scoped.","> Collector-limit checks do not claim no drops.","> Generated outputs are local unless explicitly published."]
    path.write_text("\n".join(lines)+"\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    p=argparse.ArgumentParser()
    p.add_argument("--profile", choices=["smoke","ci","full","publish"], default="smoke")
    p.add_argument("--out")
    p.add_argument("--runs", type=int)
    p.add_argument("--profile-mode", choices=["dev","release"], default="dev")
    p.add_argument("--skip-cargo", action="store_true")
    p.add_argument("--include-cargo", action="store_true")
    p.add_argument("--no-fail-fast", action="store_true")
    p.add_argument("--no-fail-thresholds", action="store_true")
    p.add_argument("--dry-run", action="store_true")
    p.add_argument("--python", default=sys.executable)
    return p.parse_args()


def main() -> int:
    args=parse_args()
    if args.runs is None:
        args.runs = {"smoke":1,"ci":1,"full":30,"publish":50}[args.profile]
    out = Path(args.out) if args.out else (derive_publish_dir() if args.profile=="publish" else default_out_dir(args.profile))
    plan=build_plan(args,out)
    if args.dry_run:
        print(json.dumps([asdict(s) for s in plan], indent=2))
        return 0
    out.mkdir(parents=True, exist_ok=True)
    env = collect_environment(args.profile_mode)
    (out/"environment.json").write_text(json.dumps(env, indent=2)+"\n", encoding="utf-8")
    started=utc_now(); results=[]
    for spec in plan:
        r=run_command(spec, out/"logs")
        results.append(r)
        if r.exit_code!=0 and not args.no_fail_fast:
            break
    finished=utc_now()
    write_commands_jsonl(out/"logs/commands.jsonl", results)
    summary=summarize_results(results,args.profile,args.profile_mode,out,started,finished)
    (out/"summary.json").write_text(json.dumps(summary, indent=2)+"\n", encoding="utf-8")
    write_scorecard(out/"scorecard.md", summary)
    return 0 if args.dry_run or summary["commands"]["failed"]==0 else 1


if __name__=="__main__":
    raise SystemExit(main())
