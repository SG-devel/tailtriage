#!/usr/bin/env python3
"""Prove Cargo package material works in an offline outside-workspace consumer."""

from __future__ import annotations

import argparse
import json
import subprocess
import tarfile
import tempfile
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
PRODUCT_PACKAGES = (
    "tailtriage",
    "tailtriage-core",
    "tailtriage-controller",
    "tailtriage-tokio",
    "tailtriage-axum",
    "tailtriage-tracing",
    "tailtriage-analyzer",
    "tailtriage-cli",
)
PATCH_PACKAGES = PRODUCT_PACKAGES[1:6]


def run(argv: list[str], *, cwd: Path = REPO_ROOT) -> None:
    subprocess.run(argv, cwd=cwd, check=True)


def cargo_metadata() -> dict[str, Any]:
    result = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--locked"],
        cwd=REPO_ROOT,
        check=True,
        capture_output=True,
        text=True,
    )
    return json.loads(result.stdout)


def product_package_versions(metadata: dict[str, Any]) -> dict[str, str]:
    workspace_ids = set(metadata.get("workspace_members", []))
    versions: dict[str, str] = {}
    duplicates: set[str] = set()
    for package in metadata.get("packages", []):
        name = package.get("name")
        if package.get("id") not in workspace_ids or name not in PRODUCT_PACKAGES:
            continue
        if name in versions:
            duplicates.add(name)
        versions[name] = package["version"]
    missing = [name for name in PRODUCT_PACKAGES if name not in versions]
    if missing or duplicates:
        details = []
        if missing:
            details.append("missing product packages: " + ", ".join(missing))
        if duplicates:
            details.append("duplicated product packages: " + ", ".join(sorted(duplicates)))
        raise ValueError("; ".join(details))
    return {name: versions[name] for name in PRODUCT_PACKAGES}


def package_command(allow_dirty: bool = False) -> list[str]:
    argv = ["cargo", "package", "--locked", "--offline", "--no-verify"]
    if allow_dirty:
        argv.append("--allow-dirty")
    for name in PRODUCT_PACKAGES:
        argv.extend(["-p", name])
    return argv


def consumer_manifest(extracted: dict[str, Path]) -> str:
    def quoted_path(name: str) -> str:
        return json.dumps(str(extracted[name].resolve()))

    patches = "\n".join(f"{name} = {{ path = {quoted_path(name)} }}" for name in PATCH_PACKAGES)
    return f'''[package]
name = "tailtriage-package-consumer"
version = "0.0.0"
edition = "2021"
publish = false

[dependencies]
tailtriage = {{ path = {quoted_path("tailtriage")}, default-features = false, features = ["full"] }}
tailtriage-analyzer = {{ path = {quoted_path("tailtriage-analyzer")} }}

[patch.crates-io]
{patches}
'''


def consumer_source() -> str:
    return '''use tailtriage::Tailtriage;
use tailtriage_analyzer::AnalyzeOptions;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let capture = Tailtriage::builder("package-consumer").build()?;
    let started = capture.begin_request("/package-consumer");
    started.completion.finish_ok();

    let _options = AnalyzeOptions::default();

    capture.shutdown()?;
    Ok(())
}
'''


def extract_archive(archive: Path, root: Path, expected_directory: Path) -> None:
    resolved_root = root.resolve()
    with tarfile.open(archive, "r:gz") as package:
        for member in package.getmembers():
            destination = (root / member.name).resolve()
            if destination != resolved_root and resolved_root not in destination.parents:
                raise RuntimeError(f"archive member escapes extraction root: {member.name}")
        package.extractall(root, filter="data")
    if not expected_directory.is_dir():
        raise RuntimeError(f"archive did not create expected directory: {expected_directory.name}")


def check(allow_dirty: bool) -> None:
    metadata = cargo_metadata()
    versions = product_package_versions(metadata)
    package_root = Path(metadata["target_directory"]) / "package"
    archives = {
        name: package_root / f"{name}-{versions[name]}.crate" for name in PRODUCT_PACKAGES
    }
    for archive in archives.values():
        archive.unlink(missing_ok=True)
    run(package_command(allow_dirty))
    missing = [str(path) for path in archives.values() if not path.is_file()]
    if missing:
        raise RuntimeError("Cargo did not generate expected archives: " + ", ".join(missing))

    with tempfile.TemporaryDirectory(prefix="tailtriage-package-consumer-") as temporary:
        proof_root = Path(temporary).resolve()
        repository = REPO_ROOT.resolve()
        if proof_root == repository or repository in proof_root.parents:
            raise RuntimeError("temporary proof root is inside the repository checkout")

        extracted = {
            name: proof_root / f"{name}-{versions[name]}" for name in PRODUCT_PACKAGES
        }
        for name in PRODUCT_PACKAGES:
            extract_archive(archives[name], proof_root, extracted[name])

        consumer = proof_root / "consumer"
        (consumer / "src").mkdir(parents=True)
        (consumer / "Cargo.toml").write_text(consumer_manifest(extracted), encoding="utf-8")
        (consumer / "src" / "main.rs").write_text(consumer_source(), encoding="utf-8")
        run(["cargo", "generate-lockfile", "--offline"], cwd=consumer)
        run(["cargo", "run", "--locked", "--offline"], cwd=consumer)

        print("Product packages: " + ", ".join(f"{name} {versions[name]}" for name in PRODUCT_PACKAGES))
        print("Cargo archives: generated all eight fresh .crate files")
        print("Consumer root: temporary and outside the repository workspace")
        print("Consumer: lock generation and execution succeeded offline")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--allow-dirty", action="store_true")
    args = parser.parse_args()
    try:
        check(args.allow_dirty)
    except (OSError, ValueError, RuntimeError, subprocess.CalledProcessError, json.JSONDecodeError) as exc:
        print(f"package consumer proof failed: {exc}")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
