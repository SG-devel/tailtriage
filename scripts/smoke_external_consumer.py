#!/usr/bin/env python3
"""Smoke-test external crates.io adoption outside the workspace.

This script creates a temporary Cargo app outside the repository, consumes
`tailtriage-core` from crates.io, produces a run artifact, and analyzes it
with the workspace CLI.
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


def write_external_app(project_dir: Path) -> tuple[Path, Path]:
    app_dir = project_dir / "external-tailtriage-smoke"
    run_cmd(["cargo", "new", "--bin", app_dir.name], cwd=project_dir)

    cargo_toml = f"""[package]
name = "external-tailtriage-smoke"
version = "0.1.1"
edition = "2021"

[dependencies]
tailtriage-core = "0.1.1"
tokio = {{ version = "1", features = ["macros", "rt", "time"] }}
"""
    (app_dir / "Cargo.toml").write_text(cargo_toml, encoding="utf-8")

    main_rs = """use std::time::Duration;

use tailtriage_core::{RequestOptions, Tailtriage};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("external-consumer-smoke")
        .output("tailtriage-run.json")
        .build()?;

    let started = tailtriage.begin_request_with(
        "/smoke",
        RequestOptions::new()
            .request_id("external-req-1")
            .kind("cli"),
    );
    let request = started.handle.clone();

    request
        .queue("ingress")
        .with_depth_at_start(1)
        .await_on(tokio::time::sleep(Duration::from_millis(2)))
        .await;

    request
        .stage("compute")
        .await_value(tokio::time::sleep(Duration::from_millis(3)))
        .await;

    started.completion.finish_ok();
    tailtriage.shutdown()?;
    Ok(())
}
"""
    (app_dir / "src/main.rs").write_text(main_rs, encoding="utf-8")

    return app_dir, app_dir / "tailtriage-run.json"


def main() -> None:
    root = repo_root()
    print("Smoke-validating external crates.io consumer adoption outside the workspace...")

    with tempfile.TemporaryDirectory(prefix="tailtriage-external-consumer-") as temp_dir:
        external_root = Path(temp_dir)
        app_dir, artifact_path = write_external_app(external_root)

        print(f"==> created external app: {app_dir}")
        run_cmd(["cargo", "run", "--quiet"], cwd=app_dir)

        if not artifact_path.exists():
            raise SystemExit(
                "external app did not create expected artifact: "
                f"{artifact_path}"
            )

        run_payload = json.loads(artifact_path.read_text(encoding="utf-8"))
        if not isinstance(run_payload, dict):
            raise SystemExit("external app artifact is not a JSON object")

        assert_keys(
            run_payload,
            EXPECTED_RUN_TOP_LEVEL_KEYS,
            context="external app run artifact",
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
            raise SystemExit("analysis output is not a JSON object")

        assert_keys(
            analysis_payload,
            EXPECTED_ANALYSIS_TOP_LEVEL_KEYS,
            context="external app analysis report",
        )

        print("External consumer smoke test passed.")
        print(f"  artifact: {artifact_path}")


if __name__ == "__main__":
    main()
