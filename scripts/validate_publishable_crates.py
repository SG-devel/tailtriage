#!/usr/bin/env python3
"""Validate publishable crate package surfaces.

This script performs two checks for each publishable crate:
1) `cargo package --list` to show what would be packaged.
2) `cargo publish --dry-run` to validate publishability without publishing.

If any crate cannot pass dry-run publication (for example because a dependency is
not yet available on crates.io), the script exits non-zero with a summary.
"""

from __future__ import annotations

import subprocess
from dataclasses import dataclass
from pathlib import Path

PUBLISHABLE_CRATES = [
    "tailtriage-core",
    "tailtriage-tokio",
    "tailtriage-cli",
]


@dataclass
class CommandResult:
    command: list[str]
    success: bool
    stderr: str


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def run_checked(cmd: list[str], cwd: Path) -> CommandResult:
    printable_cmd = " ".join(cmd)
    print(f"\n$ {printable_cmd}")
    completed = subprocess.run(
        cmd,
        cwd=cwd,
        text=True,
        capture_output=True,
    )
    if completed.stdout:
        print(completed.stdout, end="")
    if completed.stderr:
        print(completed.stderr, end="")
    return CommandResult(
        command=cmd,
        success=completed.returncode == 0,
        stderr=completed.stderr,
    )


def validate_crate(crate: str, root: Path) -> list[CommandResult]:
    print(f"\n==> validating publish package surface for {crate}")
    results: list[CommandResult] = []
    results.append(run_checked(["cargo", "package", "--list", "--locked", "-p", crate], root))
    results.append(run_checked(["cargo", "publish", "--dry-run", "--locked", "-p", crate], root))
    return results


def main() -> None:
    root = repo_root()
    print("Validating package contents and publish dry-run for all publishable crates...")

    failures: list[CommandResult] = []
    for crate in PUBLISHABLE_CRATES:
        for result in validate_crate(crate, root):
            if not result.success:
                failures.append(result)

    if failures:
        print("\nPackage validation failures detected:")
        for failure in failures:
            print(f"- {' '.join(failure.command)}")
            if failure.stderr:
                first_line = failure.stderr.strip().splitlines()[-1]
                print(f"  last error line: {first_line}")
        raise SystemExit(1)

    print("\nAll publishable crate package validations passed.")


if __name__ == "__main__":
    main()
