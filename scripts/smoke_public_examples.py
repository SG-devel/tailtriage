#!/usr/bin/env python3
"""Smoke-validate launch-critical public examples.

This script validates the public onboarding flow for selected examples:
1) run the example
2) confirm it writes a run artifact
3) confirm the artifact has the expected top-level schema shape
4) confirm `tailtriage-cli analyze ... --format json` succeeds
"""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

EXAMPLES = [
    "minimal_checkout",
    "axum_minimal",
    "mini_service_integration",
]

EXPECTED_RUN_TOP_LEVEL_KEYS = {
    "schema_version",
    "metadata",
    "requests",
    "stages",
    "queues",
    "inflight",
    "runtime_snapshots",
    "truncation",
}

EXPECTED_ANALYSIS_TOP_LEVEL_KEYS = {
    "request_count",
    "p95_latency_us",
    "primary_suspect",
    "secondary_suspects",
    "warnings",
}


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def run_cmd(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        check=True,
        text=True,
        capture_output=True,
    )


def assert_keys(payload: dict, expected: set[str], *, context: str) -> None:
    missing = sorted(expected - set(payload.keys()))
    if missing:
        missing_list = ", ".join(missing)
        raise SystemExit(f"{context} missing top-level keys: {missing_list}")


def validate_example(name: str) -> None:
    root = repo_root()
    print(f"==> validating example: {name}")

    with tempfile.TemporaryDirectory(prefix=f"tailtriage-example-smoke-{name}-") as temp_dir:
        working_dir = Path(temp_dir)
        artifact_path = working_dir / "tailtriage-run.json"

        run_cmd(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(root / "tailtriage-tokio/Cargo.toml"),
                "--example",
                name,
            ],
            cwd=working_dir,
        )

        if not artifact_path.exists():
            raise SystemExit(
                f"example '{name}' did not create expected artifact: {artifact_path}"
            )

        run_payload = json.loads(artifact_path.read_text(encoding="utf-8"))
        if not isinstance(run_payload, dict):
            raise SystemExit(f"example '{name}' artifact is not a JSON object")

        assert_keys(
            run_payload,
            EXPECTED_RUN_TOP_LEVEL_KEYS,
            context=f"example '{name}' run artifact",
        )

        analysis = run_cmd(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(root / "tailtriage-cli/Cargo.toml"),
                "--",
                "analyze",
                str(artifact_path),
                "--format",
                "json",
            ],
            cwd=root,
        )
        analysis_payload = json.loads(analysis.stdout)
        if not isinstance(analysis_payload, dict):
            raise SystemExit(f"example '{name}' analysis output is not a JSON object")

        assert_keys(
            analysis_payload,
            EXPECTED_ANALYSIS_TOP_LEVEL_KEYS,
            context=f"example '{name}' analysis report",
        )

        print(f"validated: {name}")
        print(f"  artifact: {artifact_path}")


def main() -> None:
    print("Smoke-validating launch-critical public examples...")
    for name in EXAMPLES:
        validate_example(name)
    print("All launch-critical public examples passed smoke validation.")


if __name__ == "__main__":
    main()
