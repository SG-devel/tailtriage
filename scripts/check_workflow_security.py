#!/usr/bin/env python3
"""Enforce the checked-in GitHub Actions workflow source policy."""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from pathlib import Path

REPO_ROOT = Path(__file__).resolve().parent.parent
WORKFLOWS_DIR = REPO_ROOT / ".github" / "workflows"
CI_WORKFLOW = WORKFLOWS_DIR / "ci.yml"
USES_RE = re.compile(r"^\s*(?:-\s*)?uses:\s*([^\s#]+)")
SHA_RE = re.compile(r"[0-9a-fA-F]{40}")


def _scalar(line: str) -> str:
    return line.split("#", 1)[0].strip().strip("'\"")


@dataclass
class ActionStep:
    uses: str | None = None
    with_values: dict[str, str] = field(default_factory=dict)


def action_steps(text: str) -> list[ActionStep]:
    """Read action steps and their own immediate scalar ``with`` values."""
    lines = text.splitlines()
    steps: list[ActionStep] = []
    steps_indent: int | None = None
    step: ActionStep | None = None
    step_indent: int | None = None
    with_indent: int | None = None

    for raw in lines:
        content = raw.lstrip()
        if not content or content.startswith("#"):
            continue
        indent = len(raw) - len(content)
        if steps_indent is None:
            if content == "steps:" and indent >= 4:
                steps_indent = indent
            continue
        if indent <= steps_indent:
            steps_indent = None
            step = None
            step_indent = None
            with_indent = None
            if content == "steps:" and indent >= 4:
                steps_indent = indent
            continue
        if indent == steps_indent + 2 and content.startswith("-"):
            step = ActionStep()
            steps.append(step)
            step_indent = indent
            with_indent = None
            remainder = content[1:].strip()
            if remainder.startswith("uses:"):
                step.uses = _scalar(remainder.partition(":")[2])
            continue
        if step is None or step_indent is None:
            continue
        if indent == step_indent + 2 and content.startswith("uses:"):
            step.uses = _scalar(content.partition(":")[2])
            with_indent = None
        elif indent == step_indent + 2 and content == "with:":
            with_indent = indent
        elif with_indent is not None and indent == with_indent + 2 and ":" in content:
            key, _, value = content.partition(":")
            step.with_values[key.strip()] = _scalar(value)
        elif indent <= step_indent + 2:
            with_indent = None
    return [item for item in steps if item.uses is not None]


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


def ci_source_policy_errors(text: str, *, name: str = "ci.yml") -> list[str]:
    """Return failures in CI event, permission, and Rust-toolchain policy."""
    errors: list[str] = []
    lines = text.splitlines()

    dispatch_indexes = [
        index
        for index, line in enumerate(lines)
        if len(line) - len(line.lstrip()) == 2 and line.strip() == "workflow_dispatch:"
    ]
    if len(dispatch_indexes) != 1:
        errors.append(f"{name}: workflow_dispatch must be one top-level event")
    else:
        start = dispatch_indexes[0]
        dispatch_has_inputs = False
        for line in lines[start + 1 :]:
            content = line.strip()
            indent = len(line) - len(line.lstrip())
            if content and indent <= 2:
                break
            if content.startswith("inputs:") and indent == 4:
                dispatch_has_inputs = True
        if dispatch_has_inputs:
            errors.append(f"{name}: top-level workflow_dispatch must define no inputs")

    permission_indexes = [
        index
        for index, line in enumerate(lines)
        if len(line) - len(line.lstrip()) == 0 and line.strip() == "permissions:"
    ]
    permission_values: dict[str, str] = {}
    if len(permission_indexes) == 1:
        for line in lines[permission_indexes[0] + 1 :]:
            content = line.strip()
            indent = len(line) - len(line.lstrip())
            if content and indent == 0:
                break
            if content and not content.startswith("#") and indent == 2 and ":" in content:
                key, _, value = content.partition(":")
                permission_values[key.strip()] = _scalar(value)
    if len(permission_indexes) != 1 or permission_values != {"contents": "read"}:
        errors.append(f"{name}: workflow permissions must be exactly contents: read")

    in_jobs = False
    for line in lines:
        content = line.strip()
        indent = len(line) - len(line.lstrip())
        if indent == 0:
            in_jobs = content == "jobs:"
        elif in_jobs and indent >= 4 and content == "permissions:":
            errors.append(f"{name}: job-level permissions overrides are not allowed")

    rust_steps = [
        step for step in action_steps(text) if step.uses and step.uses.startswith("dtolnay/rust-toolchain@")
    ]
    for step in rust_steps:
        if step.with_values.get("toolchain") != "stable":
            errors.append(
                f"{name}: every dtolnay/rust-toolchain action step must set with.toolchain to stable"
            )
    return errors


def cargo_deny_policy_errors(text: str, *, name: str = "ci.yml") -> list[str]:
    """Return failures in the repository's exact cargo-deny installer policy."""
    installers = [
        step for step in action_steps(text) if step.uses and step.uses.startswith("taiki-e/install-action@")
    ]
    errors: list[str] = []
    if len(installers) != 1:
        errors.append(f"{name}: exactly one taiki-e/install-action action step is required")
        return errors
    installer = installers[0]
    if installer.with_values.get("tool") != "cargo-deny@0.20.2":
        errors.append(f"{name}: cargo-deny tool must be exactly cargo-deny@0.20.2")
    if installer.with_values.get("fallback") != "none":
        errors.append(f"{name}: cargo-deny installer fallback must be exactly none")
    return errors


def check_repository(
    *, workflows_dir: Path = WORKFLOWS_DIR, ci_workflow: Path = CI_WORKFLOW
) -> list[str]:
    """Return all workflow security policy failures for a repository checkout."""
    workflows = sorted((*workflows_dir.glob("*.yml"), *workflows_dir.glob("*.yaml")))
    inventory = sorted(workflow.name for workflow in workflows)
    errors: list[str] = []
    if inventory != ["ci.yml"]:
        errors.append(f"workflow inventory must be exactly ['ci.yml']; found {inventory}")
    for workflow in workflows:
        errors.extend(workflow_policy_errors(workflow.read_text(encoding="utf-8"), name=str(workflow)))
    if ci_workflow.is_file():
        ci_text = ci_workflow.read_text(encoding="utf-8")
        errors.extend(ci_source_policy_errors(ci_text, name=str(ci_workflow)))
        errors.extend(cargo_deny_policy_errors(ci_text, name=str(ci_workflow)))
    else:
        errors.append(f"required CI workflow is missing: {ci_workflow}")
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
