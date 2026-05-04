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


def utc_now() -> str:
    return datetime.now(timezone.utc).replace(microsecond=0).isoformat().replace("+00:00", "Z")


def default_out_dir(profile: str) -> Path:
    return Path("target") / "validation" / profile


def _git_output(args: list[str]) -> str | None:
    try:
        out = subprocess.run(args, check=True, capture_output=True, text=True).stdout.strip()
        return out or None
    except Exception:
        return None


def derive_publish_dir() -> Path:
    date = datetime.now(timezone.utc).strftime("%Y%m%d")
    sha = (_git_output(["git", "rev-parse", "--short", "HEAD"]) or "unknown")
    return Path("validation") / "artifacts" / f"{date}-git-{sha}"


def validate_args(args: argparse.Namespace) -> None:
    if args.runs is not None and args.runs <= 0:
        raise SystemExit("--runs must be > 0")


def _base_cmd(py: str, script: str) -> list[str]:
    return [py, script]


def build_plan(args: argparse.Namespace) -> list[CommandSpec]:
    py = args.python
    p: list[CommandSpec] = []
    out = Path(args.out)
    no_fail = ["--no-fail-thresholds"] if args.no_fail_thresholds else []
    profile_flag = ["--profile", args.profile_mode]

    p.append(CommandSpec("diagnostic benchmark", "diagnostics", _base_cmd(py, "scripts/diagnostic_benchmark.py") + ["--manifest", "validation/diagnostics/manifest.json", "--output", str(out / "diagnostics" / "benchmark-summary.json")]))
    p.append(CommandSpec("docs contracts", "docs", _base_cmd(py, "scripts/validate_docs_contracts.py")))

    if args.profile in {"smoke", "full", "publish"}:
        runs = args.runs if args.runs is not None else (1 if args.profile == "smoke" else (30 if args.profile == "full" else 50))
        p.append(CommandSpec("diagnostic matrix", "diagnostic_matrix", _base_cmd(py, "scripts/run_diagnostic_matrix.py") + profile_flag + ["--runs", str(runs), "--scenario", "queue" if args.profile == "smoke" else "queue", "--out", str(out / "diagnostic-matrix" / "runs.jsonl"), "--summary", str(out / "diagnostic-matrix" / "summary.json"), "--scorecard", str(out / "diagnostic-matrix" / "scorecard.md")] + no_fail))
        if args.profile != "smoke":
            for s in ["blocking", "executor", "downstream"]:
                p[-1].argv.extend(["--scenario", s])

        p.append(CommandSpec("mitigation matrix", "mitigation", _base_cmd(py, "scripts/run_mitigation_matrix.py") + profile_flag + ["--scenario", "queue", "--out", str(out / "mitigation" / "runs.jsonl"), "--summary", str(out / "mitigation" / "summary.json"), "--scorecard", str(out / "mitigation" / "scorecard.md")] + no_fail))
        if args.profile != "smoke":
            for s in ["blocking", "downstream", "db-pool"]:
                p[-1].argv.extend(["--scenario", s])

        if args.profile == "smoke":
            p.append(CommandSpec("runtime-cost smoke", "runtime_cost", _base_cmd(py, "scripts/run_operational_validation.py") + profile_flag + ["--domain", "runtime-cost", "--scenario", "queue", "--runs", "1", "--out", str(out / "operational" / "runtime-cost.jsonl"), "--summary", str(out / "operational" / "runtime-cost-summary.json"), "--scorecard", str(out / "operational" / "runtime-cost-scorecard.md")] + no_fail))
            p.append(CommandSpec("collector-limits smoke", "collector_limits", _base_cmd(py, "scripts/run_operational_validation.py") + profile_flag + ["--domain", "collector-limits", "--scenario", "queue-limit-pressure", "--out", str(out / "operational" / "collector-limits.jsonl"), "--summary", str(out / "operational" / "collector-limits-summary.json"), "--scorecard", str(out / "operational" / "collector-limits-scorecard.md")] + no_fail))
        else:
            p.append(CommandSpec("operational all", "operational", _base_cmd(py, "scripts/run_operational_validation.py") + profile_flag + ["--domain", "all", "--runs", str(runs), "--out", str(out / "operational" / "operational-validation.jsonl"), "--summary", str(out / "operational" / "operational-validation-summary.json"), "--scorecard", str(out / "operational" / "operational-validation-scorecard.md")] + no_fail))

    if args.profile == "ci":
        p.extend([
            CommandSpec("diagnostic benchmark tests", "tests", _base_cmd(py, "-m") + ["unittest", "scripts.tests.test_diagnostic_benchmark"]),
            CommandSpec("diagnostic matrix tests", "tests", _base_cmd(py, "-m") + ["unittest", "scripts.tests.test_run_diagnostic_matrix"]),
            CommandSpec("mitigation matrix tests", "tests", _base_cmd(py, "-m") + ["unittest", "scripts.tests.test_run_mitigation_matrix"]),
            CommandSpec("operational tests", "tests", _base_cmd(py, "-m") + ["unittest", "scripts.tests.test_run_operational_validation"]),
            CommandSpec("docs contract tests", "tests", _base_cmd(py, "-m") + ["unittest", "scripts.tests.test_validate_docs_contracts"]),
            CommandSpec("demo fixture drift", "tests", _base_cmd(py, "scripts/check_demo_fixture_drift.py") + profile_flag),
        ])

    run_cargo = (args.profile in {"full", "publish"} and not args.skip_cargo) or args.include_cargo
    if run_cargo:
        p.extend([
            CommandSpec("cargo fmt", "cargo", ["cargo", "fmt", "--check"]),
            CommandSpec("cargo clippy", "cargo", ["cargo", "clippy", "--workspace", "--all-targets", "--", "-D", "warnings"]),
            CommandSpec("cargo test", "cargo", ["cargo", "test", "--workspace"]),
        ])
    return p


