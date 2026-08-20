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
    steps = _action_steps(text)
    deny = [values for action, values in steps if action.startswith("taiki-e/install-action@")]
    if len(deny) != 1 or deny[0].get("tool") != "cargo-deny@0.20.2":
        errors.append(f"{name}: cargo-deny installer step must set tool to cargo-deny@0.20.2")
    if len(deny) != 1 or deny[0].get("fallback") != "none":
        errors.append(f"{name}: cargo-deny installer step must set fallback to none")
    return errors


def _action_steps(text: str) -> list[tuple[str, dict[str, str]]]:
    """Extract only action-step ``uses`` and its own immediate ``with`` scalar mapping."""
    lines = text.splitlines(); result = []
    for index, line in enumerate(lines):
        match = USES_RE.match(line)
        if not match: continue
        indent = len(line) - len(line.lstrip()); values = {}; j = index + 1
        # ``uses`` and ``with`` are siblings inside a list step; stop at the next list item.
        while j < len(lines) and not (
            lines[j].strip().startswith("-")
            and len(lines[j]) - len(lines[j].lstrip()) <= indent
        ):
            child = re.match(r"^\s+([A-Za-z0-9_-]+):\s*([^#\s]+)", lines[j])
            if child: values[child.group(1)] = child.group(2).strip("'\"")
            j += 1
        result.append((match.group(1).strip("'\""), values))
    return result


def ci_source_policy_errors(text: str, *, name: str = "ci.yml") -> list[str]:
    """Check narrow, repository-specific CI trust-boundary source contracts."""
    errors: list[str] = []
    dispatch = re.search(r"(?m)^(\s*)workflow_dispatch:\s*(?:\{\s*\})?\s*$", text)
    if not dispatch:
        errors.append(f"{name}: workflow_dispatch must be present and input-free")
    else:
        indent=len(dispatch.group(1)); tail=text[dispatch.end():].splitlines()
        for line in tail:
            if line.strip() and len(line)-len(line.lstrip()) <= indent: break
            if re.match(r"^\s*inputs:\s*", line): errors.append(f"{name}: workflow_dispatch must not define inputs"); break
    permission = re.search(r"(?m)^permissions:\s*$", text)
    approved = False
    if permission:
        children=[]
        for line in text[permission.end():].splitlines():
            if line.strip() and not line.startswith(" "): break
            match=re.match(r"^\s+([\w-]+):\s*([^#\s]+)",line)
            if match: children.append((match.group(1),match.group(2)))
        approved = children == [("contents", "read")]
    if not approved or re.search(r"(?m)^[ \t]+permissions:\s*", text):
        errors.append(f"{name}: workflow permissions must be exactly contents: read")
    rust_steps=[values for action,values in _action_steps(text) if action.startswith("dtolnay/rust-toolchain@")]
    if not rust_steps or any(values.get("toolchain") != "stable" for values in rust_steps):
        errors.append(f"{name}: every pinned Rust setup step must select toolchain stable")
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
