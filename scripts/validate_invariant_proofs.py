#!/usr/bin/env python3
"""Mechanically validate links between the invariant registry and executable tests."""
from __future__ import annotations

import ast
import io
import re
import sys
import tokenize
from dataclasses import dataclass
from pathlib import Path

COMMAND_OWNED_IDS = {"P03"}
HEADER = ["ID", "Invariant / contract", "Behavior owner", "Primary proof boundary", "Secondary boundary / non-claim", "Proof class / cadence"]
EXCLUDED = {".git", "target", ".venv", "venv"}
MARKER = re.compile(
    r"^(?://|#) TT-TEST: (support|([A-Z][0-9]{2}) (primary|secondary))$"
)


class ValidationError(Exception):
    pass


@dataclass(frozen=True)
class Summary:
    rust_tests: int
    python_tests: int
    primary_markers: int
    secondary_markers: int
    support_tests: int
    registry_invariants: int


def fail(path: Path, line: int, kind: str, message: str) -> None:
    raise ValidationError(f"{path}:{line}: [{kind}] {message}")


def _cells(line: str) -> list[str]:
    if not line.strip().startswith("|") or not line.strip().endswith("|"):
        return []
    return [cell.strip() for cell in line.strip()[1:-1].split("|")]


def parse_registry(path: Path) -> set[str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    headings = [i for i, line in enumerate(lines) if line.strip() == "## Invariant registry"]
    if len(headings) != 1:
        fail(path, headings[0] + 1 if headings else 1, "registry", "expected exactly one invariant registry heading")
    start = headings[0] + 2
    if start >= len(lines) or _cells(lines[start]) != HEADER:
        fail(path, start + 1, "registry", "registry header must have the exact six columns")
    if start + 1 >= len(lines):
        fail(path, start + 2, "registry", "missing separator row")
    separator = _cells(lines[start + 1])
    if len(separator) != 6 or any(not re.fullmatch(r":?-{3,}:?", cell) for cell in separator):
        fail(path, start + 2, "registry", "invalid six-cell separator row")
    ids: set[str] = set()
    for number in range(start + 2, len(lines)):
        if not lines[number].lstrip().startswith("|"):
            break
        cells = _cells(lines[number])
        if len(cells) != 6:
            fail(path, number + 1, "registry", "invariant row must have six cells")
        if any(not cell for cell in cells):
            fail(path, number + 1, "registry", "invariant row cells must be non-empty")
        invariant = cells[0]
        if not re.fullmatch(r"[A-Z][0-9]{2}", invariant):
            fail(path, number + 1, "registry", f"invalid invariant ID {invariant!r}")
        if invariant in ids:
            fail(path, number + 1, "registry", f"duplicate invariant ID {invariant}")
        ids.add(invariant)
    if "P03" not in ids:
        fail(path, start + 1, "registry", "P03 is required")
    return ids


def _parse_marker(path: Path, line: int, text: str):
    if "TT-INVARIANT" in text:
        fail(path, line, "legacy-marker", "legacy TT-INVARIANT linkage is forbidden")
    if "TT-TEST" not in text:
        return None
    match = MARKER.fullmatch(text)
    if not match:
        fail(path, line, "malformed-marker", "invalid TT-TEST marker")
    return ("support", None) if match.group(1) == "support" else (match.group(3), match.group(2))


def _check_markers(path: Path, line: int, name: str, markers, ids: set[str], primary: dict[str, int]) -> tuple[int, int, bool]:
    if not markers:
        fail(path, line, "missing-classification", f"test {name} has no adjacent TT-TEST marker")
    values = [value for _, value in markers]
    if len(values) != len(set(values)):
        fail(path, line, "duplicate-marker", f"test {name} repeats a marker")
    if any(role == "support" for role, _ in values):
        if len(values) != 1:
            fail(path, line, "support-mix", f"test {name} mixes support with invariant markers")
        return 0, 0, True
    by_id: dict[str, set[str]] = {}
    for role, invariant in values:
        assert invariant is not None
        if invariant not in ids:
            fail(path, line, "unknown-id", f"test {name} references {invariant}")
        by_id.setdefault(invariant, set()).add(role)
        if len(by_id[invariant]) > 1:
            fail(path, line, "conflicting-marker", f"test {name} makes {invariant} primary and secondary")
        if role == "primary":
            if invariant in COMMAND_OWNED_IDS:
                fail(path, line, "command-owned-primary", f"test {name} cannot own {invariant} primary")
            primary[invariant] = primary.get(invariant, 0) + 1
    return sum(r == "primary" for r, _ in values), sum(r == "secondary" for r, _ in values), False


def _adjacent_markers(lines: list[str], comments: dict[int, str], declaration: int, decorator_lines: set[int] | None = None):
    cursor = declaration - 1
    decorators = decorator_lines or set()
    while cursor in decorators or (cursor >= 1 and not lines[cursor - 1].strip()):
        cursor -= 1
    found = []
    while cursor in comments and "TT-TEST" in comments[cursor]:
        found.append((cursor, comments[cursor]))
        cursor -= 1
        while cursor >= 1 and not lines[cursor - 1].strip():
            cursor -= 1
    return list(reversed(found))


def scan_python(path: Path, ids: set[str], primary: dict[str, int]):
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines()
    comments: dict[int, str] = {}
    try:
        for token in tokenize.generate_tokens(io.StringIO(text).readline):
            if token.type == tokenize.COMMENT:
                comments[token.start[0]] = token.string
                _parse_marker(path, token.start[0], token.string)
        tree = ast.parse(text, filename=str(path))
    except (SyntaxError, tokenize.TokenError) as error:
        fail(path, getattr(error, "lineno", 1) or 1, "registry", str(error))
    tests = [node for node in ast.walk(tree) if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef)) and node.name.startswith("test_")]
    used: set[int] = set(); p = s = support = 0
    for node in tests:
        first_declaration_line = min(
            [node.lineno] + [decorator.lineno for decorator in node.decorator_list]
        )
        marker_rows = _adjacent_markers(lines, comments, first_declaration_line)
        parsed = [(row, _parse_marker(path, row, comment)) for row, comment in marker_rows]
        parsed = [(row, value) for row, value in parsed if value]
        used.update(row for row, _ in parsed)
        a, b, c = _check_markers(path, node.lineno, node.name, parsed, ids, primary); p += a; s += b; support += c
    for row, comment in comments.items():
        if ("TT-TEST" in comment and row not in used):
            fail(path, row, "orphan-marker", "marker is not adjacent to a concrete test")
    return len(tests), p, s, support


