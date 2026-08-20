#!/usr/bin/env python3
"""Validate registry linkage to adjacent, individually classified source tests.

The Rust scanner intentionally recognizes the repository's conventional ``#[test]`` and
``#[tokio::test(...)]`` forms, with ordinary attributes between a marker and test. Python
declarations come from ``ast`` while comments come from ``tokenize`` so fixture strings cannot
masquerade as classifications.
"""
from __future__ import annotations

import ast
import io
import re
import sys
import tokenize
from dataclasses import dataclass
from pathlib import Path

HEADER = ["ID", "Primary linkage", "Invariant / contract", "Behavior owner",
          "Primary proof boundary", "Secondary boundary / non-claim", "Proof class / cadence"]
ID_RE = re.compile(r"^[A-Z][0-9]{2}$")
MARK_RE = re.compile(r"^(?://|#) TT-TEST: (?:(support)|([A-Z][0-9]{2}) (primary|secondary))$")
LEGACY_MARKER = "TT-" + "INVARIANT"
EXCLUDED = {".git", "target", ".venv", "venv"}
RUST_TEST_ATTR = re.compile(r"^\s*#\[(?:test|tokio::test(?:\([^]]*\))?)\]\s*$")
RUST_ATTR = re.compile(r"^\s*#\[[^]]+\]\s*$")
RUST_FN = re.compile(r"^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\b")
RUST_ITEM = re.compile(r"^\s*(?:pub\s+)?(?:fn|mod|struct|enum|trait|impl|type|const|static)\b")

class ValidationError(Exception): pass

@dataclass(frozen=True)
class Test:
    path: Path
    line: int
    name: str
    marker_lines: tuple[int, ...]

def fail(path: Path, line: int, message: str) -> None:
    raise ValidationError(f"{path}:{line}: {message}")

