#!/usr/bin/env python3
"""Smoke-check the controller example contract.

Validation steps:
1) run the repository/workspace controller example in release mode
2) verify artifact exists
3) verify artifact has expected top-level schema keys
4) verify artifact recorded exactly one request
5) verify packaged crate contents do not include repository examples
"""

from __future__ import annotations

import json
import subprocess
import tempfile
from pathlib import Path

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


def assert_packaged_contract(root: Path) -> None:
    package_listing = run_cmd(
        [
            "cargo",
            "package",
            "--allow-dirty",
            "--manifest-path",
            str(root / "tailtriage-controller/Cargo.toml"),
            "--list",
        ],
        cwd=root,
    )
    packaged_paths = [line.strip() for line in package_listing.stdout.splitlines() if line.strip()]
    example_paths = [path for path in packaged_paths if path.startswith("examples/")]
    if example_paths:
        rendered = ", ".join(sorted(example_paths))
        raise SystemExit(
            "tailtriage-controller packaged crate unexpectedly includes examples: "
            f"{rendered}"
        )


def main() -> None:
    root = repo_root()
    print("Smoke-checking controller example contract...")

    with tempfile.TemporaryDirectory(prefix="tailtriage-controller-example-smoke-") as temp_dir:
        working_dir = Path(temp_dir)

        run_cmd(
            [
                "cargo",
                "run",
                "--quiet",
                "--release",
                "--manifest-path",
                str(root / "tailtriage-controller/Cargo.toml"),
                "--example",
                "controller_minimal",
            ],
            cwd=working_dir,
        )

        artifacts = sorted(working_dir.glob("tailtriage-run-generation-*.json"))

        if len(artifacts) != 1:
            raise SystemExit(
                "controller example should create exactly one generation artifact, "
                f"found {len(artifacts)}"
            )

        artifact_path = artifacts[0]
        run_payload = json.loads(artifact_path.read_text(encoding="utf-8"))
        if not isinstance(run_payload, dict):
            raise SystemExit("controller example artifact is not a JSON object")

        assert_keys(
            run_payload,
            EXPECTED_RUN_TOP_LEVEL_KEYS,
            context="controller example run artifact",
        )

        requests = run_payload.get("requests")
        if not isinstance(requests, list):
            raise SystemExit("controller example artifact 'requests' field is not an array")
        if len(requests) != 1:
            raise SystemExit(
                "controller example should emit exactly one request, "
                f"found {len(requests)}"
            )

        assert_packaged_contract(root)

        print("validated: tailtriage-controller::controller_minimal")
        print(f"  artifact: {artifact_path}")
        print("validated: packaged crate excludes repository/workspace examples")


if __name__ == "__main__":
    main()
