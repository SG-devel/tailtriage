#!/usr/bin/env python3
import argparse
import datetime as dt
import hashlib
import json
import os
import platform
import subprocess
from pathlib import Path
import tomllib
import sys

REPO_ROOT = Path(__file__).resolve().parent.parent
if str(REPO_ROOT) not in sys.path:
    sys.path.insert(0, str(REPO_ROOT))

from scripts.diagnostic_benchmark import run as run_diagnostic_benchmark

EXPECTED_PACKAGES = [
    "tailtriage",
    "tailtriage-core",
    "tailtriage-analyzer",
    "tailtriage-cli",
    "tailtriage-tokio",
    "tailtriage-axum",
    "tailtriage-controller",
    "tailtriage-tracing",
]


def sha256_bytes(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()


def _cmd_output(argv):
    try:
        result = subprocess.run(argv, capture_output=True, text=True, check=False)
    except Exception:
        return None
    if result.returncode != 0:
        return None
    text = result.stdout.strip()
    return text or None


def _read_text(path: Path):
    try:
        return path.read_text(encoding="utf-8")
    except Exception:
        return None


def get_tailtriage_versions(repo_root: Path):
    root_manifest = repo_root / "Cargo.toml"
    data = tomllib.loads(root_manifest.read_text(encoding="utf-8"))
    workspace_version = data.get("workspace", {}).get("package", {}).get("version")
    members = data.get("workspace", {}).get("members", [])
    versions = {name: None for name in EXPECTED_PACKAGES}
    for member in members:
        manifest_path = repo_root / member / "Cargo.toml"
        if not manifest_path.exists():
            continue
        try:
            package = tomllib.loads(manifest_path.read_text(encoding="utf-8")).get("package", {})
        except Exception:
            continue
        name = package.get("name")
        if name not in versions:
            continue
        version = package.get("version")
        if isinstance(version, str):
            versions[name] = version
        elif isinstance(version, dict) and version.get("workspace") is True:
            versions[name] = workspace_version
    return {"workspace_package_version": workspace_version, "packages": versions}


def manifest_and_artifact_hashes(manifest_path: Path):
    manifest_bytes = manifest_path.read_bytes()
    manifest_sha = sha256_bytes(manifest_bytes)
    manifest = json.loads(manifest_bytes)
    root = manifest_path.parent
    artifacts = sorted({case["artifact"] for case in manifest.get("cases", [])})
    hasher = hashlib.sha256()
    for rel in artifacts:
        artifact_path = (root / rel).resolve()
        rel_norm = str(Path(rel).as_posix())
        hasher.update(rel_norm.encode("utf-8"))
        hasher.update(b"\0")
        hasher.update(artifact_path.read_bytes())
        hasher.update(b"\0")
    return manifest_sha, hasher.hexdigest()


def _linux_cpu_model():
    cpuinfo = _read_text(Path("/proc/cpuinfo"))
    if not cpuinfo:
        return None
    for line in cpuinfo.splitlines():
        if line.lower().startswith("model name"):
            return line.split(":", 1)[1].strip()
    return None


def _linux_mem_kib():
    meminfo = _read_text(Path("/proc/meminfo"))
    if not meminfo:
        return None
    for line in meminfo.splitlines():
        if line.startswith("MemTotal:"):
            parts = line.split()
            return int(parts[1]) if len(parts) > 1 and parts[1].isdigit() else None
    return None


def collect_environment(repo_root: Path, manifest_path: Path, snapshot_label, thresholds):
    manifest_sha, artifacts_sha = manifest_and_artifact_hashes(manifest_path)
    return {
        "schema_version": 2,
        "generated_at_utc": dt.datetime.now(dt.timezone.utc).isoformat(),
        "snapshot_label": snapshot_label,
        "git": {
            "sha": _cmd_output(["git", "rev-parse", "HEAD"]),
            "short_sha": _cmd_output(["git", "rev-parse", "--short", "HEAD"]),
            "branch": _cmd_output(["git", "branch", "--show-current"]),
            "tag": _cmd_output(["git", "describe", "--tags", "--exact-match"]),
            "describe": _cmd_output(["git", "describe", "--tags", "--always", "--dirty"]),
            "dirty": _cmd_output(["git", "status", "--porcelain"]) not in (None, ""),
        },
        "tailtriage": get_tailtriage_versions(repo_root),
        "github_actions": {
            "enabled": os.getenv("GITHUB_ACTIONS") == "true",
            "workflow": os.getenv("GITHUB_WORKFLOW"),
            "run_id": os.getenv("GITHUB_RUN_ID"),
            "run_attempt": os.getenv("GITHUB_RUN_ATTEMPT"),
            "event_name": os.getenv("GITHUB_EVENT_NAME"),
            "ref": os.getenv("GITHUB_REF"),
            "sha": os.getenv("GITHUB_SHA"),
            "runner_os": os.getenv("RUNNER_OS"),
            "runner_arch": os.getenv("RUNNER_ARCH"),
            "runner_name": os.getenv("RUNNER_NAME"),
            "image_os": os.getenv("ImageOS"),
            "image_version": os.getenv("ImageVersion"),
        },
        "software": {
            "python": platform.python_version(),
            "rustc": _cmd_output(["rustc", "--version"]),
            "cargo": _cmd_output(["cargo", "--version"]),
            "platform": platform.platform(),
            "kernel": platform.release(),
            "os_release": _read_text(Path("/etc/os-release")),
        },
        "hardware": {
            "machine": platform.machine(),
            "processor": platform.processor() or None,
            "cpu_model": _linux_cpu_model(),
            "logical_cores": os.cpu_count(),
            "memory_total_kib": _linux_mem_kib(),
        },
        "inputs": {
            "manifest": str(manifest_path.relative_to(repo_root)),
            "manifest_sha256": manifest_sha,
            "referenced_artifacts_sha256": artifacts_sha,
            "thresholds": thresholds,
        },
    }


def render_failed_cases(failed_cases):
    if not failed_cases:
        return "None\n"
    lines = ["| id | diagnostic |", "|---|---|"]
    for case in failed_cases:
        lines.append(f"| {case.get('id', case.get('observation_id'))} | {case.get('error', 'case contract failed')} |")
    return "\n".join(lines) + "\n"


def render_scorecard(metrics, env):
    execution=metrics["analyzer_execution"];accuracy=metrics["analyzer_accuracy"];contracts=metrics["report_contract"]
    fmt=lambda v: "n/a" if v is None else v
    parts = ["# Diagnostic validation scorecard\n", "## Snapshot\n", f"- Generated at (UTC): {env['generated_at_utc']}", f"- Snapshot label: {env.get('snapshot_label')}", f"- Git SHA: {env['git'].get('sha')}", f"- Git tag: {env['git'].get('tag')}", f"- Git describe: {env['git'].get('describe')}\n", "## Environment\n", f"- tailtriage workspace package version: {env['tailtriage'].get('workspace_package_version')}"]
    for k, v in env["tailtriage"]["packages"].items():
        parts.append(f"- {k}: {v}")
    parts.extend([f"- GitHub run: {env['github_actions'].get('run_id')} ({env['github_actions'].get('ref')})", f"- Runner: {env['github_actions'].get('runner_os')} {env['github_actions'].get('runner_arch')} / {env['github_actions'].get('image_version')}", f"- Python: {env['software'].get('python')}", f"- rustc: {env['software'].get('rustc')}", f"- cargo: {env['software'].get('cargo')}", f"- CPU model: {env['hardware'].get('cpu_model')}", f"- Logical cores: {env['hardware'].get('logical_cores')}", f"- Memory KiB: {env['hardware'].get('memory_total_kib')}\n", "## Inputs\n", f"- Manifest cases: {metrics['manifest_case_count']}", f"- Manifest SHA256: {env['inputs']['manifest_sha256']}", f"- Referenced artifacts SHA256: {env['inputs']['referenced_artifacts_sha256']}", f"- Thresholds: {json.dumps(env['inputs']['thresholds'], sort_keys=True)}\n", "## Analyzer execution\n", f"- Cases: {execution['case_count']}",f"- Successful analyzer executions: {execution['success_count']}",f"- Expected failures: {execution['expected_failure_count']}",f"- Unexpected failures: {execution['unexpected_failure_count']}\n","## Analyzer accuracy\n",f"- Unique accuracy observations: {accuracy['observation_count']}",f"- Executed artifact encodings: {accuracy['encoding_count']}",f"- Top-1 accuracy: {fmt(accuracy['top1_accuracy'])}",f"- Top-2 recall: {fmt(accuracy['top2_recall'])}",f"- High-confidence wrong: {accuracy['high_confidence_wrong_count']}\n","### Per-ground-truth observation counts\n"])
    for k, v in sorted(accuracy.get("per_ground_truth_counts", {}).items()):
        parts.append(f"- {k}: {v}")
    parts.append("\n### Confidence bucket accuracy\n")
    for k, v in sorted(accuracy.get("confidence_bucket_accuracy", {}).items()):
        parts.append(f"- {k}: accuracy={v.get('accuracy')} total={v.get('total')} correct={v.get('correct')}")
    parts.extend(["\n## Report-contract validation\n",f"- Cases: {contracts['case_count']}",f"- Passed: {contracts['passed_count']}",f"- Failed: {contracts['failed_count']}","- Report-contract cases do not contribute to analyzer accuracy.\n","## Validated input paths\n"])
    validated_path_labels = {
        "analysis_report": "analysis_report fixtures",
        "synthetic_analysis_report": "synthetic_analysis_report fixtures",
        "run_artifact": "tailtriage-cli analyze run_artifact",
        "tracing_span_jsonl": "tailtriage-cli import tracing-spans-jsonl -> analyze",
    }
    for k, label in validated_path_labels.items():
        parts.append(f"- {label}: {metrics.get('validated_paths', {}).get(k, 0)}")
    parts.append("\n## Failed analyzer cases\n")
    parts.append(render_failed_cases(metrics.get("failed_analyzer_cases", [])))
    parts.append("## Failed report-contract cases\n")
    parts.append(render_failed_cases(metrics.get("failed_report_contract_cases", [])))
    parts.append("## Non-claims\n")
    parts.extend(["- This is not root-cause proof.", "- This is not universal production accuracy.", "- This is not universal production overhead.", "- This is not real-service validation.", "- Fixture labels are controlled intent, not production truth.","- Report-contract pass rates are not analyzer accuracy.","- Equivalent artifact encodings are not independent observations.","- The native/tracing equivalence contract is not an accuracy denominator."])
    return "\n".join(parts) + "\n"


def generate_scorecard(repo_root: Path, manifest_rel: str, out_dir_rel: str, min_top1: float, min_top2: float, max_high_confidence_wrong: int, snapshot_label):
    manifest_path = (repo_root / manifest_rel).resolve()
    out_dir = (repo_root / out_dir_rel).resolve()
    out_dir.mkdir(parents=True, exist_ok=True)
    metrics, failures = run_diagnostic_benchmark(manifest_path, min_top1, min_top2, max_high_confidence_wrong)
    thresholds = {"min_top1": min_top1, "min_top2": min_top2, "max_high_confidence_wrong": max_high_confidence_wrong}
    environment = collect_environment(repo_root, manifest_path, snapshot_label, thresholds)
    (out_dir / "benchmark-summary.json").write_text(json.dumps(metrics, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "environment.json").write_text(json.dumps(environment, indent=2, sort_keys=True) + "\n", encoding="utf-8")
    (out_dir / "scorecard.md").write_text(render_scorecard(metrics, environment), encoding="utf-8")
    return failures


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", default="validation/diagnostics/manifest.json")
    ap.add_argument("--out-dir", default="target/validation/diagnostics")
    ap.add_argument("--min-top1", type=float, default=0.75)
    ap.add_argument("--min-top2", type=float, default=0.90)
    ap.add_argument("--max-high-confidence-wrong", type=int, default=0)
    ap.add_argument("--snapshot-label")
    args = ap.parse_args()
    failures = generate_scorecard(REPO_ROOT, args.manifest, args.out_dir, args.min_top1, args.min_top2, args.max_high_confidence_wrong, args.snapshot_label)
    if failures:
        for failure in failures:
            print(f"FAIL: {failure}")
        raise SystemExit(1)


if __name__ == "__main__":
    main()
