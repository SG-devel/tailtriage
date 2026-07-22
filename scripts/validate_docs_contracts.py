#!/usr/bin/env python3
"""Validate public documentation contract expectations."""

from __future__ import annotations

import argparse
import json
import re
import tomllib
from pathlib import Path
from typing import Any

REPO_ROOT = Path(__file__).resolve().parent.parent
README_PATH = REPO_ROOT / "README.md"
SPEC_PATH = REPO_ROOT / "SPEC.md"
DESIGN_NOTES_PATH = REPO_ROOT / "DESIGN_NOTES.md"
DOCS_INDEX_PATH = REPO_ROOT / "docs" / "README.md"
USER_GUIDE_PATH = REPO_ROOT / "docs" / "user-guide.md"
DIAGNOSTICS_PATH = REPO_ROOT / "docs" / "diagnostics.md"
OPERATIONS_PATH = REPO_ROOT / "docs" / "operations.md"
ANALYZER_CONFIG_EXAMPLE_PATH = REPO_ROOT / "examples" / "analyzer-config.toml"
ANALYZER_DOC_PATHS = (
    DIAGNOSTICS_PATH,
    OPERATIONS_PATH,
    USER_GUIDE_PATH,
    REPO_ROOT / "tailtriage-analyzer" / "README.md",
    REPO_ROOT / "tailtriage-cli" / "README.md",
    REPO_ROOT / "tailtriage-tracing" / "README.md",
)
DIAGNOSTIC_VALIDATION_PATH = REPO_ROOT / "docs" / "diagnostic-validation.md"
CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "ci.yml"
ARCHITECTURE_PATH = REPO_ROOT / "docs" / "architecture.md"
CONTROLLER_README_PATH = REPO_ROOT / "tailtriage-controller" / "README.md"
ANALYSIS_FIXTURE_PATH = REPO_ROOT / "demos" / "queue_service" / "fixtures" / "sample-analysis.json"
CONTROLLER_SOURCE_PATH = REPO_ROOT / "tailtriage-controller" / "src" / "lib.rs"
CORE_COLLECTOR_SOURCE_PATH = REPO_ROOT / "tailtriage-core" / "src" / "collector.rs"
CORE_LIB_SOURCE_PATH = REPO_ROOT / "tailtriage-core" / "src" / "lib.rs"
PUBLIC_DOCS_GLOB = (REPO_ROOT / "docs").glob("*.md")
USER_FACING_TERMINOLOGY_PATHS = (
    README_PATH,
    DOCS_INDEX_PATH,
    USER_GUIDE_PATH,
    DIAGNOSTICS_PATH,
    OPERATIONS_PATH,
    ARCHITECTURE_PATH,
    REPO_ROOT / "docs" / "runtime-cost.md",
    REPO_ROOT / "docs" / "collector-limits.md",
    REPO_ROOT / "docs" / "getting-started-demo.md",
    REPO_ROOT / "tailtriage" / "README.md",
    REPO_ROOT / "tailtriage-core" / "README.md",
    REPO_ROOT / "tailtriage-controller" / "README.md",
    REPO_ROOT / "tailtriage-tokio" / "README.md",
    REPO_ROOT / "tailtriage-axum" / "README.md",
    REPO_ROOT / "tailtriage-analyzer" / "README.md",
    REPO_ROOT / "tailtriage-cli" / "README.md",
    REPO_ROOT / "tailtriage-tracing" / "README.md",
    REPO_ROOT / "tailtriage" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage" / "Cargo.toml",
)

RUN_SCHEMA_CURRENT_CLAIM_PATHS = (
    README_PATH,
    SPEC_PATH,
    REPO_ROOT / "VALIDATION.md",
    REPO_ROOT / "IMPLEMENTATION_PLAN.md",
    DESIGN_NOTES_PATH,
    DOCS_INDEX_PATH,
    USER_GUIDE_PATH,
    DIAGNOSTICS_PATH,
    OPERATIONS_PATH,
    ARCHITECTURE_PATH,
    REPO_ROOT / "tailtriage-core" / "README.md",
    REPO_ROOT / "tailtriage-cli" / "README.md",
    REPO_ROOT / "tailtriage-analyzer" / "README.md",
    REPO_ROOT / "tailtriage-tracing" / "README.md",
    REPO_ROOT / "tailtriage-controller" / "README.md",
)

STALE_CONTROLLER_POLICY_NAMES = (
    'kind = "manual"',
    'kind = "max_requests"',
    'kind = "max_duration_ms"',
    'kind = "first_limit_hit"',
)

DOCS_INDEX_EXCLUDED_MARKDOWN = {
    # GitHub workflow templates, surfaced by GitHub UI rather than docs index.
    ".github/ISSUE_TEMPLATE/bug_report.md",
    ".github/ISSUE_TEMPLATE/feature_request.md",
    ".github/pull_request_template.md",

    # Agent/maintainer/planning docs, not product docs.
    "AGENTS.md",
    "DESIGN_NOTES.md",
    "IMPLEMENTATION_PLAN.md",

    # The docs index should not be required to link to itself.
    "docs/README.md",

    # Validation-domain internals. User-facing guidance is under docs/.
    "validation/collector-limits/README.md",
    "validation/collector-limits/latest/scorecard.md",
    "validation/diagnostics/README.md",
    "validation/diagnostics/latest/scorecard.md",
    "validation/runtime-cost/README.md",
    "validation/runtime-cost/latest/scorecard.md",
}

DOCS_DISALLOWED_HISTORY_PATTERNS = (
    r"issue\s*#\d+",
    r"PR\s*#\d+",
)

CAPTURE_INTEGRATION_README_PATHS = (
    REPO_ROOT / "tailtriage" / "README.md",
    REPO_ROOT / "tailtriage-core" / "README.md",
    REPO_ROOT / "tailtriage-controller" / "README.md",
    REPO_ROOT / "tailtriage-tokio" / "README.md",
    REPO_ROOT / "tailtriage-axum" / "README.md",
)

DIAGNOSTICS_FIELD_REFERENCE_LABELS = (
    "field reference",
    "field-reference",
)

VALIDATION_DOC_PATHS = (
    REPO_ROOT / "VALIDATION.md",
    DIAGNOSTIC_VALIDATION_PATH,
    REPO_ROOT / "validation" / "diagnostics" / "README.md",
    REPO_ROOT / "validation" / "diagnostics" / "latest" / "scorecard.md",
)

DIAGNOSTIC_BENCHMARK_CI_ARGS = (
    "--manifest validation/diagnostics/manifest.json",
    "--min-top1 0.75",
    "--min-top2 0.90",
    "--max-high-confidence-wrong 0",
)

STALE_VALIDATION_DOC_PHRASES = (
    "no in normal pr ci",
)


