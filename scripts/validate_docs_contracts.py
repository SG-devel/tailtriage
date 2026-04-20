#!/usr/bin/env python3
"""Validate README/controller docs against current structural contracts."""

from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
README_PATH = REPO_ROOT / "README.md"
CONTROLLER_README_PATH = REPO_ROOT / "tailtriage-controller" / "README.md"
ANALYSIS_FIXTURE_PATH = REPO_ROOT / "demos" / "queue_service" / "fixtures" / "sample-analysis.json"
CONTROLLER_SOURCE_PATH = REPO_ROOT / "tailtriage-controller" / "src" / "lib.rs"
CORE_COLLECTOR_SOURCE_PATH = REPO_ROOT / "tailtriage-core" / "src" / "collector.rs"
PUBLIC_DOCS_GLOB = (REPO_ROOT / "docs").glob("*.md")

STALE_CONTROLLER_POLICY_NAMES = (
    "kind = \"manual\"",
    "kind = \"max_requests\"",
    "kind = \"max_duration_ms\"",
    "kind = \"first_limit_hit\"",
)


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        description="Validate public docs examples against analyzer/controller contracts."
    )
    return parser.parse_args()


def extract_fenced_block(markdown: str, *, fence: str, anchor: str) -> str:
    anchor_index = markdown.find(anchor)
    if anchor_index < 0:
        raise ValueError(f"missing anchor heading: {anchor}")

    pattern = re.compile(rf"```{re.escape(fence)}\n(.*?)\n```", re.DOTALL)
    match = pattern.search(markdown, pos=anchor_index)
    if match is None:
        raise ValueError(f"missing fenced {fence} block after anchor: {anchor}")
    return match.group(1)


def _kind_of(value: Any) -> str:
    if value is None:
        return "null"
    if isinstance(value, bool):
        return "bool"
    if isinstance(value, int):
        return "int"
    if isinstance(value, float):
        return "float"
    if isinstance(value, str):
        return "string"
    if isinstance(value, list):
        return "array"
    if isinstance(value, dict):
        return "object"
    raise TypeError(f"unsupported JSON value type: {type(value)}")


def assert_same_object_shape(*, name: str, actual: dict[str, Any], expected: dict[str, Any]) -> None:
    actual_keys = set(actual.keys())
    expected_keys = set(expected.keys())
    if actual_keys != expected_keys:
        raise ValueError(
            f"{name} key drift: expected {sorted(expected_keys)}, got {sorted(actual_keys)}"
        )

    for key, expected_value in expected.items():
        actual_kind = _kind_of(actual[key])
        expected_kind = _kind_of(expected_value)
        if actual_kind != expected_kind:
            raise ValueError(
                f"{name}.{key} type drift: expected {expected_kind}, got {actual_kind}"
            )


def validate_readme_analyzer_example() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")
    snippet = extract_fenced_block(
        readme_text,
        fence="json",
        anchor="### Example output (JSON)",
    )
    readme_json = json.loads(snippet)
    if not isinstance(readme_json, dict):
        raise ValueError("README analyzer example must be a top-level JSON object")

    fixture = json.loads(ANALYSIS_FIXTURE_PATH.read_text(encoding="utf-8"))
    if not isinstance(fixture, dict):
        raise ValueError("analysis fixture must be a top-level JSON object")

    assert_same_object_shape(name="README report", actual=readme_json, expected=fixture)

    primary = readme_json.get("primary_suspect")
    fixture_primary = fixture.get("primary_suspect")
    if not isinstance(primary, dict) or not isinstance(fixture_primary, dict):
        raise ValueError("primary_suspect must be an object")
    assert_same_object_shape(
        name="README primary_suspect",
        actual=primary,
        expected=fixture_primary,
    )

    secondary = readme_json.get("secondary_suspects")
    fixture_secondary = fixture.get("secondary_suspects")
    if not isinstance(secondary, list) or not isinstance(fixture_secondary, list):
        raise ValueError("secondary_suspects must be an array")
    if not secondary:
        raise ValueError("README secondary_suspects must include at least one suspect")

    first_secondary = secondary[0]
    first_fixture_secondary = fixture_secondary[0]
    if not isinstance(first_secondary, dict) or not isinstance(first_fixture_secondary, dict):
        raise ValueError("secondary_suspects[0] must be an object")
    assert_same_object_shape(
        name="README secondary_suspects[0]",
        actual=first_secondary,
        expected=first_fixture_secondary,
    )


