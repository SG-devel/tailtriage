#!/usr/bin/env python3
"""Enforce immutable remote GitHub Action references and cargo-deny policy."""

from __future__ import annotations

import re
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS_DIR = REPO_ROOT / ".github" / "workflows"
CI_WORKFLOW = WORKFLOWS_DIR / "ci.yml"
USES_RE = re.compile(r"^\s*(?:-\s*)?uses:\s*([^\s#]+)")
SHA_RE = re.compile(r"[0-9a-fA-F]{40}")


def workflow_policy_errors(text: str, *, name: str = "workflow") -> list[str]:
    """Return immutable-reference policy failures found in one workflow."""
    errors: list[str] = []
    for line_number, line in enumerate(text.splitlines(), start=1):
        match = USES_RE.match(line)
        if match is None:
            continue
        action = match.group(1).strip("'\"")
        if action.startswith("./"):
            continue
        owner, separator, ref = action.rpartition("@")
        if not separator or not owner or SHA_RE.fullmatch(ref) is None:
            errors.append(
                f"{name}:{line_number}: remote action must use a full 40-hex commit SHA: {action}"
            )
    return errors


def cargo_deny_policy_errors(text: str, *, name: str = "ci.yml") -> list[str]:
    """Return failures in the repository's exact cargo-deny installer policy."""
    errors: list[str] = []
    tool_values = re.findall(r"(?m)^\s*tool:\s*([^\s#]+)", text)
    deny_values = [value.strip("'\"") for value in tool_values if value.startswith("cargo-deny")]
    if deny_values != ["cargo-deny@0.20.2"]:
        errors.append(f"{name}: cargo-deny tool must be exactly cargo-deny@0.20.2")

    fallback_values = re.findall(r"(?m)^\s*fallback:\s*([^\s#]+)", text)
    if [value.strip("'\"") for value in fallback_values] != ["none"]:
        errors.append(f"{name}: cargo-deny installer fallback must be exactly none")
    return errors


def ci_source_policy_errors(text: str, *, name: str = "ci.yml") -> list[str]:
    """Check narrow, repository-specific CI trust-boundary source contracts."""
    errors: list[str] = []
    if not re.search(r"(?m)^\s{2}workflow_dispatch:\s*(?:\{\s*\})?\s*$", text):
        errors.append(f"{name}: workflow_dispatch must be present and input-free")
    if re.search(r"(?m)^\s{4}inputs:\s*", text):
        errors.append(f"{name}: workflow_dispatch must not define inputs")
    if not re.search(r"(?m)^permissions:\s*\n\s{2}contents:\s*read\s*$", text):
        errors.append(f"{name}: workflow permissions must be exactly contents: read")
    if not re.search(r"(?m)^\s+toolchain:\s*[^\s#]+", text):
        errors.append(f"{name}: pinned Rust setup must select an explicit toolchain")
    return errors


def check_repository(
    *, workflows_dir: Path = WORKFLOWS_DIR, ci_workflow: Path = CI_WORKFLOW
) -> list[str]:
    """Return all workflow security policy failures for a repository checkout."""
    errors: list[str] = []
    workflows = sorted((*workflows_dir.glob("*.yml"), *workflows_dir.glob("*.yaml")))
    if [workflow.name for workflow in workflows] != ["ci.yml"]:
        errors.append("workflow inventory must contain only ci.yml")
    for workflow in workflows:
        errors.extend(workflow_policy_errors(workflow.read_text(encoding="utf-8"), name=str(workflow)))
    errors.extend(
        cargo_deny_policy_errors(ci_workflow.read_text(encoding="utf-8"), name=str(ci_workflow))
    )
    errors.extend(ci_source_policy_errors(ci_workflow.read_text(encoding="utf-8"), name=str(ci_workflow)))
    return errors


def main() -> int:
    errors = check_repository()
    if errors:
        print("workflow security policy failed:")
        for error in errors:
            print(f"- {error}")
        return 1
    print("workflow security policy passed")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