RUSTDOC_INCLUDE_CRATE_LIBS = (
    REPO_ROOT / "tailtriage" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-core" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-controller" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-tokio" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-axum" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-analyzer" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-cli" / "src" / "lib.rs",
    REPO_ROOT / "tailtriage-tracing" / "src" / "lib.rs",
)

ANALYZER_GROUPS = (
    "queueing",
    "blocking",
    "executor",
    "downstream",
    "confidence",
    "evidence",
    "route",
    "temporal",
)
ANALYZER_V1_VALID_PATHS = {
    "queueing.trigger_permille",
    "blocking.min_nonzero_samples_for_signal",
    "blocking.strong_p95_threshold",
    "blocking.strong_peak_threshold",
    "blocking.strong_nonzero_share_permille",
    "blocking.strong_min_samples",
    "executor.min_global_queue_p95_for_signal",
    "downstream.blocking_correlated_stage_patterns",
    "downstream.min_stage_samples",
    "downstream.blocking_correlation_score_margin",
    "confidence.medium_score_threshold",
    "confidence.high_score_threshold",
    "confidence.ambiguity_min_score",
    "confidence.ambiguity_score_gap",
    "evidence.low_completed_request_threshold",
    "route.min_request_count",
    "route.breakdown_limit",
    "route.emit_on_divergent_suspects",
    "route.slowest_to_fastest_p95_ratio_numerator",
    "route.slowest_to_fastest_p95_ratio_denominator",
    "route.slowest_to_global_p95_ratio_numerator",
    "route.slowest_to_global_p95_ratio_denominator",
    "temporal.min_request_count",
    "temporal.min_segment_request_count",
    "temporal.share_shift_permille",
    "temporal.p95_shift_ratio_numerator",
    "temporal.p95_shift_ratio_denominator",
    "temporal.emit_on_suspect_shift",
    "temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement",
}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description="Validate public docs contracts.")
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


def extract_fenced_blocks_after_anchor(markdown: str, *, fence: str, anchor: str) -> list[str]:
    anchor_index = markdown.find(anchor)
    if anchor_index < 0:
        raise ValueError(f"missing anchor heading: {anchor}")

    pattern = re.compile(rf"```{re.escape(fence)}\n(.*?)\n```", re.DOTALL)
    return [match.group(1) for match in pattern.finditer(markdown, pos=anchor_index)]


def extract_all_fenced_blocks(markdown: str, *, fence: str) -> list[str]:
    pattern = re.compile(rf"```{re.escape(fence)}\n(.*?)\n```", re.DOTALL)
    return [match.group(1) for match in pattern.finditer(markdown)]


def markdown_links(markdown: str) -> set[str]:
    return set(re.findall(r"\[[^\]]+\]\(([^)]+)\)", markdown))


def has_markdown_heading(markdown: str, heading_pattern: str) -> bool:
    return (
        re.search(rf"^\s*#+\s+{heading_pattern}\s*$", markdown, flags=re.IGNORECASE | re.MULTILINE)
        is not None
    )


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
            raise ValueError(f"{name}.{key} type drift: expected {expected_kind}, got {actual_kind}")



def validate_governance_strictness_contract() -> None:
    text = SPEC_PATH.read_text(encoding="utf-8")
    lower_text = text.lower()

    required_tokens = (
        "default run artifact analysis",
        "tailtriage analyze --strict-artifact",
        "tracing import `--strict` separately controls",
        "does not replace strict run artifact validation",
        "stable wrapper format",
        "only accepted tracing jsonl file format",
        "pre-stable/internal jsonl must be regenerated",
    )
    for token in required_tokens:
        if token not in lower_text:
            raise ValueError(f"SPEC.md strictness contract missing token: {token}")

    conflations = (
        r"strict artifact validation[^\n]*(?:cli/import strict flags|tracing import `--strict`)",
        r"tracing import `--strict`[^\n]*(?:runs? validate_artifact_strict|(?<!not )replace(?:s)? strict run artifact validation)",
    )
    for pattern in conflations:
        if re.search(pattern, lower_text):
            raise ValueError("SPEC.md conflates strict Run artifact validation with tracing import --strict")


def validate_governance_pending_state_contract() -> None:
    text = DESIGN_NOTES_PATH.read_text(encoding="utf-8")
    lower_text = text.lower()

    required_tokens = (
        "pending/unfinished request state can grow with admitted requests",
        "completion token finishes or the collector is dropped",
        "shutdown()` currently inspects pending requests",
        "does not clear pending bookkeeping",
        "seal the collector against later admissions",
        "separate from the retained request, queue, stage, in-flight, and runtime vectors",
        "known current limitations rather than desired permanent contracts",
    )
    for token in required_tokens:
        if token not in lower_text:
            raise ValueError(f"DESIGN_NOTES.md pending-state contract missing token: {token}")

    if re.search(r"pending/unfinished request state[^\n]*until[^\n]*run shuts down", lower_text):
        raise ValueError("DESIGN_NOTES.md must not claim shutdown clears pending request state")

    if re.search(r"(?m)^\s*all live (?:bookkeeping|state) (?:is|are) (?:capture-limited|bounded by capture limits)", lower_text):
        raise ValueError("DESIGN_NOTES.md must not claim all live bookkeeping is capture-limited")

def validate_readme_analyzer_example() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")

    anchors = (
        "### Example output (representative JSON)",
        "### Example output (JSON)",
    )

    snippet = None
    for anchor in anchors:
        if anchor in readme_text:
            snippet = extract_fenced_block(readme_text, fence="json", anchor=anchor)
            break
    if snippet is None:
        raise ValueError(f"README analyzer example anchor missing; tried: {anchors}")

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

    return {re.sub(r"(?<!^)(?=[A-Z])", "_", variant).lower() for variant in variants}


