#!/usr/bin/env python3
"""Validate structural documentation and repository source-policy contracts."""

from __future__ import annotations

import argparse
import ast
import re
import shlex
import tomllib
from pathlib import Path
from typing import Any
from urllib.parse import urlsplit

REPO_ROOT = Path(__file__).resolve().parent.parent
README_PATH = REPO_ROOT / "README.md"
DEV_DOCS_DIR = REPO_ROOT / "docs" / "dev"
DOCS_INDEX_PATH = REPO_ROOT / "docs" / "README.md"
USER_GUIDE_PATH = REPO_ROOT / "docs" / "user-guide.md"
DIAGNOSTICS_PATH = REPO_ROOT / "docs" / "diagnostics.md"
OPERATIONS_PATH = REPO_ROOT / "docs" / "operations.md"
ARCHITECTURE_PATH = REPO_ROOT / "docs" / "architecture.md"
ANALYZER_CONFIG_EXAMPLE_PATH = REPO_ROOT / "examples" / "analyzer-config.toml"
ANALYZER_DOC_PATHS = (DIAGNOSTICS_PATH, OPERATIONS_PATH, USER_GUIDE_PATH, REPO_ROOT / "tailtriage-analyzer" / "README.md", REPO_ROOT / "tailtriage-cli" / "README.md", REPO_ROOT / "tailtriage-tracing" / "README.md")
CI_WORKFLOW_PATH = REPO_ROOT / ".github" / "workflows" / "ci.yml"
WORKFLOWS_DIR = REPO_ROOT / ".github" / "workflows"
CONTROLLER_README_PATH = REPO_ROOT / "tailtriage-controller" / "README.md"
CONTROLLER_SOURCE_PATH = REPO_ROOT / "tailtriage-controller" / "src" / "lib.rs"
CORE_COLLECTOR_SOURCE_PATH = REPO_ROOT / "tailtriage-core" / "src" / "collector.rs"
CORE_LIB_SOURCE_PATH = REPO_ROOT / "tailtriage-core" / "src" / "lib.rs"
PUBLIC_DOCS_GLOB = (REPO_ROOT / "docs").glob("*.md")
STALE_CONTROLLER_POLICY_NAMES = ('kind = "manual"', 'kind = "max_requests"', 'kind = "max_duration_ms"', 'kind = "first_limit_hit"')
DOCS_INDEX_EXCLUDED_MARKDOWN = {".github/ISSUE_TEMPLATE/bug_report.md", ".github/ISSUE_TEMPLATE/feature_request.md", ".github/pull_request_template.md", "AGENTS.md", "docs/README.md", "validation/collector-limits/README.md", "validation/collector-limits/latest/scorecard.md", "validation/diagnostics/README.md", "validation/diagnostics/latest/scorecard.md", "validation/runtime-cost/README.md", "validation/runtime-cost/latest/scorecard.md"}
RUSTDOC_INCLUDE_CRATE_LIBS = tuple(REPO_ROOT / crate / "src" / "lib.rs" for crate in ("tailtriage", "tailtriage-core", "tailtriage-controller", "tailtriage-tokio", "tailtriage-axum", "tailtriage-analyzer", "tailtriage-cli", "tailtriage-tracing"))
ANALYZER_GROUPS = ("queueing", "blocking", "executor", "downstream", "confidence", "evidence", "route", "temporal")
DIAGNOSTIC_BENCHMARK_CI_ARGS = ("--manifest validation/diagnostics/manifest.json", "--min-top1 0.75", "--min-top2 0.90", "--max-high-confidence-wrong 0")


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


def resolve_local_markdown_destination(
    document: Path, destination: str, *, repo_root: Path = REPO_ROOT
) -> Path | None:
    """Resolve a local Markdown destination, returning None for non-file schemes."""
    path_text = destination.split("#", 1)[0]
    if not path_text:
        return document.resolve()

    parsed = urlsplit(path_text)
    if parsed.scheme:
        return None

    root = repo_root.resolve()
    resolved = (document.parent / path_text).resolve()
    try:
        resolved.relative_to(root)
    except ValueError as error:
        raise ValueError(
            f"{document} Markdown destination escapes repository root: {destination}"
        ) from error
    return resolved


def has_markdown_heading(markdown: str, heading_pattern: str) -> bool:
    return (
        re.search(rf"^\s*#+\s+{heading_pattern}\s*$", markdown, flags=re.IGNORECASE | re.MULTILINE)
        is not None
    )