def parse_registry(path: Path) -> dict[str, str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    try: start = lines.index("## Invariant registry") + 1
    except ValueError: fail(path, 1, "expected exactly one '## Invariant registry' heading")
    while start < len(lines) and not lines[start].strip(): start += 1
    rows = []
    for index in range(start, len(lines)):
        line = lines[index]
        if line.startswith("## "): break
        if not line.strip():
            if rows: break
            continue
        if not (line.startswith("|") and line.endswith("|")): fail(path, index + 1, "invalid registry row")
        rows.append((index + 1, [c.strip() for c in line[1:-1].split("|")]))
    if len(rows) < 3 or rows[0][1] != HEADER: fail(path, start + 1, "registry header must match the required seven columns exactly")
    registry = {}
    for line, cells in rows[2:]:
        if len(cells) != 7: fail(path, line, f"registry row has {len(cells)} cells; expected 7")
        key, linkage = cells[:2]
        if not ID_RE.fullmatch(key): fail(path, line, f"invalid invariant ID {key!r}")
        if key in registry: fail(path, line, f"duplicate invariant ID {key}")
        if linkage not in {"test", "command"}: fail(path, line, f"invalid primary linkage {linkage!r}; expected test or command")
        registry[key] = linkage
    return registry

def source_files(root: Path, suffix: str):
    for path in root.rglob(f"*{suffix}"):
        if not any(p in EXCLUDED for p in path.relative_to(root).parts): yield path

def comments_python(path: Path, text: str) -> dict[int, str]:
    result = {}
    try:
        for token in tokenize.generate_tokens(io.StringIO(text).readline):
            if token.type == tokenize.COMMENT: result[token.start[0]] = token.string.strip()
    except (SyntaxError, tokenize.TokenError) as error: fail(path, getattr(error, "lineno", 1) or 1, f"cannot tokenize Python source: {error}")
    return result

def marker_block(lines: list[str], declaration: int, prefix: str, comment_lines: set[int] | None = None) -> tuple[int, ...]:
    i = declaration - 1
    # decorators/attributes and blank lines are part of the declaration block.
    while i >= 1 and (not lines[i-1].strip() or lines[i-1].lstrip().startswith(prefix + "[")):
        i -= 1
    found = []
    while i >= 1:
        raw = lines[i-1].strip()
        if comment_lines is not None and i not in comment_lines: break
        if raw.startswith(prefix + " TT-TEST:"): found.append(i); i -= 1
        else: break
    return tuple(reversed(found))

def discover_python(path: Path) -> tuple[list[Test], dict[int, str]]:
    text = path.read_text(encoding="utf-8"); lines = text.splitlines(); comments = comments_python(path, text)
    try: tree = ast.parse(text)
    except SyntaxError as error: fail(path, error.lineno or 1, f"cannot parse Python source: {error}")
    tests = []
    for node in ast.walk(tree):
        if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test_"):
            first = min([node.lineno] + [d.lineno for d in node.decorator_list])
            tests.append(Test(path, node.lineno, node.name, marker_block(lines, first, "#", set(comments)),))
    return tests, comments

def discover_rust(path: Path) -> tuple[list[Test], dict[int, str]]:
    lines = path.read_text(encoding="utf-8").splitlines(); tests=[]; comments={i:s.strip() for i,s in enumerate(lines,1) if s.strip().startswith("//")}
    for i, line in enumerate(lines, 1):
        match = RUST_FN.match(line)
        if not match: continue
        j=i-1; has_test=False
        while j >= 1 and (not lines[j-1].strip() or RUST_ATTR.match(lines[j-1])):
            has_test |= bool(RUST_TEST_ATTR.match(lines[j-1])); j-=1
        if has_test: tests.append(Test(path, i, match.group(1), marker_block(lines, j + 1, "//")))
    return tests, comments

def parse_marker(path: Path, line: int, value: str) -> tuple[str, str]:
    match = MARK_RE.fullmatch(value)
    if not match: fail(path, line, "malformed TT-TEST marker")
    return ("support", "support") if match.group(1) else (match.group(2), match.group(3))

def validate(root: Path, matrix: Path | None = None) -> None:
    matrix = matrix or root / "docs/dev/INVARIANT_PROOF_MATRIX.md"; registry=parse_registry(matrix)
    tests=[]; all_comments={}
    for path in source_files(root, ".rs"):
        found, comments=discover_rust(path); tests += found; all_comments[path]=comments
    for path in source_files(root, ".py"):
        found, comments=discover_python(path); tests += found; all_comments[path]=comments
    attached={(t.path,n) for t in tests for n in t.marker_lines}; primary=set()
    for path, comments in all_comments.items():
        for line, value in comments.items():
            if LEGACY_MARKER in value:
                fail(path, line, "legacy invariant marker is forbidden")
            if "TT-TEST" in value and (path,line) not in attached: fail(path, line, "TT-TEST marker is not attached to a test function")
    for test in tests:
        if not test.marker_lines: fail(test.path, test.line, f"test `{test.name}` has no TT-TEST classification")
        marks=[parse_marker(test.path,n,all_comments[test.path][n]) for n in test.marker_lines]
        if len(set(marks)) != len(marks): fail(test.path,test.marker_lines[0],f"test `{test.name}` has a duplicate TT-TEST marker")
        if any(i=="support" for i,_ in marks) and len(marks)>1: fail(test.path,test.marker_lines[0],f"test `{test.name}` mixes support and invariant classifications")
        for identifier, role in marks:
            if identifier == "support": continue
            if identifier not in registry: fail(test.path,test.marker_lines[0],f"test `{test.name}` references unknown invariant ID {identifier}")
            roles={r for i,r in marks if i==identifier}
            if len(roles)>1: fail(test.path,test.marker_lines[0],f"test `{test.name}` is both primary and secondary for {identifier}")
            if role=="primary":
                if registry[identifier]=="command": fail(test.path,test.marker_lines[0],f"command invariant {identifier} cannot have a primary test marker")
                primary.add(identifier)
    missing=[i for i,l in registry.items() if l=="test" and i not in primary]
    if missing: fail(matrix,1,"test invariants missing primary tests: " + ", ".join(missing))

def main() -> int:
    try: validate(Path(__file__).resolve().parents[1])
    except (OSError, UnicodeError, ValidationError) as error: print(error,file=sys.stderr); return 1
    print("invariant proof linkage is valid"); return 0
if __name__ == "__main__": raise SystemExit(main())