def validate_controller_readme_toml() -> None:
    readme_text = CONTROLLER_README_PATH.read_text(encoding="utf-8")
    if not has_markdown_heading(readme_text, r"TOML\s+field\s+reference"):
        raise ValueError("controller README must include a TOML field reference section")
    _validate_controller_precedence_semantics(readme_text)

    required_reference_tokens = (
        "service_name",
        "initially_enabled",
        "mode",
        "strict_lifecycle",
        "capture_limits_override",
        "max_requests",
        "max_stages",
        "max_queues",
        "max_inflight_snapshots",
        "max_runtime_snapshots",
        "enabled_for_armed_runs",
        "mode_override",
        "interval_ms",
        "run_end_policy",
        "continue_after_limits_hit",
        "auto_seal_on_limits_hit",
    )
    for token in required_reference_tokens:
        if token not in readme_text:
            raise ValueError(f"controller README TOML field reference missing token: {token}")

    if "## Minimal TOML example" in readme_text and "## Expanded TOML example" in readme_text:
        minimal_snippet = extract_fenced_block(
            readme_text,
            fence="toml",
            anchor="## Minimal TOML example",
        )
        expanded_snippet = extract_fenced_block(
            readme_text,
            fence="toml",
            anchor="## Expanded TOML example",
        )
    else:
        snippets = extract_all_fenced_blocks(readme_text, fence="toml")
        if len(snippets) < 2:
            raise ValueError("controller README must include minimal and expanded TOML examples")
        minimal_snippet, expanded_snippet = snippets[0], snippets[1]
    minimal = tomllib.loads(minimal_snippet)
    expanded = tomllib.loads(expanded_snippet)

    _validate_controller_toml_shape(parsed=minimal, example_name="minimal")
    _validate_controller_toml_shape(parsed=expanded, example_name="expanded")

    expanded_controller = expanded["controller"]
    if "initially_enabled" not in expanded_controller:
        raise ValueError("expanded controller TOML example must include controller.initially_enabled")
    if expanded_controller["initially_enabled"] is not False:
        raise ValueError("expanded controller TOML example must set controller.initially_enabled = false")

    expanded_activation = expanded_controller["activation"]
    for required_table in ("capture_limits_override", "runtime_sampler", "run_end_policy"):
        if required_table not in expanded_activation or not isinstance(
            expanded_activation[required_table], dict
        ):
            raise ValueError(
                f"expanded controller TOML example must include [controller.activation.{required_table}]"
            )

    runtime_sampler = expanded_activation["runtime_sampler"]
    for key in (
        "enabled_for_armed_runs",
        "mode_override",
        "interval_ms",
        "max_runtime_snapshots",
    ):
        if key not in runtime_sampler:
            raise ValueError(f"expanded controller TOML example must include runtime_sampler.{key}")

    run_end_policy = expanded_activation["run_end_policy"]
    if "kind" not in run_end_policy:
        raise ValueError("expanded controller TOML example must include run_end_policy.kind")


def _validate_controller_precedence_semantics(readme_text: str) -> None:
    semantic_checks = (
        (
            "service_name fallback",
            r"service_name[\s\S]{0,200}(?:fall[s]?\s+back|uses?)[\s\S]{0,120}builder",
        ),
        (
            "initially_enabled fallback",
            r"initially_enabled[\s\S]{0,200}(?:fall[s]?\s+back|uses?)[\s\S]{0,120}builder",
        ),
        (
            "activation settings owned by TOML",
            r"(?:activation[\s\S]{0,200}(?:comes?\s+from|owned\s+by)[\s\S]{0,80}toml|toml[\s\S]{0,80}owned[\s\S]{0,120}activation)",
        ),
        (
            "activation optional-subfield defaults",
            r"omitted[\s\S]{0,120}activation[\s\S]{0,120}default",
        ),
    )
    lower_text = readme_text.lower()
    for check_name, pattern in semantic_checks:
        if re.search(pattern, lower_text, flags=re.IGNORECASE) is None:
            raise ValueError(f"controller README precedence guidance missing semantic rule: {check_name}")


def _validate_controller_toml_shape(*, parsed: dict[str, Any], example_name: str) -> None:
    controller = parsed.get("controller")
    if not isinstance(controller, dict):
        raise ValueError(
            f"{example_name} controller README TOML example must include a [controller] table"
        )

    service_name = controller.get("service_name")
    if not isinstance(service_name, str) or not service_name.strip():
        raise ValueError(
            f"{example_name} controller README TOML example must include non-empty controller.service_name"
        )

    activation = controller.get("activation")
    if not isinstance(activation, dict):
        raise ValueError(
            f"{example_name} controller README TOML example must include a [controller.activation] table"
        )

    mode = activation.get("mode")
    if not isinstance(mode, str) or not mode.strip():
        raise ValueError(
            f"{example_name} controller README TOML example must include non-empty controller.activation.mode"
        )

    sink = activation.get("sink")
    if not isinstance(sink, dict):
        raise ValueError(
            f"{example_name} controller README TOML example must include a "
            "[controller.activation.sink] table"
        )

    sink_type = sink.get("type")
    output_path = sink.get("output_path")
    if sink_type != "local_json":
        raise ValueError(
            f'{example_name} controller README TOML example must set '
            'controller.activation.sink.type = "local_json"'
        )
    if not isinstance(output_path, str) or not output_path.strip():
        raise ValueError(
            f"{example_name} controller README TOML example must include non-empty "
            "controller.activation.sink.output_path"
        )

    run_end_policy = activation.get("run_end_policy")
    if run_end_policy is None:
        return
    if not isinstance(run_end_policy, dict):
        raise ValueError(f"{example_name} controller README run_end_policy snippet must parse as a table")

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


def normalize_doc_link(link: str) -> str:
    return link.split("#", 1)[0]


def repo_markdown_files() -> set[str]:
    return {
        path.relative_to(REPO_ROOT).as_posix()
        for path in REPO_ROOT.rglob("*.md")
        if ".git" not in path.parts
        and "target" not in path.parts
        and path.relative_to(REPO_ROOT).as_posix() not in DOCS_INDEX_EXCLUDED_MARKDOWN
    }


def docs_index_link_targets() -> set[str]:
    text = DOCS_INDEX_PATH.read_text(encoding="utf-8")
    docs_dir = DOCS_INDEX_PATH.parent
    targets: set[str] = set()

    for raw_link in markdown_links(text):
        link = normalize_doc_link(raw_link)

        if "://" in link or link.startswith("mailto:"):
            continue
        if not link.endswith(".md"):
            continue

        resolved = (docs_dir / link).resolve()

        try:
            targets.add(resolved.relative_to(REPO_ROOT.resolve()).as_posix())
        except ValueError:
            continue

    return targets


def validate_docs_index_contract() -> None:
    required = repo_markdown_files()
    linked = docs_index_link_targets()

    missing = sorted(required - linked)
    if missing:
        raise ValueError(f"docs index missing required Markdown links: {missing}")


def validate_root_readme_docs_link() -> None:
    text = README_PATH.read_text(encoding="utf-8")
    links = {normalize_doc_link(link) for link in markdown_links(text)}

    if "docs/README.md" not in links:
        raise ValueError("root README must link to docs/README.md")
    

