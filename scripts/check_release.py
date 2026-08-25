#!/usr/bin/env python3
"""Check whether a commit is ready for a manual release."""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import sys
from pathlib import Path
from typing import Any

EXAMPLE_BEARING_PACKAGES = {
    "tailtriage-controller",
    "tailtriage-tokio",
    "tailtriage-axum",
}


def command(argv: list[str]) -> subprocess.CompletedProcess[str]:
    return subprocess.run(argv, capture_output=True, text=True)


def failed_command(argv: list[str], result: subprocess.CompletedProcess[str]) -> str:
    details = [f"command {argv!r} exited with {result.returncode}"]
    if result.stdout:
        details.append(f"stdout:\n{result.stdout.rstrip()}")
    if result.stderr:
        details.append(f"stderr:\n{result.stderr.rstrip()}")
    return "\n".join(details)


def is_publishable(package: dict[str, Any]) -> bool:
    allowed = package.get("publish")
    return allowed is None or (isinstance(allowed, list) and "crates-io" in allowed)


def version_tuple(version: str) -> tuple[int, int, int] | None:
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)(?:[-+].*)?", version)
    return tuple(map(int, match.groups())) if match else None  # type: ignore[return-value]


def requirement_allows(requirement: str, version: str) -> bool:
    """Cover the Cargo requirement forms used for workspace path dependencies."""
    wanted = version_tuple(version)
    if wanted is None:
        return False
    for alternative in requirement.split("||"):
        clauses = [part.strip() for part in alternative.split(",")]
        permitted = True
        for clause in clauses:
            match = re.fullmatch(r"(\^|~|=|>=|<=|>|<)?\s*(\d+)(?:\.(\d+|\*))?(?:\.(\d+|\*))?", clause)
            if not match:
                permitted = False
                break
            operator, major, minor, patch = match.groups()
            parts = (int(major), int(minor or 0) if minor != "*" else 0, int(patch or 0) if patch != "*" else 0)
            if minor == "*" or patch == "*" or (operator is None and (minor is None or patch is None)):
                specified = 1 if minor in {None, "*"} else 2
                permitted &= wanted[:specified] == parts[:specified]
            elif operator in {None, "^"}:
                upper = (parts[0] + 1, 0, 0) if parts[0] else ((0, parts[1] + 1, 0) if parts[1] else (0, 0, parts[2] + 1))
                permitted &= parts <= wanted < upper
            elif operator == "~":
                permitted &= parts <= wanted < (parts[0], parts[1] + 1, 0)
            else:
                permitted &= {"=": wanted == parts, ">=": wanted >= parts, "<=": wanted <= parts, ">": wanted > parts, "<": wanted < parts}[operator]
        if permitted:
            return True
    return False


def publication_order(packages: list[dict[str, Any]], version: str) -> tuple[list[str], list[str]]:
    names = {package["name"] for package in packages}
    dependencies: dict[str, set[str]] = {name: set() for name in names}
    errors: list[str] = []
    for package in packages:
        for dependency in package.get("dependencies", []):
            dependency_name = dependency["name"]  # Cargo's package name, not a local rename.
            if dependency_name not in names or dependency.get("kind") == "dev":
                continue
            dependencies[package["name"]].add(dependency_name)
            if not requirement_allows(dependency["req"], version):
                errors.append(
                    f"{package['name']} requires internal package {dependency_name} as "
                    f"{dependency['req']!r}, which does not allow {version}"
                )

    order: list[str] = []
    remaining = {name: set(required) for name, required in dependencies.items()}
    while remaining:
        ready = sorted(name for name, required in remaining.items() if not required)
        if not ready:
            errors.append("publishable package dependency cycle detected: " + ", ".join(sorted(remaining)))
            break
        for name in ready:
            order.append(name)
            del remaining[name]
        for required in remaining.values():
            required.difference_update(ready)
    return order, errors


