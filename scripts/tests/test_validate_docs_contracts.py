#!/usr/bin/env python3
"""Tests for structural docs and source-policy validation helpers."""

from __future__ import annotations
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path
from unittest import mock
REPO_ROOT = Path(__file__).resolve().parents[2]
SCRIPTS_DIR = REPO_ROOT / 'scripts'
sys.path.insert(0, str(SCRIPTS_DIR))
import validate_docs_contracts

class ValidateDocsContractsTests(unittest.TestCase):

    def _write_residual_api_sources(self, root: Path, overrides: dict[str, str] | None = None) -> None:
        sources = {
            'tailtriage-controller/src/lib.rs': 'pub fn begin_request() {}\n',
            'tailtriage-tokio/src/lib.rs': 'pub fn builder() {}\n',
            'tailtriage-axum/src/lib.rs': 'pub fn middleware() {}\n',
            'tailtriage-cli/src/lib.rs': '#![doc = include_str!("../README.md")]\n',
            'tailtriage-core/src/config.rs': '',
            'tailtriage-core/src/collector.rs': '',
            'tailtriage-core/src/run_builder.rs': '',
            'tailtriage-core/src/lib.rs': '',
            'tailtriage-analyzer/src/options/mod.rs': 'pub struct AnalyzeOptionDescriptor { path: &\'static str }\nimpl AnalyzeOptionDescriptor { pub(crate) fn new() {} }\n',
        }
        sources.update(overrides or {})
        for rel, body in sources.items():
            path = root / rel
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(body, encoding='utf-8')

    # TT-TEST: Z02 primary
    def test_actual_repository_manual_release_boundary_is_non_mutating(self) -> None:
        validate_docs_contracts.validate_manual_release_boundary()

    # TT-TEST: support
    def test_run_end_policy_variants_include_expected_kinds(self) -> None:
        kinds = validate_docs_contracts.extract_run_end_policy_kinds_from_source()
        self.assertEqual(kinds, {'continue_after_limits_hit', 'auto_seal_on_limits_hit'})

    # TT-TEST: M01 primary
    def test_crate_rustdocs_include_readmes_contract(self) -> None:
        validate_docs_contracts.validate_crate_rustdocs_include_readmes()

    # TT-TEST: M01 secondary
    def test_crate_rustdocs_include_readmes_contract_fails_when_missing_include(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            rels = ('tailtriage/src/lib.rs', 'tailtriage-core/src/lib.rs', 'tailtriage-controller/src/lib.rs', 'tailtriage-tokio/src/lib.rs', 'tailtriage-axum/src/lib.rs', 'tailtriage-analyzer/src/lib.rs', 'tailtriage-cli/src/lib.rs', 'tailtriage-tracing/src/lib.rs')
            paths = []
            for rel in rels:
                path = repo_root / rel
                path.parent.mkdir(parents=True, exist_ok=True)
                path.write_text('#![doc = include_str!("../README.md")]\n', encoding='utf-8')
                paths.append(path)
            (repo_root / rels[0]).write_text('// missing include\n', encoding='utf-8')
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', repo_root), mock.patch.object(validate_docs_contracts, 'RUSTDOC_INCLUDE_CRATE_LIBS', tuple(paths)):
                with self.assertRaisesRegex(ValueError, 'README directive'):
                    validate_docs_contracts.validate_crate_rustdocs_include_readmes()

    # TT-TEST: support
    def test_residual_public_api_cleanup_contract_accepts_private_cli_internals(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root, {
                'tailtriage-core/src/collector.rs': 'pub(crate) enum RuntimeSamplerRegistrationError {}\n',
                'tailtriage-core/src/lib.rs': 'pub mod __internal { pub fn register_tokio_runtime_sampler() {} }\n',
                'tailtriage-core/src/validation.rs': 'enum RunValidationSummaryAudience {}\nfn summarize_normalized_run() {}\n',
            })
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: support
    def test_residual_public_api_cleanup_contract_rejects_cli_helper_export(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root, {
                'tailtriage-cli/src/lib.rs': 'pub fn build_analyze_options() {}\n',
            })
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                with self.assertRaisesRegex(ValueError, 'removed residual public API'):
                    validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_builder_method(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/config.rs', 'pub fn light(self) -> Self { self }', 'TailtriageBuilder::light')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_owned_request_method(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/collector.rs', 'pub fn begin_request_owned(&self) {}', 'Tailtriage::begin_request_owned')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_run_builder_finish(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/run_builder.rs', 'pub fn finish(self) -> Run { todo!() }', 'RunBuilder::finish')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_analyzer_methods(self) -> None:
        for method in ('with_queueing', 'valid_override_paths'):
            with self.subTest(method=method):
                self._assert_residual_api_rejected('tailtriage-analyzer/src/options/mod.rs', f'pub fn {method}() {{}}', f'AnalyzeOptions::{method}')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_public_descriptor_field(self) -> None:
        self._assert_residual_api_rejected('tailtriage-analyzer/src/options/mod.rs', 'pub struct AnalyzeOptionDescriptor { pub path: &\'static str }', 'public field')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_public_descriptor_constructor(self) -> None:
        self._assert_residual_api_rejected('tailtriage-analyzer/src/options/mod.rs', 'pub struct AnalyzeOptionDescriptor {}\nimpl AnalyzeOptionDescriptor { pub fn new() {} }', 'AnalyzeOptionDescriptor::new')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_core_root_type_export(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/lib.rs', 'pub use collector::{Tailtriage, RuntimeSamplerRegistrationError};', 'RuntimeSamplerRegistrationError')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_core_root_function_export(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/lib.rs', 'pub use validation::summarize_normalized_run;', 'summarize_normalized_run')

    def _assert_residual_api_rejected(self, rel: str, source: str, symbol: str) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root, {rel: source})
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                with self.assertRaisesRegex(ValueError, symbol):
                    validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 primary
    def test_actual_repository_residual_public_api_cleanup_contract(self) -> None:
        validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M01 primary
    def test_markdown_examples_validate_against_contract(self) -> None:
        validate_docs_contracts.validate_controller_readme_toml()

    # TT-TEST: M01 primary
    def test_analyzer_ownership_navigation(self) -> None:
        validate_docs_contracts.validate_analyzer_ownership_navigation()

    # TT-TEST: M01 secondary
    def test_analyzer_ownership_navigation_rejects_missing_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            path = repo_root / 'README.md'
            path.write_text('# Documentation\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'missing analyzer ownership links'):
                validate_docs_contracts.validate_analyzer_ownership_navigation(required_links={path: ('analyzer-guide.md',)}, repo_root=repo_root)

    # TT-TEST: M01 secondary
    def test_analyzer_ownership_navigation_accepts_exact_relative_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            source = repo_root / 'docs' / 'guide.md'
            target = repo_root / 'docs' / 'diagnostics.md'
            source.parent.mkdir()
            source.write_text('[Diagnostics](diagnostics.md)\n', encoding='utf-8')
            target.write_text('# Diagnostics\n', encoding='utf-8')
            validate_docs_contracts.validate_analyzer_ownership_navigation(required_links={source: ('diagnostics.md',)}, repo_root=repo_root)

    # TT-TEST: M01 secondary
    def test_analyzer_ownership_navigation_accepts_fragment(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            source = repo_root / 'guide.md'
            target = repo_root / 'diagnostics.md'
            source.write_text('[Confidence](diagnostics.md#confidence)\n', encoding='utf-8')
            target.write_text('# Diagnostics\n', encoding='utf-8')
            validate_docs_contracts.validate_analyzer_ownership_navigation(required_links={source: ('diagnostics.md',)}, repo_root=repo_root)

    # TT-TEST: M01 secondary
    def test_analyzer_ownership_navigation_rejects_prefix_lookalike(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            source = repo_root / 'guide.md'
            lookalike = repo_root / 'diagnostics.md-old'
            source.write_text('[Wrong](diagnostics.md-old)\n', encoding='utf-8')
            lookalike.write_text('not the destination\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'missing analyzer ownership links'):
                validate_docs_contracts.validate_analyzer_ownership_navigation(required_links={source: ('diagnostics.md',)}, repo_root=repo_root)

    # TT-TEST: M01 secondary
    def test_analyzer_ownership_navigation_rejects_missing_local_target(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            source = repo_root / 'guide.md'
            source.write_text('[Missing](diagnostics.md)\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'not an existing file'):
                validate_docs_contracts.validate_analyzer_ownership_navigation(required_links={source: ('diagnostics.md',)}, repo_root=repo_root)

    # TT-TEST: M01 secondary
    def test_analyzer_ownership_navigation_rejects_repository_escape(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workspace = Path(tmp_dir)
            repo_root = workspace / 'repo'
            repo_root.mkdir()
            source = repo_root / 'guide.md'
            (workspace / 'outside.md').write_text('# Outside\n', encoding='utf-8')
            source.write_text('[Outside](../outside.md)\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'escapes repository root'):
                validate_docs_contracts.validate_analyzer_ownership_navigation(required_links={source: ('../outside.md',)}, repo_root=repo_root)

    # TT-TEST: M01 primary
    def test_docs_index_contract(self) -> None:
        validate_docs_contracts.validate_docs_index_contract()

    # TT-TEST: M01 primary
    def test_root_readme_docs_link(self) -> None:
        validate_docs_contracts.validate_root_readme_docs_link()

    # TT-TEST: M01 primary
    def test_analyzer_config_example_contract(self) -> None:
        validate_docs_contracts.validate_analyzer_config_example_contract()

    # TT-TEST: M01 secondary
    def test_analyzer_config_example_contract_rejects_malformed_toml(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / 'analyzer-config.toml'
            path.write_text('[analyzer\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'invalid TOML'):
                validate_docs_contracts.validate_analyzer_config_example_contract(config_path=path)

    # TT-TEST: support
    def test_analyzer_config_example_contract_does_not_require_schema_groups(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            path = Path(tmp_dir) / 'analyzer-config.toml'
            path.write_text('syntactically_valid = true\n', encoding='utf-8')
            validate_docs_contracts.validate_analyzer_config_example_contract(config_path=path)

    # TT-TEST: M02 secondary
    def test_sampler_integration_boundary_contract_validates(self) -> None:
        validate_docs_contracts.validate_sampler_integration_boundary()

    # TT-TEST: M01 secondary
    def test_docs_index_contract_checks_deliberate_developer_doc_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            docs_dir = repo_root / 'docs'
            docs_dir.mkdir(parents=True)
            (repo_root / 'README.md').write_text('# Root\n', encoding='utf-8')
            docs_index_path = docs_dir / 'README.md'
            docs_index_path.write_text('[Root](../README.md)\n[Validation](dev/VALIDATION.md)\n', encoding='utf-8')
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', repo_root), mock.patch.object(validate_docs_contracts, 'DOCS_INDEX_PATH', docs_index_path), mock.patch.object(validate_docs_contracts, 'DEV_DOCS_DIR', docs_dir / 'dev'), mock.patch.object(validate_docs_contracts, 'DOCS_INDEX_EXCLUDED_MARKDOWN', {'docs/README.md'}), self.assertRaisesRegex(ValueError, 'dead local Markdown links'):
                validate_docs_contracts.validate_docs_index_contract()

    # TT-TEST: M01 secondary
    def test_docs_index_contract_rejects_repository_escape(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workspace = Path(tmp_dir)
            repo_root = workspace / 'repo'
            docs_dir = repo_root / 'docs'
            docs_dir.mkdir(parents=True)
            (workspace / 'outside.md').write_text('# Outside\n', encoding='utf-8')
            (repo_root / 'README.md').write_text('# Root\n', encoding='utf-8')
            docs_index_path = docs_dir / 'README.md'
            docs_index_path.write_text('[Outside](../../outside.md)\n', encoding='utf-8')
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', repo_root), mock.patch.object(validate_docs_contracts, 'DOCS_INDEX_PATH', docs_index_path), self.assertRaisesRegex(ValueError, 'escapes repository root'):
                validate_docs_contracts.validate_docs_index_contract()

    # TT-TEST: support
    def test_manual_release_boundary_rejects_executable_release_script_mutation(self) -> None:
        prohibited_cases = {'cargo publish': ('command(["cargo", "publish", "--locked"])\n', 'cargo publish'), 'cargo login': ('command(["cargo", "login"])\n', 'cargo registry login'), 'git commit': ('command(["git", "commit", "-m", "automated"])\n', 'git commit'), 'git tag': ('command(["git", "tag", "v0.4.0"])\n', 'git tag creation'), 'git push': ('command(["git", "push", "origin", "main"])\n', 'git push'), 'GitHub Release': ('command(["gh", "release", "create", "v0.4.0"])\n', 'GitHub Release publication')}
        for label, (source, expected) in prohibited_cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as tmp_dir:
                root = Path(tmp_dir)
                script = root / 'scripts' / 'check_release.py'
                script.parent.mkdir()
                script.write_text(source, encoding='utf-8')
                with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root), self.assertRaisesRegex(ValueError, f'executes prohibited {expected}'):
                    validate_docs_contracts.validate_manual_release_boundary(workflow_paths=(), release_script_paths=(script,))

    # TT-TEST: support
    def test_manual_release_boundary_rejects_workflow_mutation_commands(self) -> None:
        prohibited_cases = {'cargo publish': ('cargo publish --locked', 'cargo publish'), 'cargo toolchain publish': ('cargo +stable publish --locked', 'cargo publish'), 'wrapped env cargo publish': ('env FOO=bar cargo publish --locked', 'cargo publish'), 'wrapped command cargo publish': ('command cargo publish --locked', 'cargo publish'), 'cargo login': ('cargo login', 'cargo registry login'), 'git commit': ('git commit -m automated', 'git commit'), 'git tag': ('git tag v0.4.0', 'git tag creation'), 'git push': ('git push origin main', 'git push'), 'git config tag': ('git -c user.name=bot tag v0.4.0', 'git tag creation'), 'wrapped git push': ('env FOO=bar git push origin main', 'git push'), 'GitHub Release': ('gh release create v0.4.0', 'GitHub Release publication')}
        for label, (command, expected) in prohibited_cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as tmp_dir:
                root = Path(tmp_dir)
                workflow = root / '.github' / 'workflows' / 'release.yml'
                workflow.parent.mkdir(parents=True)
                workflow.write_text(f'steps:\n  - run: {command}\n', encoding='utf-8')
                with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root), self.assertRaisesRegex(ValueError, f'executes prohibited {expected}'):
                    validate_docs_contracts.validate_manual_release_boundary(workflow_paths=(workflow,), release_script_paths=())

    # TT-TEST: support
    def test_manual_release_boundary_rejects_release_actions_and_credentials(self) -> None:
        prohibited_cases = {'release action': ('steps:\n  - uses: softprops/action-gh-release@v2\n', 'invokes GitHub Release automation'), 'registry credentials': ('env:\n  CARGO_REGISTRY_TOKEN: ${{ secrets.CRATES_IO_TOKEN }}\n', 'configures registry publication credentials'), 'registry-specific credentials': ('env:\n  CARGO_REGISTRY_PRIVATE_TOKEN: ${{ secrets.PRIVATE_TOKEN }}\n', 'configures registry publication credentials')}
        for label, (source, expected) in prohibited_cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as tmp_dir:
                root = Path(tmp_dir)
                workflow = root / '.github' / 'workflows' / 'release.yml'
                workflow.parent.mkdir(parents=True)
                workflow.write_text(source, encoding='utf-8')
                with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root), self.assertRaisesRegex(ValueError, expected):
                    validate_docs_contracts.validate_manual_release_boundary(workflow_paths=(workflow,), release_script_paths=())

    # TT-TEST: support
    def test_manual_release_boundary_rejects_contents_write_permissions(self) -> None:
        prohibited_cases = {'workflow': 'permissions:\n  contents: write\njobs: {}\n', 'job': 'jobs:\n  release:\n    permissions:\n      contents: write\n    steps: []\n', 'workflow write-all': 'permissions: write-all\njobs: {}\n', 'job write-all': 'jobs:\n  validate:\n    permissions: write-all\n    steps: []\n'}
        for label, source in prohibited_cases.items():
            with self.subTest(label=label), tempfile.TemporaryDirectory() as tmp_dir:
                root = Path(tmp_dir)
                workflow = root / '.github' / 'workflows' / 'release.yml'
                workflow.parent.mkdir(parents=True)
                workflow.write_text(source, encoding='utf-8')
                with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root), self.assertRaisesRegex(ValueError, 'prohibited (?:contents: write|permissions: write-all) permission'):
                    validate_docs_contracts.validate_manual_release_boundary(workflow_paths=(workflow,), release_script_paths=())

    # TT-TEST: support
    def test_manual_release_boundary_ignores_inert_command_text(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            workflow = root / 'ci.yml'
            script = root / 'check_release.py'
            workflow.write_text("permissions: read-all\nsteps:\n  - run: echo cargo publish --locked\n  - run: printf '%s' 'git push origin main'\n", encoding='utf-8')
            script.write_text('print("cargo publish and gh release create are manual")\n', encoding='utf-8')
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_manual_release_boundary(workflow_paths=(workflow,), release_script_paths=(script,))

    # TT-TEST: support
    def test_diagnostic_benchmark_ci_uses_executable_step(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            workflow = Path(tmp_dir) / 'ci.yml'
            workflow.write_text('jobs:\n  test:\n    steps:\n      - name: benchmark\n        run: python3 scripts/diagnostic_benchmark.py --manifest validation/diagnostics/manifest.json --min-top1 0.75 --min-top2 0.90 --max-high-confidence-wrong 0\n', encoding='utf-8')
            validate_docs_contracts.validate_diagnostic_benchmark_ci_contract(workflow_path=workflow)

    # TT-TEST: M01 secondary
    def test_published_readme_rejects_repository_only_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme = Path(tmp_dir) / 'README.md'
            readme.write_text('See [guide](../docs/user-guide.md).\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'local links must stay inside'):
                validate_docs_contracts.validate_published_crate_readmes_are_self_contained((readme,))

    # TT-TEST: M01 secondary
    def test_published_readme_rejects_repository_only_reference_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme = Path(tmp_dir) / 'README.md'
            readme.write_text('[guide][g]\n\n[g]: ../docs/user-guide.md\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'local links must stay inside'):
                validate_docs_contracts.validate_published_crate_readmes_are_self_contained((readme,))

    # TT-TEST: M01 secondary
    def test_published_readme_accepts_anchor_and_package_local_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            package = Path(tmp_dir) / 'crate'
            package.mkdir()
            readme = package / 'README.md'
            guide = package / 'guide.md'
            readme.write_text('[Section](#section), [guide](guide.md), and [reference][local].\n\n[local]: <guide.md> "Guide"\n', encoding='utf-8')
            guide.write_text('# Guide\n', encoding='utf-8')
            validate_docs_contracts.validate_published_crate_readmes_are_self_contained((readme,))

    # TT-TEST: M01 secondary
    def test_published_readme_requires_each_readme(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            missing = Path(tmp_dir) / 'crate' / 'README.md'
            with self.assertRaisesRegex(ValueError, 'missing'):
                validate_docs_contracts.validate_published_crate_readmes_are_self_contained((missing,))

if __name__ == '__main__':
    unittest.main()
