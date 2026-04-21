#!/usr/bin/env python3
"""Detect published workspace version changes across CI event boundaries.

This script compares root Cargo.toml `workspace.package.version` between a base
ref and head ref for pull_request/push events and writes
`published_version_changed=true|false` to `GITHUB_OUTPUT`.
"""

from __future__ import annotations

import os
import subprocess
import tomllib


def read_workspace_version(ref: str) -> str:
    cargo_toml = subprocess.check_output(
        ["git", "show", f"{ref}:Cargo.toml"]
    ).decode("utf-8")
    parsed = tomllib.loads(cargo_toml)
    return parsed["workspace"]["package"]["version"]


def base_ref_for_event(event_name: str) -> str:
    if event_name == "pull_request":
        return os.environ.get("GITHUB_BASE_SHA", "")
    if event_name == "push":
        return os.environ.get("GITHUB_EVENT_BEFORE", "")
    return ""


def write_output(value: str) -> None:
    with open(os.environ["GITHUB_OUTPUT"], "a", encoding="utf-8") as output:
        output.write(f"published_version_changed={value}\n")


def main() -> None:
    event_name = os.environ["GITHUB_EVENT_NAME"]
    head_ref = os.environ["GITHUB_SHA"]
    base_ref = base_ref_for_event(event_name)

    changed = True
    if base_ref and set(base_ref) != {"0"}:
        try:
            old_version = read_workspace_version(base_ref)
            new_version = read_workspace_version(head_ref)
            changed = old_version != new_version
            print(
                "published workspace version comparison:",
                f"base={base_ref} ({old_version})",
                f"head={head_ref} ({new_version})",
            )
        except Exception as exc:
            changed = True
            print(
                "failed to read workspace version history; "
                "failing safe with published_version_changed=true:",
                exc,
            )
    else:
        print(
            "missing or zeroed base ref; "
            "failing safe with published_version_changed=true"
        )

    output_value = "true" if changed else "false"
    write_output(output_value)
    print(f"published_version_changed={output_value}")


if __name__ == "__main__":
    main()
