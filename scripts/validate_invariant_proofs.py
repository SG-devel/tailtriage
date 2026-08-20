#!/usr/bin/env python3
"""Validate the living invariant registry and its source proof markers."""

from __future__ import annotations

import io
import re
import sys
import tokenize
from dataclasses import dataclass
from pathlib import Path

HEADER = ["ID", "Primary linkage", "Invariant / contract", "Behavior owner",
          "Primary proof boundary", "Secondary boundary / non-claim", "Proof class / cadence"]
ID_RE = re.compile(r"^[A-Z][0-9]{2}$")
RUST_RE = re.compile(r"^// TT-INVARIANT: ([A-Z][0-9]{2}) (primary|secondary)$")
PY_RE = re.compile(r"^# TT-INVARIANT: ([A-Z][0-9]{2}) (primary|secondary)$")
EXCLUDED = {".git", "target", ".venv", "venv"}


class ValidationError(Exception):
    pass


@dataclass(frozen=True)
class Marker:
    invariant_id: str
    role: str
    path: Path
    line: int


def fail(path: Path, line: int, message: str) -> None:
    raise ValidationError(f"{path}:{line}: {message}")


def parse_registry(path: Path) -> dict[str, str]:
    lines = path.read_text(encoding="utf-8").splitlines()
    headings = [i for i, line in enumerate(lines) if line == "## Invariant registry"]
    if len(headings) != 1:
        fail(path, 1, f"expected exactly one '## Invariant registry' heading, found {len(headings)}")
    start = headings[0] + 1
    while start < len(lines) and not lines[start].strip():
        start += 1
    rows: list[tuple[int, list[str]]] = []
    for index in range(start, len(lines)):
        line = lines[index]
        if line.startswith("## "):
            break
        if not line.strip():
            if rows:
                break
            continue
        if not (line.startswith("|") and line.endswith("|")):
            fail(path, index + 1, "registry table row must start and end with '|'")
        rows.append((index + 1, [cell.strip() for cell in line[1:-1].split("|")]))
    if len(rows) < 3:
        fail(path, start + 1, "registry table requires a header, separator, and at least one row")
    if rows[0][1] != HEADER:
        fail(path, rows[0][0], "registry header must match the required seven columns exactly")
    if len(rows[1][1]) != 7 or any(not re.fullmatch(r":?-{3,}:?", c) for c in rows[1][1]):
        fail(path, rows[1][0], "registry separator must contain seven normal Markdown separators")
    registry: dict[str, str] = {}
    for line, cells in rows[2:]:
        if len(cells) != 7:
            fail(path, line, f"registry row has {len(cells)} cells; expected 7")
        if any(not cell for cell in cells):
            fail(path, line, "registry cells must be non-empty")
        invariant_id, linkage = cells[:2]
        if not ID_RE.fullmatch(invariant_id):
            fail(path, line, f"invalid invariant ID {invariant_id!r}")
        if invariant_id in registry:
            fail(path, line, f"duplicate invariant ID {invariant_id}")
        if linkage not in {"marker", "command"}:
            fail(path, line, f"invalid primary linkage {linkage!r}; expected marker or command")
        registry[invariant_id] = linkage
    return registry


def source_files(root: Path, suffix: str):
    for path in root.rglob(f"*{suffix}"):
        if not any(part in EXCLUDED for part in path.relative_to(root).parts):
            yield path


def scan_markers(root: Path) -> list[Marker]:
    markers = []
    for path in source_files(root, ".rs"):
        for line_no, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
            candidate = raw.strip()
            if candidate.startswith("//") and "TT-INVARIANT" in candidate:
                match = RUST_RE.fullmatch(candidate)
                if not match:
                    fail(path, line_no, "malformed Rust TT-INVARIANT marker")
                markers.append(Marker(*match.groups(), path, line_no))
    for path in source_files(root, ".py"):
        try:
            tokens = tokenize.generate_tokens(io.StringIO(path.read_text(encoding="utf-8")).readline)
            for token in tokens:
                if token.type == tokenize.COMMENT and "TT-INVARIANT" in token.string:
                    candidate = token.string.strip()
                    match = PY_RE.fullmatch(candidate)
                    if not match:
                        fail(path, token.start[0], "malformed Python TT-INVARIANT marker")
                    markers.append(Marker(*match.groups(), path, token.start[0]))
        except (SyntaxError, tokenize.TokenError) as error:
            fail(path, getattr(error, "lineno", 1) or 1, f"cannot tokenize Python source: {error}")
    return markers


def validate(root: Path, matrix: Path | None = None) -> None:
    matrix = matrix or root / "docs/dev/INVARIANT_PROOF_MATRIX.md"
    registry = parse_registry(matrix)
    markers = scan_markers(root)
    primary = set()
    for marker in markers:
        linkage = registry.get(marker.invariant_id)
        if linkage is None:
            fail(marker.path, marker.line, f"unknown invariant ID {marker.invariant_id}")
        if marker.role == "primary":
            if linkage == "command":
                fail(marker.path, marker.line, f"command invariant {marker.invariant_id} cannot have a primary source marker")
            primary.add(marker.invariant_id)
    missing = [key for key, linkage in registry.items() if linkage == "marker" and key not in primary]
    if missing:
        fail(matrix, 1, "marker invariants missing primary proof markers: " + ", ".join(missing))


def main() -> int:
    root = Path(__file__).resolve().parents[1]
    try:
        validate(root)
    except (OSError, UnicodeError, ValidationError) as error:
        print(error, file=sys.stderr)
        return 1
    print("invariant proof linkage is valid")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