def validate_user_guide_contract() -> None:
    text = USER_GUIDE_PATH.read_text(encoding="utf-8")
    lower_text = text.lower()
    required_concepts = (
        "default adoption path",
        "request lifecycle contract",
        "direct capture vs controller",
        "controller toml config",
        "tailtriagecontroller::builder(",
        "[controller]",
        "[controller.activation]",
        "[controller.activation.sink]",
        "runtime sampler",
        "future generations only",
        "insufficient_evidence",
    )
    for concept in required_concepts:
        if concept not in lower_text:
            raise ValueError(f"user guide missing required concept/token: {concept}")

    toml_snippet = extract_fenced_block(
        text,
        fence="toml",
        anchor="Minimal TOML shape:",
    )
    parsed = tomllib.loads(toml_snippet)
    controller = parsed.get("controller")
    if not isinstance(controller, dict):
        raise ValueError("user guide TOML example must include a [controller] table")

    service_name = controller.get("service_name")
    if not isinstance(service_name, str) or not service_name.strip():
        raise ValueError("user guide TOML example must include non-empty controller.service_name")

    activation = controller.get("activation")
    if not isinstance(activation, dict):
        raise ValueError("user guide TOML example must include a [controller.activation] table")

    mode = activation.get("mode")
    if not isinstance(mode, str) or not mode.strip():
        raise ValueError("user guide TOML example must include non-empty controller.activation.mode")

    sink = activation.get("sink")
    if not isinstance(sink, dict):
        raise ValueError("user guide TOML example must include a [controller.activation.sink] table")

    sink_type = sink.get("type")
    output_path = sink.get("output_path")
    if sink_type != "local_json":
        raise ValueError(
            "user guide TOML example must set controller.activation.sink.type = \"local_json\""
        )
    if not isinstance(output_path, str) or not output_path.strip():
        raise ValueError(
            "user guide TOML example must include non-empty controller.activation.sink.output_path"
        )


def validate_operations_guide_contract() -> None:
    if not OPERATIONS_PATH.exists():
        raise ValueError("operations guide is missing: docs/operations.md")

    text = OPERATIONS_PATH.read_text(encoding="utf-8")
    lower_text = text.lower()
    required_concepts = (
        "production operations guide",
        "recommended rollout path",
        "light",
        "investigation",
        "runtime sampling",
        "artifact",
        "truncation",
        "capture limits",
        "insufficient_evidence",
        "evidence_quality",
        "not universal production guarantees",
        "not proof of root cause",
        "controller",
    )
    for concept in required_concepts:
        if concept not in lower_text:
            raise ValueError(f"operations guide missing required concept/token: {concept}")

    required_refs = ("validation.md", "diagnostics.md", "runtime-cost.md", "collector-limits.md")
    for ref in required_refs:
        if ref not in lower_text:
            raise ValueError(f"operations guide missing required reference: {ref}")


def validate_diagnostics_contract_truthfulness() -> None:
    readme_text = README_PATH.read_text(encoding="utf-8")
    docs_index_text = DOCS_INDEX_PATH.read_text(encoding="utf-8")
    diagnostics_text = DIAGNOSTICS_PATH.read_text(encoding="utf-8")

    combined_labels_text = f"{readme_text}\n{docs_index_text}".lower()
    references_field_ref = any(label in combined_labels_text for label in DIAGNOSTICS_FIELD_REFERENCE_LABELS)
    if references_field_ref and "## Field reference" not in diagnostics_text:
        raise ValueError(
            "README/docs index describe diagnostics as field reference, "
            "but docs/diagnostics.md lacks a matching field reference section"
        )

def validate_analyzer_config_example_contract(*, config_path: Path = ANALYZER_CONFIG_EXAMPLE_PATH) -> None:
    if not config_path.exists():
        raise ValueError(f"missing analyzer config example: {config_path}")
    text = config_path.read_text(encoding="utf-8")
    try:
        parsed = tomllib.loads(text)
    except tomllib.TOMLDecodeError as exc:
        raise ValueError(f"invalid TOML in {config_path}: {exc}") from exc

    analyzer = parsed.get("analyzer")
    if not isinstance(analyzer, dict):
        raise ValueError(f"{config_path} must define an [analyzer] table")
    if analyzer.get("schema_version") != 1:
        raise ValueError(f"{config_path} must set analyzer.schema_version = 1")

    missing_groups = [group for group in ANALYZER_GROUPS if not isinstance(analyzer.get(group), dict)]
    if missing_groups:
        raise ValueError(
            f"{config_path} missing required [analyzer.*] groups: "
            f"{', '.join(missing_groups)}"
        )

    invalid_root = sorted(group for group in ANALYZER_GROUPS if isinstance(parsed.get(group), dict))
    if invalid_root:
        raise ValueError(
            f"{config_path} must not define root-level analyzer groups: "
            f"{', '.join(invalid_root)}"
        )


def _extract_analyzer_paths_for_validation(text: str) -> set[str]:
    prefixes = "|".join((*ANALYZER_GROUPS, "queuing"))
    pattern = re.compile(
        rf"(?:`([^`]+)`|\b(--analyzer-set\s+)?(({prefixes})\.[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*)(?:=[^\s`]+)?)"
    )
    paths: set[str] = set()
    for quoted, _, bare_with_optional_value, _ in pattern.findall(text):
        candidate = quoted or bare_with_optional_value
        candidate = candidate.split("=", 1)[0].strip()
        candidate = re.sub(r"^--analyzer-set\s+", "", candidate)
        if re.fullmatch(rf"(?:{prefixes})\.[A-Za-z0-9_]+(?:\.[A-Za-z0-9_]+)*", candidate):
            paths.add(candidate)
    return paths


def validate_no_root_level_analyzer_toml_in_docs(*, doc_paths: tuple[Path, ...] = ANALYZER_DOC_PATHS) -> None:
    for path in doc_paths:
        text = path.read_text(encoding="utf-8")
        for group in ANALYZER_GROUPS:
            if re.search(rf"(?m)^\s*\[{group}\]\s*$", text):
                raise ValueError(f"{path.relative_to(REPO_ROOT)} contains invalid root-level TOML header: [{group}]")


def validate_analyzer_tuning_tokens_contract() -> None:
    diagnostics_text = DIAGNOSTICS_PATH.read_text(encoding="utf-8")
    diagnostics_lower = diagnostics_text.lower()
    diagnostics_required = ("analyzer tuning", "analyzeoptions", "--help-analyzer-options")
    for token in diagnostics_required:
        if token not in diagnostics_lower:
            raise ValueError(f"docs/diagnostics.md missing required analyzer token: {token}")
    if "not proof" not in diagnostics_lower:
        raise ValueError("docs/diagnostics.md must include bounded wording that suspects are not proof")

    operations_lower = OPERATIONS_PATH.read_text(encoding="utf-8").lower()
    if "analyzer config" not in operations_lower and "analyzer tuning" not in operations_lower:
        raise ValueError("docs/operations.md missing analyzer config/tuning guidance")
    for token in ("representative runs", "truncation"):
        if token not in operations_lower:
            raise ValueError(f"docs/operations.md missing required analyzer-operations token: {token}")
    if (
        "same analyzer config" not in operations_lower
        and re.search(r"analyzer config\s+is\s+the\s+same", operations_lower) is None
    ):
        raise ValueError("docs/operations.md must mention using the same analyzer config across comparisons")

    user_guide_lower = USER_GUIDE_PATH.read_text(encoding="utf-8").lower()
    for token in ("[analyzer]", "schema_version = 1", "--analyzer-config", "--analyzer-set", "try_analyze_run"):
        if token not in user_guide_lower:
            raise ValueError(f"docs/user-guide.md missing required analyzer token: {token}")

    cli_lower = (REPO_ROOT / "tailtriage-cli" / "README.md").read_text(encoding="utf-8").lower()
    for token in ("--analyzer-config", "--analyzer-set", "--help-analyzer-options", "report json"):
        if token not in cli_lower:
            raise ValueError(f"tailtriage-cli/README.md missing required analyzer token: {token}")

    analyzer_lower = (REPO_ROOT / "tailtriage-analyzer" / "README.md").read_text(encoding="utf-8").lower()
    for token in ("analyzeoptions", "try_analyze_run", "with_queueing", "analyzer_config"):
        if token not in analyzer_lower:
            raise ValueError(f"tailtriage-analyzer/README.md missing required analyzer token: {token}")