def run_command(spec: CommandSpec, log_dir: Path, env: dict[str, str]) -> dict[str, Any]:
    started = utc_now()
    proc = subprocess.run(spec.argv, capture_output=True, text=True, env=env)
    finished = utc_now()
    stem = spec.name.replace(" ", "-")
    stdout = log_dir / f"{stem}.stdout.log"
    stderr = log_dir / f"{stem}.stderr.log"
    stdout.write_text(proc.stdout or "", encoding="utf-8")
    stderr.write_text(proc.stderr or "", encoding="utf-8")
    return {"name": spec.name, "track": spec.track, "argv": spec.argv, "start_time_utc": started, "end_time_utc": finished, "duration_seconds": 0.0, "exit_code": proc.returncode, "stdout_path": str(stdout), "stderr_path": str(stderr)}


def write_commands_jsonl(path: Path, results: list[dict[str, Any]]) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text("".join(json.dumps(r) + "\n" for r in results), encoding="utf-8")


def collect_environment(profile_mode: str) -> dict[str, Any]:
    return {
        "schema_version": 1,
        "git_sha": _git_output(["git", "rev-parse", "HEAD"]),
        "git_branch": _git_output(["git", "rev-parse", "--abbrev-ref", "HEAD"]),
        "rustc": _git_output(["rustc", "--version"]),
        "cargo": _git_output(["cargo", "--version"]),
        "python": sys.version.split()[0],
        "target": platform.machine(),
        "os": platform.system(),
        "kernel": platform.release(),
        "cpu_model": platform.processor() or None,
        "physical_cores": None,
        "logical_cores": os.cpu_count() or 0,
        "memory_gb": None,
        "build_profile": profile_mode,
        "features": [],
        "tokio_unstable": False,
        "timestamp_utc": utc_now(),
    }


def summarize_results(results: list[dict[str, Any]], profile: str, profile_mode: str, out_dir: Path, started: str, finished: str) -> dict[str, Any]:
    failed = [r for r in results if r["exit_code"] != 0]
    tracks = {t: "passed" for t in ["diagnostics", "diagnostic_matrix", "mitigation", "runtime_cost", "collector_limits", "operational", "docs", "cargo", "tests"]}
    for r in results:
        if r["exit_code"] != 0:
            tracks[r["track"]] = "failed"
    return {"schema_version": 1, "profile": profile, "profile_mode": profile_mode, "out_dir": str(out_dir), "started_at_utc": started, "finished_at_utc": finished, "duration_seconds": 0.0, "status": "failed" if failed else "passed", "commands": {"total": len(results), "passed": len(results) - len(failed), "failed": len(failed)}, "tracks": tracks, "failed_commands": [{"name": r["name"], "exit_code": r["exit_code"]} for r in failed]}


def write_summary(path: Path, summary: dict[str, Any]) -> None:
    path.write_text(json.dumps(summary, indent=2) + "\n", encoding="utf-8")


def write_scorecard(path: Path, summary: dict[str, Any]) -> None:
    lines = ["# Tailtriage validation scorecard", "", f"Profile: {summary['profile']}", f"Build profile: {summary['profile_mode']}", f"Status: {summary['status']}", f"Generated: {summary['finished_at_utc']}", "", "Root cause is not proven by this triage validation.", "Runtime-cost results are machine/workload scoped.", "Collector-limit checks validate bounded and visible drops; they do not claim no drops.", "Generated outputs are local unless explicitly published."]
    path.write_text("\n".join(lines) + "\n", encoding="utf-8")


def parse_args() -> argparse.Namespace:
    ap = argparse.ArgumentParser()
    ap.add_argument("--profile", choices=["smoke", "ci", "full", "publish"], default="smoke")
    ap.add_argument("--out")
    ap.add_argument("--runs", type=int)
    ap.add_argument("--profile-mode", choices=["dev", "release"], default="dev")
    ap.add_argument("--skip-cargo", action="store_true")
    ap.add_argument("--include-cargo", action="store_true")
    ap.add_argument("--no-fail-fast", action="store_true")
    ap.add_argument("--no-fail-thresholds", action="store_true")
    ap.add_argument("--dry-run", action="store_true")
    ap.add_argument("--python", default=sys.executable)
    return ap.parse_args()


def main() -> None:
    args = parse_args()
    if not args.out:
        args.out = str(derive_publish_dir() if args.profile == "publish" else default_out_dir(args.profile))
    validate_args(args)
    plan = build_plan(args)
    if args.dry_run:
        print(json.dumps([asdict(x) for x in plan], indent=2))
        return
    out = Path(args.out)
    (out / "logs").mkdir(parents=True, exist_ok=True)
    env_meta = collect_environment(args.profile_mode)
    (out / "environment.json").write_text(json.dumps(env_meta, indent=2) + "\n", encoding="utf-8")
    started = utc_now()
    results = []
    env = os.environ.copy()
    for spec in plan:
        r = run_command(spec, out / "logs", env)
        results.append(r)
        if r["exit_code"] != 0 and not args.no_fail_fast:
            break
    finished = utc_now()
    write_commands_jsonl(out / "logs" / "commands.jsonl", results)
    summary = summarize_results(results, args.profile, args.profile_mode, out, started, finished)
    write_summary(out / "summary.json", summary)
    write_scorecard(out / "scorecard.md", summary)
    if summary["status"] != "passed":
        raise SystemExit(1)


if __name__ == "__main__":
    main()