def validate_analyzer_ownership_navigation(
    *,
    required_links: dict[Path, tuple[str, ...]] | None = None,
    repo_root: Path = REPO_ROOT,
) -> None:
    """Protect document ownership and navigation without freezing explanatory prose."""
    required = required_links or {
        DOCS_INDEX_PATH: (
            "user-guide.md",
            "operations.md",
            "analyzer-guide.md",
            "diagnostics.md",
            "analyzer-rationale.md",
            "../tailtriage-cli/README.md",
            "../tailtriage-analyzer/README.md",
            "../SPEC.md",
            "dev/VALIDATION.md",
        ),
        README_PATH: (
            "docs/README.md",
            "docs/analyzer-guide.md",
            "docs/diagnostics.md",
            "docs/operations.md",
        ),
        REPO_ROOT / "docs" / "analyzer-guide.md": (
            "diagnostics.md",
            "analyzer-rationale.md",
            "operations.md",
            "../tailtriage-cli/README.md",
            "../tailtriage-analyzer/README.md",
        ),
        OPERATIONS_PATH: ("analyzer-guide.md", "diagnostics.md"),
        USER_GUIDE_PATH: ("analyzer-guide.md", "diagnostics.md"),
    }
    for path, expected in required.items():
        links = markdown_links(path.read_text(encoding="utf-8"))
        destinations: set[Path] = set()
        for link in links:
            if not link.split("#", 1)[0].lower().endswith(".md"):
                continue
            destination = resolve_local_markdown_destination(path, link, repo_root=repo_root)
            if destination is None:
                continue
            destinations.add(destination)

        missing = []
        for link in expected:
            destination = resolve_local_markdown_destination(path, link, repo_root=repo_root)
            if destination is None or destination not in destinations:
                missing.append(link)
            elif not destination.is_file():
                raise ValueError(f"{path} Markdown destination is not an existing file: {link}")
        if missing:
            display_path = path.relative_to(repo_root) if path.is_relative_to(repo_root) else path
            raise ValueError(f"{display_path} missing analyzer ownership links: {missing}")


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
    """Parse controller README examples and compare enum values with Rust source."""
    readme_text = CONTROLLER_README_PATH.read_text(encoding="utf-8")
    snippets = extract_all_fenced_blocks(readme_text, fence="toml")
    if len(snippets) < 2:
        raise ValueError("controller README must include minimal and expanded TOML examples")

    minimal, expanded = (tomllib.loads(snippet) for snippet in snippets[:2])
    _validate_controller_toml_shape(parsed=minimal, example_name="minimal")
    _validate_controller_toml_shape(parsed=expanded, example_name="expanded")


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
        and not path.is_relative_to(DEV_DOCS_DIR)
        and path.relative_to(REPO_ROOT).as_posix() not in DOCS_INDEX_EXCLUDED_MARKDOWN
    }


def docs_index_link_targets() -> set[str]:
    text = DOCS_INDEX_PATH.read_text(encoding="utf-8")
    targets: set[str] = set()

    for raw_link in markdown_links(text):
        destination = resolve_local_markdown_destination(
            DOCS_INDEX_PATH, raw_link, repo_root=REPO_ROOT
        )
        if destination is None or destination.suffix.lower() != ".md":
            continue

        targets.add(destination.relative_to(REPO_ROOT.resolve()).as_posix())

    return targets


def validate_docs_index_link_targets_exist() -> None:
    text = DOCS_INDEX_PATH.read_text(encoding="utf-8")
    missing: list[str] = []

    for raw_link in sorted(markdown_links(text)):
        destination = resolve_local_markdown_destination(
            DOCS_INDEX_PATH, raw_link, repo_root=REPO_ROOT
        )
        if destination is None or destination.suffix.lower() != ".md":
            continue
        if not destination.is_file():
            missing.append(raw_link)

    if missing:
        raise ValueError(
            f"docs index contains dead local Markdown links: {missing}"
        )


def validate_docs_index_contract() -> None:
    validate_docs_index_link_targets_exist()

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


def validate_no_root_level_analyzer_toml_in_docs(*, doc_paths: tuple[Path, ...] = ANALYZER_DOC_PATHS) -> None:
    for path in doc_paths:
        text = path.read_text(encoding="utf-8")
        for group in ANALYZER_GROUPS:
            if re.search(rf"(?m)^\s*\[{group}\]\s*$", text):
                raise ValueError(f"{path.relative_to(REPO_ROOT)} contains invalid root-level TOML header: [{group}]")


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