def validate_analyzer_override_paths_contract(*, doc_paths: tuple[Path, ...] = ANALYZER_DOC_PATHS) -> None:
    for path in doc_paths:
        text = path.read_text(encoding="utf-8")
        candidates = _extract_analyzer_paths_for_validation(text)
        invalid = sorted(candidate for candidate in candidates if candidate not in ANALYZER_V1_VALID_PATHS)
        if invalid:
            raise ValueError(
                f"{path.relative_to(REPO_ROOT)} contains invalid analyzer override path(s): "
                + ", ".join(invalid)
            )



def _strip_allowed_analyzer_migration_note(text: str) -> str:
    """Allow old API token only in the dedicated migration-note example block."""
    marker = "## Migration note"
    marker_index = text.find(marker)
    if marker_index < 0:
        return text

    migration_section = text[marker_index:]
    migration_block_pattern = re.compile(r"```rust\n(.*?)\n```", re.DOTALL)
    block_match = migration_block_pattern.search(migration_section)
    if block_match is None:
        return text

    block = block_match.group(1)
    if "tailtriage_cli::analyze" not in block:
        return text

    start = marker_index + block_match.start(1)
    end = marker_index + block_match.end(1)
    return text[:start] + text[end:]


def validate_cli_not_presented_as_library_analyzer_api() -> None:
    paths = (
        README_PATH,
        DOCS_INDEX_PATH,
        USER_GUIDE_PATH,
        DIAGNOSTICS_PATH,
        ARCHITECTURE_PATH,
        REPO_ROOT / "tailtriage-cli" / "README.md",
    REPO_ROOT / "tailtriage-tracing" / "README.md",
        REPO_ROOT / "tailtriage-analyzer" / "README.md",
    )
    banned_tokens = ("tailtriage_cli::analyze",)
    hits: list[str] = []
    for path in paths:
        text = path.read_text(encoding="utf-8")
        scan_text = text
        if path == REPO_ROOT / "tailtriage-analyzer" / "README.md":
            scan_text = _strip_allowed_analyzer_migration_note(text)
        lowered = text.lower()
        if "tailtriage-cli" in lowered and "library analyzer api" in lowered:
            rel = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
            hits.append(f"{rel} presents tailtriage-cli as library analyzer API")
        for token in banned_tokens:
            if token in scan_text:
                rel = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
                hits.append(f"{rel} contains banned token: {token}")
    if hits:
        raise ValueError("CLI/library analyzer contract violation:\n" + "\n".join(hits))


def validate_analyzer_cli_docs_split_contract() -> None:
    analyzer_text = (REPO_ROOT / "tailtriage-analyzer" / "README.md").read_text(encoding="utf-8")
    analyzer_lower = analyzer_text.lower()
    analyzer_required = (
        "in-process",
        "completed",
        "run",
        "typed",
        "report",
        "render_json",
        "render_json_pretty",
        "analyze_run",
        "analyze_run_json",
        "analyze_run_json_pretty",
        "render_text",
        "analyzeoptions::default()",
        "tailtriage-cli",
    )
    for token in analyzer_required:
        if token not in analyzer_lower:
            raise ValueError(f"tailtriage-analyzer README missing required concept/token: {token}")

    if "not streaming" not in analyzer_lower and "not live streaming" not in analyzer_lower:
        raise ValueError("tailtriage-analyzer README must state it is not streaming/live-streaming")

    if "../docs/" in analyzer_text:
        raise ValueError("tailtriage-analyzer README must not link to ../docs/ for crates.io interpretation guidance")

    if "## How to interpret a report" not in analyzer_text:
        raise ValueError("tailtriage-analyzer README must include heading: ## How to interpret a report")

    analyzer_interpretation_tokens = (
        "primary_suspect",
        "secondary_suspects",
        "evidence[]",
        "next_checks[]",
        "score",
        "confidence",
        "evidence_quality",
        "route_breakdowns",
        "temporal_segments",
        "Report JSON",
        "Run artifact JSON",
    )
    for token in analyzer_interpretation_tokens:
        if token not in analyzer_text:
            raise ValueError(
                "tailtriage-analyzer README interpretation guidance missing required token: "
                f"{token}"
            )

    cli_text = (REPO_ROOT / "tailtriage-cli" / "README.md").read_text(encoding="utf-8")
    cli_lower = cli_text.lower()
    cli_required_patterns = (
        ("saved/run artifact loading", r"(saved|captured|on-disk|persisted|run).{0,120}artifact"),
        ("schema validation", r"(schema.{0,80}validat|validat.{0,80}schema)"),
        ("non-empty requests loader rule", r"non[-\s]?empty.{0,80}requests"),
        ("tailtriage-analyzer use", r"tailtriage-analyzer"),
        ("command-line text/json output", r"(command[-\s]?line|cli).{0,160}(text|json|human-readable)"),
        ("in-process pointer for Rust users", r"(rust|in-process).{0,120}tailtriage-analyzer"),
        ("report vs run artifact json distinction", r"(report json).{0,140}(run artifact json|artifact json)|(run artifact json).{0,140}(report json)"),
        ("cli does not consume report json as input", r"(does\s+not|never|is\s+not).{0,120}(consume|accept|load|read).{0,80}report json.{0,80}(input|artifact)"),
    )
    for label, pattern in cli_required_patterns:
        if re.search(pattern, cli_lower, flags=re.IGNORECASE | re.DOTALL) is None:
            raise ValueError(f"tailtriage-cli README missing required concept: {label}")