def extract_run_end_policy_kinds_from_source() -> set[str]:
    source = CONTROLLER_SOURCE_PATH.read_text(encoding="utf-8")
    block_match = re.search(
        r"enum\s+RunEndPolicyConfigToml\s*\{(?P<body>.*?)\}\n\nimpl\s+From<RunEndPolicyConfigToml>",
        source,
        flags=re.DOTALL,
    )
    if block_match is None:
        raise ValueError("unable to locate RunEndPolicyConfigToml enum in controller source")

    body = block_match.group("body")
    variants = re.findall(r"\b([A-Z][A-Za-z0-9_]*)\b\s*,", body)
    if not variants:
        raise ValueError("RunEndPolicyConfigToml enum has no variants")

    return {
        re.sub(r"(?<!^)(?=[A-Z])", "_", variant).lower()
        for variant in variants
    }


def validate_controller_readme_toml() -> None:
    readme_text = CONTROLLER_README_PATH.read_text(encoding="utf-8")
    snippet = extract_fenced_block(
        readme_text,
        fence="toml",
        anchor="## TOML config and manual reload",
    )
    parsed = tomllib.loads(snippet)

    run_end_policy = (
        parsed.get("controller", {})
        .get("activation", {})
        .get("run_end_policy", {})
    )
    if not isinstance(run_end_policy, dict):
        raise ValueError("controller README run_end_policy snippet must parse as a table")

    documented_kind = run_end_policy.get("kind")
    if not isinstance(documented_kind, str):
        raise ValueError("controller README run_end_policy.kind must be a string")

    supported_kinds = extract_run_end_policy_kinds_from_source()
    if documented_kind not in supported_kinds:
        raise ValueError(
            "controller README run_end_policy.kind drift: "
            f"{documented_kind!r} not in supported {sorted(supported_kinds)}"
        )


def validate_no_stale_controller_policy_names() -> None:
    paths = [README_PATH, CONTROLLER_README_PATH, *sorted(PUBLIC_DOCS_GLOB)]
    hits: list[str] = []
    for path in paths:
        text = path.read_text(encoding="utf-8")
        for token in STALE_CONTROLLER_POLICY_NAMES:
            if token in text:
                hits.append(f"{path.relative_to(REPO_ROOT)} contains stale token: {token}")

    if hits:
        joined = "\n".join(hits)
        raise ValueError(f"stale controller run_end_policy docs found:\n{joined}")


def validate_controller_example_usage_contract() -> None:
    readme_text = CONTROLLER_README_PATH.read_text(encoding="utf-8")
    misleading_tokens = (
        "cargo add tailtriage-controller",
        "cargo run --example controller_minimal",
    )
    if all(token in readme_text for token in misleading_tokens):
        raise ValueError(
            "controller README contains a misleading dependency-example flow: "
            "`cargo add tailtriage-controller` + "
            "`cargo run --example controller_minimal`."
        )


def validate_no_public_sampler_forge_method() -> None:
    source = CORE_COLLECTOR_SOURCE_PATH.read_text(encoding="utf-8")
    if "__tailtriage_internal_register_tokio_runtime_sampler" in source:
        raise ValueError(
            "collector source still exposes __tailtriage_internal_register_tokio_runtime_sampler; "
            "public sampler metadata forge methods are not allowed"
        )


def main() -> int:
    _ = parse_args()
    validate_readme_analyzer_example()
    validate_controller_readme_toml()
    validate_no_stale_controller_policy_names()
    validate_controller_example_usage_contract()
    validate_no_public_sampler_forge_method()
    print("docs contracts validated successfully")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