def validate_published_crate_readmes_are_self_contained(paths: tuple[Path, ...]) -> None:
    """Reject repository-only documentation links from published crate READMEs."""
    failures = []
    for path in paths:
        if "../docs/" in path.read_text(encoding="utf-8"):
            rel = path.relative_to(REPO_ROOT) if path.is_relative_to(REPO_ROOT) else path
            failures.append(str(rel))
    if failures:
        raise ValueError(
            "published crate READMEs must not use repository-relative ../docs/ links: "
            + ", ".join(failures)
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


def validate_residual_public_api_cleanup() -> None:
    forbidden_by_path = {
        REPO_ROOT / "tailtriage-controller" / "src" / "lib.rs": (
            "pub fn try_begin_request(",
            "pub fn try_begin_request_with(",
        ),
        REPO_ROOT / "tailtriage-tokio" / "src" / "lib.rs": (
            "pub const fn crate_name(",
            "pub fn start(\n        tailtriage: Arc<Tailtriage>,",
        ),
        REPO_ROOT / "tailtriage-axum" / "src" / "lib.rs": ("pub const fn crate_name(",),
        REPO_ROOT / "tailtriage-cli" / "src" / "lib.rs": (
            "pub mod artifact;",
            "pub enum CliAnalyzeConfigError",
            "pub fn build_analyze_options(",
            "pub fn analyzer_options_help_text(",
        ),
    }
    for path, forbidden in forbidden_by_path.items():
        text = path.read_text(encoding="utf-8")
        for symbol in forbidden:
            if symbol in text:
                raise ValueError(
                    f"{path.relative_to(REPO_ROOT)} exposes removed residual public API: {symbol}"
                )


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


def _prohibited_release_command(command: str) -> str | None:
    """Return the prohibited repository/release operation executed by a command."""
    normalized = " ".join(command.strip().split())
    if not normalized or normalized.startswith("#"):
        return None

    # This is deliberately a small command classifier, not a shell parser. Splitting
    # command lists lets each statically recognizable invocation pass through the same
    # wrapper and tool-global-option normalization.
    for segment in re.split(r"\s*(?:&&|\|\||[;|&])\s*", normalized):
        try:
            words = shlex.split(segment, comments=True)
        except ValueError:
            words = segment.split()
        if not words or words[0] in {"echo", "printf"}:
            continue

        if words[0] == "command":
            words = words[1:]
        if words and words[0] == "env":
            words = words[1:]
            while words and re.fullmatch(r"[A-Za-z_][A-Za-z0-9_]*=.*", words[0]):
                words = words[1:]
        if not words:
            continue

        tool, arguments = words[0], words[1:]
        if tool == "cargo":
            if arguments and arguments[0].startswith("+"):
                arguments = arguments[1:]
            if arguments and arguments[0] == "publish":
                return "cargo publish"
            if arguments and arguments[0] == "login":
                return "cargo registry login"
        elif tool == "git":
            while len(arguments) >= 2 and arguments[0] == "-c":
                arguments = arguments[2:]
            if arguments and arguments[0] in {"commit", "tag", "push"}:
                return {
                    "commit": "git commit",
                    "tag": "git tag creation",
                    "push": "git push",
                }[arguments[0]]
        elif (
            tool == "gh"
            and len(arguments) >= 2
            and arguments[0] == "release"
            and arguments[1] in {"create", "upload", "edit"}
        ):
            return "GitHub Release publication"
    return None


def _static_command_argument(node: ast.AST) -> str | None:
    if isinstance(node, (ast.List, ast.Tuple)):
        words: list[str] = []
        for element in node.elts:
            if not isinstance(element, ast.Constant) or not isinstance(element.value, str):
                return None
            words.append(element.value)
        return " ".join(words)
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value
    return None


def _prohibited_workflow_permission_lines(text: str) -> list[tuple[int, str]]:
    """Return prohibited workflow/job permission declarations and their lines."""
    errors: list[tuple[int, str]] = []
    # Each entry is an indentation level and a mapping key. This deliberately
    # recognizes only the workflow- and job-level permission forms relevant to
    # Z02; command strings, comments, and step mappings are outside those scopes.
    mapping_stack: list[tuple[int, str]] = []
    key_pattern = re.compile(r"^(\s*)([A-Za-z0-9_-]+)\s*:\s*(.*?)\s*$")

    for line_number, line in enumerate(text.splitlines(), start=1):
        if not line.strip() or line.lstrip().startswith("#"):
            continue
        match = key_pattern.match(line)
        if match is None:
            continue

        indent = len(match.group(1).expandtabs(2))
        key = match.group(2)
        value = match.group(3).split(" #", 1)[0].strip().lower()
        while mapping_stack and mapping_stack[-1][0] >= indent:
            mapping_stack.pop()
        parents = [parent_key for _, parent_key in mapping_stack]
        permission_scope = not parents or (
            len(parents) == 2 and parents[0] == "jobs"
        )

        if key == "permissions" and permission_scope and value == "write-all":
            errors.append((line_number, "requests prohibited permissions: write-all permission"))
        elif (
            key == "contents"
            and value == "write"
            and parents
            and parents[-1] == "permissions"
            and (len(parents) == 1 or (len(parents) == 3 and parents[0] == "jobs"))
        ):
            errors.append((line_number, "requests prohibited contents: write permission"))

        if not value:
            mapping_stack.append((indent, key))

    return errors


def validate_manual_release_boundary(
    *,
    workflow_paths: tuple[Path, ...] | None = None,
    release_script_paths: tuple[Path, ...] | None = None,
) -> None:
    """Reject durable repository/release mutation in checked-in automation."""
    if workflow_paths is None:
        workflow_paths = tuple(sorted((*WORKFLOWS_DIR.glob("*.yml"), *WORKFLOWS_DIR.glob("*.yaml"))))
    if release_script_paths is None:
        release_script_paths = tuple(sorted((REPO_ROOT / "scripts").glob("*release*.py")))

    errors: list[str] = []
    for path in workflow_paths:
        text = path.read_text(encoding="utf-8")
        for line_number, message in _prohibited_workflow_permission_lines(text):
            errors.append(f"{path.relative_to(REPO_ROOT)}:{line_number} {message}")
        for line_number, line in enumerate(text.splitlines(), start=1):
            executable = re.sub(r"^\s*(?:-\s*)?(?:run:\s*)?", "", line)
            prohibited = _prohibited_release_command(executable)
            if prohibited:
                errors.append(f"{path.relative_to(REPO_ROOT)}:{line_number} executes prohibited {prohibited}")
            if re.search(r"\bCARGO_REGISTRY(?:_[A-Z0-9]+)?_TOKEN\b", line):
                errors.append(f"{path.relative_to(REPO_ROOT)}:{line_number} configures registry publication credentials")
            if re.search(r"uses:\s*[^#\s]*(?:create[-_]release|action[-_]gh[-_]release|release[-_]action)", line, re.IGNORECASE):
                errors.append(f"{path.relative_to(REPO_ROOT)}:{line_number} invokes GitHub Release automation")

    command_runner_names = {"command", "run", "Popen", "call", "check_call", "check_output"}
    for path in release_script_paths:
        tree = ast.parse(path.read_text(encoding="utf-8"), filename=str(path))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call) or not node.args:
                continue
            name = node.func.attr if isinstance(node.func, ast.Attribute) else (
                node.func.id if isinstance(node.func, ast.Name) else ""
            )
            if name not in command_runner_names:
                continue
            command = _static_command_argument(node.args[0])
            prohibited = _prohibited_release_command(command or "")
            if prohibited:
                errors.append(
                    f"{path.relative_to(REPO_ROOT)}:{node.lineno} executes prohibited {prohibited}"
                )

    if errors:
        raise ValueError("manual release boundary failed:\n" + "\n".join(errors))


def main() -> int:
    _ = parse_args()
    validate_analyzer_ownership_navigation()
    validate_crate_rustdocs_include_readmes()
    validate_residual_public_api_cleanup()
    validate_controller_readme_toml()
    validate_no_stale_controller_policy_names()
    validate_docs_index_contract()
    validate_root_readme_docs_link()
    validate_analyzer_config_example_contract()
    validate_no_root_level_analyzer_toml_in_docs()
    validate_cli_not_presented_as_library_analyzer_api()
    validate_published_crate_readmes_are_self_contained((REPO_ROOT / "tailtriage-analyzer" / "README.md", REPO_ROOT / "tailtriage-cli" / "README.md"))
    validate_diagnostic_benchmark_ci_contract()
    validate_controller_example_usage_contract()
    validate_sampler_integration_boundary()
    validate_manual_release_boundary()
    print("docs contracts validated successfully")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