def validate_capture_readmes_analyzer_cli_wording_contract() -> None:
    stale_patterns = (
        r"analysis\s+is\s+still\s+done\s+by\s+`?tailtriage-cli`?",
        r"analysis\s+happens\s+in\s+`?tailtriage-cli`?",
        r"artifact\s+produced\s+here.{0,80}analy[sz]ed\s+by\s+`?tailtriage-cli`?",
        r"this\s+crate\s+writes\s+artifacts?.{0,80}`?tailtriage-cli`?\s+analy[sz]es",
        r"analysis\s+or\s+report\s+generation.{0,120}`?tailtriage-cli`?",
    )

    failures: list[str] = []
    for path in CAPTURE_INTEGRATION_README_PATHS:
        text = path.read_text(encoding="utf-8")
        lower = text.lower()
        if "tailtriage-analyzer" not in lower:
            failures.append(
                f"{path.relative_to(REPO_ROOT)} must mention tailtriage-analyzer for in-process analysis"
            )
        if "tailtriage-cli" not in lower:
            failures.append(
                f"{path.relative_to(REPO_ROOT)} must mention tailtriage-cli for command-line artifact analysis"
            )

        for pattern in stale_patterns:
            if re.search(pattern, lower, flags=re.IGNORECASE | re.DOTALL):
                failures.append(
                    f"{path.relative_to(REPO_ROOT)} contains stale CLI-only analyzer wording: {pattern}"
                )

    if failures:
        raise ValueError(
            "capture/integration README analyzer wording contract violation:\n" + "\n".join(failures)
        )

def _active_yaml_lines(text: str) -> str:
    return "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))


def _workflow_step_blocks(workflow_text: str) -> list[str]:
    starts = [
        match.start()
        for match in re.finditer(r"(?m)^\s*-\s+name\s*:", workflow_text)
    ]
    if not starts:
        return []

    starts.append(len(workflow_text))
    return [workflow_text[starts[index] : starts[index + 1]] for index in range(len(starts) - 1)]


def _compact_command_text(text: str) -> str:
    return re.sub(r"\s+", " ", text).strip()


def validate_diagnostic_benchmark_ci_contract(
    *, workflow_path: Path = CI_WORKFLOW_PATH
) -> None:
    workflow_text = workflow_path.read_text(encoding="utf-8")
    matching_steps = [
        _active_yaml_lines(block)
        for block in _workflow_step_blocks(workflow_text)
        if "scripts/diagnostic_benchmark.py" in _active_yaml_lines(block)
    ]

    if not matching_steps:
        raise ValueError(
            ".github/workflows/ci.yml must run scripts/diagnostic_benchmark.py "
            "as a normal CI step"
        )

    benchmark_step = matching_steps[0]
    if re.search(
        r"(?im)^\s*continue-on-error\s*:\s*[\"']?true[\"']?\s*$", benchmark_step
    ):
        raise ValueError(
            "deterministic diagnostics benchmark CI step must not set "
            "continue-on-error: true"
        )

    command_text = _compact_command_text(benchmark_step)
    missing_args = [arg for arg in DIAGNOSTIC_BENCHMARK_CI_ARGS if arg not in command_text]
    if missing_args:
        raise ValueError(
            "deterministic diagnostics benchmark CI command missing required arguments: "
            f"{missing_args}"
        )


def validate_validation_docs_ci_contract(
    *, doc_paths: tuple[Path, ...] = VALIDATION_DOC_PATHS
) -> None:
    failures: list[str] = []
    combined_text_parts: list[str] = []
    for path in doc_paths:
        text = path.read_text(encoding="utf-8")
        combined_text_parts.append(text)
        lower_text = text.lower()
        for phrase in STALE_VALIDATION_DOC_PHRASES:
            if phrase in lower_text:
                try:
                    display_path = str(path.relative_to(REPO_ROOT))
                except ValueError:
                    display_path = str(path)
                failures.append(f"{display_path} contains stale validation-CI wording: {phrase}")

    if failures:
        raise ValueError("validation docs contain stale CI wording:\n" + "\n".join(failures))

    combined_text = "\n".join(combined_text_parts)
    if ".github/workflows/validation-snapshot.yml" not in combined_text:
        raise ValueError(
            "validation docs must state durable/versioned scorecards are produced by "
            ".github/workflows/validation-snapshot.yml"
        )

    if re.search(
        r"normal\s+CI.{0,160}(?:does\s+not|doesn't).{0,120}"
        r"(?:publish|upload|auto-overwrite).{0,120}"
        r"(?:durable\s+)?(?:diagnostic\s+)?scorecards?",
        combined_text,
        flags=re.IGNORECASE | re.DOTALL,
    ) is None:
        raise ValueError(
            "validation docs must state normal CI does not publish durable diagnostic scorecards"
        )


def validate_architecture_contract() -> None:
    text = ARCHITECTURE_PATH.read_text(encoding="utf-8")
    required_tokens = (
        "`tailtriage`",
        "`tailtriage-controller`",
        "default entry point",
        "file-based",
    )
    for token in required_tokens:
        if token not in text:
            raise ValueError(f"architecture doc missing required product-contract token: {token}")


def validate_docs_no_history_framing() -> None:
    failures: list[str] = []
    for path in sorted(PUBLIC_DOCS_GLOB):
        text = path.read_text(encoding="utf-8")
        for pattern in DOCS_DISALLOWED_HISTORY_PATTERNS:
            if re.search(pattern, text, flags=re.IGNORECASE):
                failures.append(f"{path.relative_to(REPO_ROOT)} matches disallowed pattern: {pattern}")

    if failures:
        raise ValueError("docs include stale history/process framing:\n" + "\n".join(failures))


def validate_no_user_facing_facade_wording() -> None:
    failures: list[str] = []
    for path in USER_FACING_TERMINOLOGY_PATHS:
        text = path.read_text(encoding="utf-8")
        if re.search(r"\bfacade\b", text, flags=re.IGNORECASE):
            try:
                display_path = str(path.relative_to(REPO_ROOT))
            except ValueError:
                display_path = str(path)
            failures.append(f"{display_path} contains disallowed term: facade")

    if failures:
        raise ValueError(
            "user-facing files contain stale facade wording:\n" + "\n".join(failures)
        )



def validate_crate_rustdocs_include_readmes() -> None:
    required = '#![doc = include_str!("../README.md")]'
    failures: list[str] = []
    for path in RUSTDOC_INCLUDE_CRATE_LIBS:
        text = path.read_text(encoding="utf-8")
        if required not in text:
            failures.append(
                f"{path.relative_to(REPO_ROOT)} missing required rustdoc include_str README directive"
            )

    if failures:
        raise ValueError("crate rustdoc README include contract violation:\n" + "\n".join(failures))

def is_misleading_controller_example_flow(readme_text: str) -> bool:
    for block in re.findall(r"```bash\n(.*?)\n```", readme_text, flags=re.DOTALL):
        if "cargo add tailtriage-controller" in block and "cargo run --example controller_minimal" in block:
            return True
    return False


