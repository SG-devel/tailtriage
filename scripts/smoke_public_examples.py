#!/usr/bin/env python3
"""Smoke-validate public examples or compile selected onboarding Markdown.

This script validates the public onboarding flow for selected examples:
1) run the example
2) confirm it writes a run artifact
3) confirm the artifact has the expected top-level schema shape
4) confirm `tailtriage-cli analyze ... --format json` succeeds
"""

from __future__ import annotations

import argparse
import json
import re
import subprocess
import tempfile
from pathlib import Path

EXAMPLES = [
    {
        "package": "tailtriage-tokio",
        "name": "minimal_checkout",
        "artifact": "tailtriage-run.json",
    },
    {
        "package": "tailtriage-axum",
        "name": "axum_core_manual",
        "artifact": "tailtriage-run.json",
    },
    {
        "package": "tailtriage-axum",
        "name": "axum_service_adoption",
        "artifact": "tailtriage-run.json",
    },
    {
        "package": "tailtriage-tokio",
        "name": "mini_service_integration",
        "artifact": "tailtriage-run.json",
    },
    {
        "package": "tailtriage-controller",
        "name": "controller_minimal",
        "artifact": "tailtriage-run-generation-1.json",
    },
    {
        "package": "tailtriage-controller",
        "name": "controller_toml_startup",
        "artifact": "tailtriage-run-generation-1.json",
    },
]

EXPECTED_RUN_TOP_LEVEL_KEYS = {
    "schema_version",
    "metadata",
    "requests",
    "stages",
    "queues",
    "inflight",
    "runtime_snapshots",
    "truncation",
}

EXPECTED_ANALYSIS_TOP_LEVEL_KEYS = {
    "request_count",
    "p95_latency_us",
    "primary_suspect",
    "secondary_suspects",
    "warnings",
}

MARKDOWN_SNIPPETS = (
    ("README.md", "## Capture one useful Run"),
    ("docs/user-guide.md", "## 2) Instrument one meaningful request"),
)
MARKDOWN_FENCE_INFO = "rust,no_run"


def repo_root() -> Path:
    return Path(__file__).resolve().parent.parent


def run_cmd(cmd: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(
        cmd,
        cwd=cwd,
        check=True,
        text=True,
        capture_output=True,
    )


def extract_markdown_rust_fence(markdown: str, *, anchor: str) -> str:
    """Extract the sole selected Rust fence from one explicitly anchored section."""
    headings = list(re.finditer(rf"(?m)^{re.escape(anchor)}\s*$", markdown))
    if not headings:
        raise ValueError(f"missing Markdown anchor: {anchor}")
    if len(headings) > 1:
        raise ValueError(
            f"ambiguous Markdown anchor: expected exactly one {anchor}; "
            f"found {len(headings)}"
        )
    heading = headings[0]

    level = len(anchor) - len(anchor.lstrip("#"))
    next_heading = re.search(
        rf"(?m)^#{{1,{level}}}\s+", markdown[heading.end() :]
    )
    section_end = (
        heading.end() + next_heading.start() if next_heading is not None else len(markdown)
    )
    section = markdown[heading.end() : section_end]
    fences = re.findall(
        rf"(?ms)^```{re.escape(MARKDOWN_FENCE_INFO)}\s*\n(.*?)^```\s*$", section
    )
    if len(fences) != 1:
        raise ValueError(
            f"expected exactly one {MARKDOWN_FENCE_INFO} fence after {anchor}; "
            f"found {len(fences)}"
        )
    return fences[0]


def compile_markdown_snippet(snippet: str, *, root: Path) -> None:
    """Compile an extracted tutorial as an ephemeral downstream Cargo consumer."""
    with tempfile.TemporaryDirectory(prefix="tailtriage-markdown-smoke-") as temp_dir:
        consumer = Path(temp_dir)
        (consumer / "src").mkdir()
        (consumer / "Cargo.toml").write_text(
            "[package]\n"
            'name = "tailtriage-markdown-smoke"\n'
            'version = "0.0.0"\n'
            'edition = "2021"\n\n'
            "[dependencies]\n"
            f"tailtriage = {{ path = {json.dumps(str(root / 'tailtriage'))} }}\n"
            'tokio = { version = "1", features = ["macros", "rt", "time"] }\n',
            encoding="utf-8",
        )
        (consumer / "src" / "main.rs").write_text(snippet, encoding="utf-8")
        run_cmd(
            ["cargo", "check", "--quiet", "--manifest-path", str(consumer / "Cargo.toml")],
            cwd=consumer,
        )


def validate_markdown_snippets() -> None:
    root = repo_root()
    extracted = []
    for relative_path, anchor in MARKDOWN_SNIPPETS:
        path = root / relative_path
        snippet = extract_markdown_rust_fence(path.read_text(encoding="utf-8"), anchor=anchor)
        extracted.append((relative_path, anchor, snippet))

    unique_snippets = list(dict.fromkeys(snippet for _, _, snippet in extracted))
    for snippet in unique_snippets:
        compile_markdown_snippet(snippet, root=root)
    if len(unique_snippets) == 1:
        print("Selected README and user-guide Markdown fences are byte-identical and compile.")
    else:
        print("Selected README and user-guide Markdown fences differ and each compiles.")


def assert_keys(payload: dict, expected: set[str], *, context: str) -> None:
    missing = sorted(expected - set(payload.keys()))
    if missing:
        missing_list = ", ".join(missing)
        raise SystemExit(f"{context} missing top-level keys: {missing_list}")


def validate_example(example: dict[str, str]) -> None:
    package = example["package"]
    name = example["name"]
    artifact_name = example["artifact"]
    root = repo_root()
    print(f"==> validating example: {package}::{name}")

    with tempfile.TemporaryDirectory(prefix=f"tailtriage-example-smoke-{package}-{name}-") as temp_dir:
        working_dir = Path(temp_dir)
        artifact_path = working_dir / artifact_name

        run_cmd(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(root / package / "Cargo.toml"),
                "--example",
                name,
            ],
            cwd=working_dir,
        )

        if not artifact_path.exists():
            raise SystemExit(
                f"example '{name}' did not create expected artifact: {artifact_path}"
            )

        run_payload = json.loads(artifact_path.read_text(encoding="utf-8"))
        if not isinstance(run_payload, dict):
            raise SystemExit(f"example '{name}' artifact is not a JSON object")

        assert_keys(
            run_payload,
            EXPECTED_RUN_TOP_LEVEL_KEYS,
            context=f"example '{name}' run artifact",
        )

        analysis = run_cmd(
            [
                "cargo",
                "run",
                "--quiet",
                "--manifest-path",
                str(root / "tailtriage-cli/Cargo.toml"),
                "--",
                "analyze",
                str(artifact_path),
                "--format",
                "json",
            ],
            cwd=root,
        )
        analysis_payload = json.loads(analysis.stdout)
        if not isinstance(analysis_payload, dict):
            raise SystemExit(f"example '{name}' analysis output is not a JSON object")

        assert_keys(
            analysis_payload,
            EXPECTED_ANALYSIS_TOP_LEVEL_KEYS,
            context=f"example '{name}' analysis report",
        )

        print(f"validated: {name}")
        print(f"  artifact: {artifact_path}")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check-markdown",
        action="store_true",
        help="compile only the selected README and user-guide Rust fences",
    )
    args = parser.parse_args()
    if args.check_markdown:
        validate_markdown_snippets()
        return

    print("Smoke-validating public examples...")
    for example in EXAMPLES:
        validate_example(example)
    print("All public examples passed smoke validation.")


if __name__ == "__main__":
    main()