def _rust_lex(text: str):
    """Return code with literals/comments blanked plus real line comments."""
    out = list(text); comments = {}; i = 0; line = 1; n = len(text)
    def blank(a, b):
        for k in range(a, b):
            if out[k] != "\n": out[k] = " "
    while i < n:
        if text.startswith("//", i):
            end = text.find("\n", i); end = n if end < 0 else end
            comments[line] = text[i:end]; blank(i, end); i = end; continue
        if text.startswith("/*", i):
            start = i; depth = 1; i += 2
            while i < n and depth:
                if text.startswith("/*", i): depth += 1; i += 2
                elif text.startswith("*/", i): depth -= 1; i += 2
                else:
                    if text[i] == "\n": line += 1
                    i += 1
            blank(start, i); continue
        raw = re.match(r"r(#+)?\"", text[i:])
        if raw:
            start = i; hashes = raw.group(1) or ""; i += len(raw.group(0)); end_token = '"' + hashes
            end = text.find(end_token, i); i = n if end < 0 else end + len(end_token)
            line += text[start:i].count("\n"); blank(start, i); continue
        if text[i] == '"' or (text[i] == "'" and re.match(r"'(?:\\.|[^\\'\n])'", text[i:])):
            quote = text[i]; start = i; i += 1
            while i < n:
                if text[i] == "\\": i += 2; continue
                if text[i] == quote: i += 1; break
                i += 1
            line += text[start:i].count("\n"); blank(start, i); continue
        if text[i] == "\n": line += 1
        i += 1
    return "".join(out), comments


def scan_rust(path: Path, ids: set[str], primary: dict[str, int]):
    text = path.read_text(encoding="utf-8"); lines = text.splitlines(); clean, comments = _rust_lex(text)
    for row, comment in comments.items(): _parse_marker(path, row, comment)
    attrs = list(re.finditer(r"#\s*\[\s*(?:tokio\s*::\s*)?test(?:\s*\([^\]]*\))?\s*\]", clean))
    used_attrs = set(); used_markers = set(); count = p = s = support = 0
    for index, attr in enumerate(attrs):
        attr_line = clean.count("\n", 0, attr.start()) + 1
        next_attr = attrs[index + 1].start() if index + 1 < len(attrs) else len(clean)
        tail = clean[attr.end():next_attr]
        match = re.match(r"(?:\s*#\s*\[[^\]]*\])*\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\(", tail)
        if not match:
            fail(path, attr_line, "unsupported-test-form", "test attribute is not followed by a supported function declaration")
        name = match.group(1); fn_pos = attr.end() + match.start() + match.group(0).rfind("fn")
        fn_line = clean.count("\n", 0, fn_pos) + 1
        if fn_line == attr_line:
            fail(path, attr_line, "unsupported-test-form", "inline test declaration is unsupported")
        block_line = attr_line
        cursor = attr_line - 1
        while cursor >= 1:
            stripped = lines[cursor - 1].strip()
            if not stripped:
                cursor -= 1
            elif stripped.startswith("#[") or stripped.startswith("#!["):
                block_line = cursor; cursor -= 1
            else:
                break
        marker_rows = _adjacent_markers(lines, comments, block_line)
        parsed = [(row, _parse_marker(path, row, comment)) for row, comment in marker_rows]
        parsed = [(row, value) for row, value in parsed if value]; used_markers.update(row for row, _ in parsed)
        a,b,c = _check_markers(path, fn_line, name, parsed, ids, primary); p += a; s += b; support += c; count += 1; used_attrs.add(attr.start())
    for row, comment in comments.items():
        if "TT-TEST" in comment and row not in used_markers:
            fail(path, row, "orphan-marker", "marker is not adjacent to a concrete test")
    return count, p, s, support


def validate_repository(root: Path) -> Summary:
    root = root.resolve(); ids = parse_registry(root / "docs/dev/INVARIANT_PROOF_MATRIX.md"); primary = {}
    rust = python = p = s = support = 0
    for path in sorted(root.rglob("*")):
        if not path.is_file() or any(part in EXCLUDED for part in path.relative_to(root).parts): continue
        if path.suffix == ".rs":
            a,b,c,d = scan_rust(path, ids, primary); rust += a; p += b; s += c; support += d
        elif path.suffix == ".py":
            a,b,c,d = scan_python(path, ids, primary); python += a; p += b; s += c; support += d
    for invariant in sorted(ids - COMMAND_OWNED_IDS):
        if not primary.get(invariant): fail(root / "docs/dev/INVARIANT_PROOF_MATRIX.md", 1, "missing-primary", f"{invariant} has no primary test")
    return Summary(rust, python, p, s, support, len(ids))


def main() -> int:
    try: validate_repository(Path(__file__).resolve().parents[1])
    except (OSError, UnicodeError, ValidationError) as error:
        print(error, file=sys.stderr); return 1
    print("invariant proof linkage is valid"); return 0


if __name__ == "__main__": raise SystemExit(main())
