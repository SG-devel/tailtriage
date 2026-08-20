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
    lines = text.splitlines()
    result = []
    jobs = next((i for i, line in enumerate(lines) if re.match(r"^jobs:\s*(?:#.*)?$", line)), None)
    if jobs is None:
        return result
    jobs_end = jobs + 1
    while jobs_end < len(lines) and (not lines[jobs_end].strip() or lines[jobs_end].startswith((" ", "\t"))):
        jobs_end += 1
    job_indent = min(
        (len(lines[i]) - len(lines[i].lstrip()) for i in range(jobs + 1, jobs_end)
         if re.match(r"^\s+[A-Za-z0-9_-]+:\s*(?:#.*)?$", lines[i])), default=None,
    )
    if job_indent is None:
        return result
    for index in range(jobs + 1, jobs_end):
        steps_match = re.match(r"^(\s+)steps:\s*(?:#.*)?$", lines[index])
        if not steps_match or len(steps_match.group(1)) != job_indent + 2:
            continue
        steps_indent = len(steps_match.group(1)); end = index + 1
        while end < len(lines):
            if lines[end].strip() and len(lines[end]) - len(lines[end].lstrip()) <= steps_indent:
                break
            end += 1
        list_indent = next((len(lines[i]) - len(lines[i].lstrip()) for i in range(index + 1, end)
                            if re.match(r"^\s+-\s+", lines[i])), None)
        items = [i for i in range(index + 1, end) if re.match(r"^\s+-\s+", lines[i])
                 and len(lines[i]) - len(lines[i].lstrip()) == list_indent]
        for position, item in enumerate(items):
            item_indent = len(lines[item]) - len(lines[item].lstrip())
            item_end = items[position + 1] if position + 1 < len(items) else end
            field_indent = item_indent + 2; action = None; values = {}; with_indent = None
            for current in range(item, item_end):
                source = lines[current]; indent = len(source) - len(source.lstrip())
                first_uses = re.match(r"^\s+-\s+uses:\s*([^\s#]+)", source)
                sibling_uses = re.match(r"^\s+uses:\s*([^\s#]+)", source)
                if current == item and first_uses:
                    action = first_uses.group(1).strip("'\"")
                elif sibling_uses and indent == field_indent:
                    action = sibling_uses.group(1).strip("'\"")
                elif re.match(r"^\s*with:\s*(?:#.*)?$", source) and indent == field_indent:
                    with_indent = indent
                elif with_indent is not None:
                    if source.strip() and indent <= with_indent:
                        with_indent = None
                    elif indent == with_indent + 2:
                        child = re.match(r"^\s+([A-Za-z0-9_-]+):\s*([^#\s]+)\s*(?:#.*)?$", source)
                        if child: values[child.group(1)] = child.group(2).strip("'\"")
            if action is not None: result.append((action, values))
    return result


def ci_source_policy_errors(text: str, *, name: str = "ci.yml") -> list[str]:
    """Check narrow, repository-specific CI trust-boundary source contracts."""
    errors: list[str] = []
    lines = text.splitlines()
    on_index = next((i for i, line in enumerate(lines) if re.match(r"^on:\s*(?:#.*)?$", line)), None)
    dispatch_index = dispatch_indent = None
    if on_index is not None:
        on_end = on_index + 1
        while on_end < len(lines) and (not lines[on_end].strip() or lines[on_end].startswith((" ", "\t"))):
            on_end += 1
        child_indent = min((len(lines[i]) - len(lines[i].lstrip()) for i in range(on_index + 1, on_end)
                            if lines[i].strip() and not lines[i].lstrip().startswith("#")), default=None)
        if child_indent is not None:
            for i in range(on_index + 1, on_end):
                if (len(lines[i]) - len(lines[i].lstrip()) == child_indent and
                        re.match(r"^\s*workflow_dispatch:\s*(?:\{\s*\})?\s*(?:#.*)?$", lines[i])):
                    dispatch_index, dispatch_indent = i, child_indent
                    break
    if dispatch_index is None:
        errors.append(f"{name}: workflow_dispatch must be present and input-free")
    else:
        for line in lines[dispatch_index + 1:]:
            if line.strip() and len(line)-len(line.lstrip()) <= dispatch_indent: break
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