def validate_controller_example_usage_contract() -> None:
    readme_text = CONTROLLER_README_PATH.read_text(encoding="utf-8")
    if is_misleading_controller_example_flow(readme_text):
        raise ValueError(
            "controller README contains a misleading dependency-example flow: "
            "`cargo add tailtriage-controller` + `cargo run --example controller_minimal`."
        )


def find_public_sampler_forge_methods(source: str) -> list[str]:
    return re.findall(r"^\s*pub\s+fn\s+([A-Za-z0-9_]*sampler[A-Za-z0-9_]*)\s*\(", source, re.MULTILINE)


def validate_sampler_integration_boundary() -> None:
    collector_source = CORE_COLLECTOR_SOURCE_PATH.read_text(encoding="utf-8")
    lib_source = CORE_LIB_SOURCE_PATH.read_text(encoding="utf-8")

    if "__tailtriage_internal_register_tokio_runtime_sampler" in collector_source:
        raise ValueError(
            "collector source still exposes __tailtriage_internal_register_tokio_runtime_sampler; "
            "public sampler metadata forge methods are not allowed"
        )

    public_methods = find_public_sampler_forge_methods(collector_source)
    if public_methods:
        raise ValueError(
            "collector source exposes public sampler-related methods: " f"{sorted(public_methods)}"
        )

    if "#[doc(hidden)]\npub mod __internal {" not in lib_source:
        raise ValueError("tailtriage-core hidden __internal integration module is missing")

    if "pub fn register_tokio_runtime_sampler(" not in lib_source:
        raise ValueError(
            "tailtriage-core hidden __internal register_tokio_runtime_sampler hook is missing"
        )



def normalized_words(text: str) -> str:
    return re.sub(r"\s+", " ", text.lower()).strip()


def require_doc_concepts(path: Path, concepts: tuple[tuple[str, tuple[str, ...]], ...]) -> None:
    text = normalized_words(path.read_text(encoding="utf-8"))
    missing = [label for label, tokens in concepts if not all(token in text for token in tokens)]
    if missing:
        raise ValueError(
            f"{path.relative_to(REPO_ROOT)} missing public docs contract concept(s): "
            + ", ".join(missing)
        )


def validate_tracing_completed_jsonl_public_contract() -> None:
    require_doc_concepts(
        USER_GUIDE_PATH,
        (
            ("retained original source output", ("completed-span jsonl output contains retained original tracing source records",)),
            ("representable-evidence-only replay parity", ("replay parity is limited to representable normalized request/stage/queue evidence",)),
            ("complete run artifact", ("run json remains the complete persisted artifact",)),
            ("run-only omissions", ("runtime snapshots", "in-flight snapshots", "semantic/raw truncation counters", "omitted-source diagnostics")),
        ),
    )
    require_doc_concepts(
        ARCHITECTURE_PATH,
        (
            ("tracing parser and retention role", ("tracing-specific parsing and retention",)),
            ("core normalization role", ("core normalization",)),
            ("private provenance role", ("private source provenance",)),
            ("retained-source jsonl role", ("retained-source jsonl",)),
            ("complete run artifact", ("run json remains the complete persisted artifact",)),
        ),
    )
    require_doc_concepts(
        SPEC_PATH,
        (
            ("prompt 05 public api boundary", ("prompt 05 owns public tracing api simplification",)),
            ("prompt 06 compatibility boundary", ("prompt 06 owns compatibility-mode removal",)),
        ),
    )



def validate_tracing_jsonl_no_compat_guidance_contract() -> None:
    public_paths = (
        README_PATH,
        SPEC_PATH,
        USER_GUIDE_PATH,
        DOCS_INDEX_PATH,
        ARCHITECTURE_PATH,
        REPO_ROOT / "tailtriage-cli" / "README.md",
        REPO_ROOT / "tailtriage-tracing" / "README.md",
    )
    disallowed_patterns = (
        (r"jsonl" + r"parse" + r"mode", "JSONL parse mode API"),
        (r"wrapper" + r"\s*" + r"-?" + r"\s*" + r"only" + r"\s+" + r"mode", "tracing JSONL wrapper-only mode wording"),
        (r"--" + r"\s*" + r"input-format", "CLI input format option"),
        (r"compatible\s+(?:mode|parser|tracing\s+import)", "compatible tracing import guidance"),
        (r'(?<!"format":"tailtriage\.tracing-span\.v1",)"span"\s*:\s*\{', "unversioned tracing JSONL example"),
    )
    for path in public_paths:
        text = path.read_text(encoding="utf-8")
        compact = re.sub(r"\s+", " ", text.lower())
        for pattern, label in disallowed_patterns:
            if re.search(pattern, compact):
                raise ValueError(f"{path.relative_to(REPO_ROOT)} contains unsupported tracing JSONL guidance: {label}")


def strip_live_tracing_migration_sections(text: str) -> str:
    pattern = re.compile(
        r"^## Live tracing session migration\s*$.*?(?=^##\s+|\Z)",
        flags=re.IGNORECASE | re.MULTILINE | re.DOTALL,
    )
    return pattern.sub("", text)


def validate_tracing_readme_migration_section_contract() -> None:
    path = REPO_ROOT / "tailtriage-tracing" / "README.md"
    text = path.read_text(encoding="utf-8")
    normalized = normalized_words(text)
    duplicate = "for both `tracingsession` and `tracingsession`"
    if duplicate in normalized:
        raise ValueError(
            "tailtriage-tracing/README.md contains duplicated TracingSession migration wording"
        )
    heading_count = len(
        re.findall(
            r"^## Live tracing session migration\s*$",
            text,
            flags=re.IGNORECASE | re.MULTILINE,
        )
    )
    if heading_count != 1:
        raise ValueError(
            "tailtriage-tracing/README.md must contain exactly one "
            "'## Live tracing session migration' heading"
        )


def validate_live_tracing_session_public_contract() -> None:
    validate_tracing_readme_migration_section_contract()
    required = (
        USER_GUIDE_PATH,
        REPO_ROOT / "tailtriage-tracing" / "README.md",
    )
    for path in required:
        require_doc_concepts(
            path,
            (
                ("sole current live entry point", ("tracingsession", "current live", "entry point")),
                ("async shutdown", ("shutdown().await",)),
                ("opt-in background sampling", ("background runtime sampling is opt-in", "sampler_interval",)),
                ("opt-in manual runtime", ("manual runtime collection is opt-in", "manual_runtime_snapshots",)),
                ("fallible manual recording", ("record_runtime_snapshot", "configuration error")),
                ("retained original source jsonl", ("completed-span jsonl", "retained original tracing source")),
                ("complete run artifact", ("run json", "complete persisted artifact")),
                ("independent transactions", ("each output file is an independent transaction",)),
            ),
        )

    obsolete = (
        "TracingRecorder",
        "TracingRecorderBuilder",
        "TracingIntakeSession",
        "TracingIntakeSessionBuilder",
        "TracingTokioSession",
        "TracingTokioSessionBuilder",
        "TracingTokioSessionStartError",
        "TracingTokioSessionShutdownError",
        "disable_background_sampler",
        "block_on_ready",
    )
    public_paths = (
        README_PATH,
        USER_GUIDE_PATH,
        OPERATIONS_PATH,
        ARCHITECTURE_PATH,
        REPO_ROOT / "tailtriage" / "README.md",
        REPO_ROOT / "tailtriage-tracing" / "README.md",
        REPO_ROOT / "tailtriage" / "src" / "lib.rs",
        REPO_ROOT / "tailtriage-tracing" / "src" / "lib.rs",
    )
    for path in public_paths:
        current = strip_live_tracing_migration_sections(path.read_text(encoding="utf-8"))
        found = [symbol for symbol in obsolete if symbol in current]
        if found:
            raise ValueError(
                f"{path.relative_to(REPO_ROOT)} contains obsolete current live tracing guidance outside migration section: {found}"
            )