def changelog_errors(version: str, text: str) -> list[str]:
    heading = re.search(rf"^##\s+\[?{re.escape(version)}\]?\s+-\s+(.+)$", text, re.MULTILINE)
    if not heading:
        return [f"CHANGELOG.md has no section for {version}"]
    date = heading.group(1).strip()
    errors = []
    if date.lower() == "unreleased":
        errors.append(f"CHANGELOG.md section for {version} is still Unreleased")
    if not re.fullmatch(r"\d{4}-\d{2}-\d{2}", date):
        errors.append(f"CHANGELOG.md heading for {version} needs a concrete YYYY-MM-DD date")
    body_start = heading.end()
    next_heading = re.search(r"^##\s+", text[body_start:], re.MULTILINE)
    body = text[body_start : body_start + next_heading.start() if next_heading else None]
    if not body.strip():
        errors.append(f"CHANGELOG.md section for {version} is empty")
    return errors


def packaged_example_errors(package_names: set[str]) -> list[str]:
    """Check actual Cargo package listings for the public example boundary."""
    errors = []
    for name in sorted(EXAMPLE_BEARING_PACKAGES & package_names):
        argv = ["cargo", "package", "--locked", "-p", name, "--list"]
        result = command(argv)
        if result.returncode != 0:
            errors.append(failed_command(argv, result))
            continue
        if not any(line.strip().startswith("examples/") for line in result.stdout.splitlines()):
            errors.append(f"Cargo package {name} contains no examples/** entries")
    return errors


def check(version: str, changelog: Path = Path("CHANGELOG.md")) -> int:
    errors: list[str] = []
    status_command = ["git", "status", "--porcelain"]
    status = command(status_command)
    if status.returncode != 0:
        errors.append("could not inspect worktree cleanliness:\n" + failed_command(status_command, status))
    elif status.stdout.strip():
        errors.append("worktree is not clean:\n" + status.stdout.rstrip())

    head_command = ["git", "rev-parse", "HEAD"]
    head_result = command(head_command)
    head = head_result.stdout.strip()
    if head_result.returncode != 0:
        errors.append("could not determine HEAD:\n" + failed_command(head_command, head_result))
    elif not head:
        errors.append("could not determine HEAD")
    else:
        print(f"HEAD: {head}")

    metadata_command = ["cargo", "metadata", "--format-version", "1", "--locked"]
    metadata_result = command(metadata_command)
    metadata: dict[str, Any] = {}
    if metadata_result.returncode != 0:
        errors.append("cargo metadata failed:\n" + failed_command(metadata_command, metadata_result))
    else:
        try:
            metadata = json.loads(metadata_result.stdout)
        except json.JSONDecodeError as exc:
            errors.append(f"cargo metadata returned invalid JSON: {exc}")

    workspace_ids = set(metadata.get("workspace_members", []))
    workspace = [package for package in metadata.get("packages", []) if package["id"] in workspace_ids]
    publishable = [package for package in workspace if is_publishable(package)]
    non_publishable = sorted(package["name"] for package in workspace if not is_publishable(package))
    if metadata and not publishable:
        errors.append("workspace has no publishable packages")
    for package in publishable:
        if package["version"] != version:
            errors.append(f"{package['name']} has version {package['version']}, expected {version}")
    order, graph_errors = publication_order(publishable, version)
    errors.extend(graph_errors)
    try:
        errors.extend(changelog_errors(version, changelog.read_text(encoding="utf-8")))
    except OSError as exc:
        errors.append(f"could not read CHANGELOG.md: {exc}")

    if errors:
        print("Release preflight failed:", file=sys.stderr)
        for error in errors:
            print(f"- {error}", file=sys.stderr)
        return 1


    example_errors = packaged_example_errors({package["name"] for package in publishable})
    if example_errors:
        print("Release preflight failed:", file=sys.stderr)
        for error in example_errors:
            print(f"- {error}", file=sys.stderr)
        return 1

    package_command = ["cargo", "package", "--locked"]
    for name in order:
        package_command.extend(["-p", name])
    packaged = command(package_command)
    if packaged.returncode != 0:
        print("Release preflight failed: " + failed_command(package_command, packaged), file=sys.stderr)
        return 1

    print(f"Requested version: {version}")
    print("Publishable packages: " + ", ".join(sorted(package["name"] for package in publishable)))
    print("Non-publishable workspace packages: " + (", ".join(non_publishable) or "none"))
    print("Publication order: " + " -> ".join(order))
    print("Cargo packaging: succeeded")
    print("Manual publication instructions (do not run automatically):")
    for name in order:
        print(f"cargo publish --locked -p {name}")
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--version", required=True)
    return check(parser.parse_args().version)


if __name__ == "__main__":
    raise SystemExit(main())