RUN_COLLECTION_JSON_KEYS = frozenset(
    ("requests", "stages", "queues", "inflight", "runtime_snapshots")
)
RUN_JSON_CONTEXT_RE = re.compile(
    r"Run\s+(?:JSON(?:\s+artifact)?|artifact|schema)", re.IGNORECASE
)


def _json_objects_with_schema_version_one(text: str) -> list[tuple[dict[str, Any], int]]:
    decoder = json.JSONDecoder()
    objects: list[tuple[dict[str, Any], int]] = []
    for match in re.finditer(r'\{', text):
        try:
            value, _ = decoder.raw_decode(text[match.start() :])
        except json.JSONDecodeError:
            continue
        if isinstance(value, dict) and value.get("schema_version") == 1:
            objects.append((value, match.start()))
    return objects


def _has_explicit_run_json_context(text: str, offset: int) -> bool:
    context = text[max(0, offset - 240) : offset]
    return bool(RUN_JSON_CONTEXT_RE.search(context))


def _looks_like_run_json_object(value: dict[str, Any], text: str, offset: int) -> bool:
    if value.get("schema_version") != 1:
        return False
    if _has_explicit_run_json_context(text, offset):
        return True
    keys = set(value)
    return "metadata" in keys and len(keys & RUN_COLLECTION_JSON_KEYS) >= 2


def validate_run_schema_v2_public_contract(
    *,
    doc_paths: tuple[Path, ...] = RUN_SCHEMA_CURRENT_CLAIM_PATHS,
    required_current_paths: tuple[Path, ...] | None = None,
) -> None:
    if required_current_paths is None:
        required_current_paths = (
            SPEC_PATH,
            DIAGNOSTICS_PATH,
            REPO_ROOT / "tailtriage-cli" / "README.md",
        )
        if doc_paths != RUN_SCHEMA_CURRENT_CLAIM_PATHS:
            required_current_paths = doc_paths
    stale_run_patterns = (
        r"current\s+supported\s+run\s+schema\s+version\s+(?:is\s*)?[:=]?\s*`?1`?",
        r"current\s+run\s+json\s+schema\s+version\s+(?:is\s*)?[:=]?\s*`?1`?",
    )
    stale_canonical_patterns = (
        r"current\s+supported\s+schema\s+version\s+is\s*`?1`?",
        r"current\s+supported\s+schema\s+version\s*:\s*`?1`?",
    )
    for path in doc_paths:
        text = path.read_text(encoding="utf-8")
        for pattern in stale_run_patterns:
            if re.search(pattern, text, flags=re.IGNORECASE):
                raise ValueError(
                    f"{path.relative_to(REPO_ROOT)} contains stale current Run schema claim: {pattern}"
                )
        if path in required_current_paths:
            for pattern in stale_canonical_patterns:
                if re.search(pattern, text, flags=re.IGNORECASE):
                    raise ValueError(
                        f"{path.relative_to(REPO_ROOT)} contains stale current Run schema claim: {pattern}"
                    )
        if re.search(
            r"(?:metadata\.finished_at_unix_ms|RunMetadata::finished_at_unix_ms)",
            text,
            flags=re.IGNORECASE,
        ):
            raise ValueError(
                f"{path.relative_to(REPO_ROOT)} contains removed current Run metadata field"
            )
        for value, offset in _json_objects_with_schema_version_one(text):
            if _looks_like_run_json_object(value, text, offset):
                raise ValueError(
                    f"{path.relative_to(REPO_ROOT)} contains stale Run JSON schema-version 1 example"
                )

    required = (
        "Run JSON schema version 2",
        "metadata.finalized_at_unix_ms",
        "sole run-level finalization timestamp",
        "Schema-v1 Run JSON",
        "Event-level completion timestamps",
    )
    for path in required_current_paths:
        text = path.read_text(encoding="utf-8")
        missing = [token for token in required if token not in text]
        if missing:
            raise ValueError(f"{path.relative_to(REPO_ROOT)} missing Run schema v2 wording: {missing}")
        lower = text.lower()
        if "numeric finalization" not in lower and "numeric `metadata.finalized_at_unix_ms`" not in lower:
            raise ValueError(f"{path.relative_to(REPO_ROOT)} must require numeric finalization for persisted Run artifacts")
        if "null" not in lower:
            raise ValueError(f"{path.relative_to(REPO_ROOT)} must permit null finalization for active snapshots")

def main() -> int:
    _ = parse_args()
    validate_governance_strictness_contract()
    validate_governance_pending_state_contract()
    validate_readme_analyzer_example()
    validate_crate_rustdocs_include_readmes()
    validate_controller_readme_toml()
    validate_no_stale_controller_policy_names()
    validate_docs_index_contract()
    validate_root_readme_docs_link()
    validate_user_guide_contract()
    validate_operations_guide_contract()
    validate_diagnostics_contract_truthfulness()
    validate_analyzer_config_example_contract()
    validate_no_root_level_analyzer_toml_in_docs()
    validate_analyzer_tuning_tokens_contract()
    validate_analyzer_override_paths_contract()
    validate_cli_not_presented_as_library_analyzer_api()
    validate_analyzer_cli_docs_split_contract()
    validate_capture_readmes_analyzer_cli_wording_contract()
    validate_diagnostic_benchmark_ci_contract()
    validate_validation_docs_ci_contract()
    validate_architecture_contract()
    validate_docs_no_history_framing()
    validate_no_user_facing_facade_wording()
    validate_controller_example_usage_contract()
    validate_sampler_integration_boundary()
    validate_tracing_completed_jsonl_public_contract()
    validate_tracing_jsonl_no_compat_guidance_contract()
    validate_live_tracing_session_public_contract()
    validate_run_schema_v2_public_contract()
    print("docs contracts validated successfully")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
