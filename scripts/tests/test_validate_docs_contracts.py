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
            'tailtriage-controller/src/lib.rs': '''pub struct TailtriageController;
pub struct TailtriageControllerBuilder;
pub struct TailtriageControllerTemplate { pub output_path: PathBuf, pub mode: CaptureMode }
pub struct ControllerActivationTemplate { pub output_path: PathBuf, pub mode: CaptureMode }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeSamplerTemplate { pub enabled: bool, pub mode_override: Option<CaptureMode>, pub interval: Option<Duration>, pub max_runtime_snapshots: Option<usize> }
pub struct ControllerRequestHandle { kind: ControllerRequestKind }
pub struct ControllerQueueTimer<'a> { kind: ControllerQueueTimerKind<'a> }
pub struct ControllerStageTimer<'a> { kind: ControllerStageTimerKind<'a> }
pub struct ControllerInflightGuard<'a> { kind: ControllerInflightGuardKind<'a> }
enum ControllerRequestKind { Active(OwnedRequestHandle), Inert(InertControllerRequestHandle) }
enum ControllerQueueTimerKind<'a> { Active(QueueTimer<'a>), Inert }
enum ControllerStageTimerKind<'a> { Active(StageTimer<'a>), Inert }
enum ControllerInflightGuardKind<'a> { Active(InflightGuard<'a>), Inert }
struct InertControllerRequestHandle;
impl ControllerRequestHandle {
    pub fn is_captured(&self) -> bool { true }
    pub fn captured_handle(&self) -> Option<&OwnedRequestHandle> { None }
}
pub enum UnrelatedPublicEnum { Active, Inert }
impl TailtriageController { pub fn builder() {} }
impl TailtriageControllerBuilder { pub const fn mode(self, mode: CaptureMode) -> Self { self } }
''',
            'tailtriage-tokio/src/lib.rs': 'pub fn builder() {}\n',
            'tailtriage-axum/src/lib.rs': 'pub fn middleware() {}\n',
            'tailtriage-cli/src/lib.rs': '#![doc = include_str!("../README.md")]\n',
            'tailtriage-core/src/config.rs': '',
            'tailtriage-core/src/collector.rs': '',
            'tailtriage-core/src/run_builder.rs': '',
            'tailtriage-core/src/lib.rs': '',
            'tailtriage/src/lib.rs': '',
            'tailtriage-analyzer/src/options/mod.rs': 'pub struct AnalyzeOptionDescriptor { path: &\'static str }\nimpl AnalyzeOptionDescriptor { pub(crate) fn new() {} }\n',
            'tailtriage-analyzer/src/options/overrides.rs': 'fn valid_override_paths() {}\npub(crate) fn valid_override_paths_for_crate() {}\n',
            'tailtriage-analyzer/src/lib.rs': 'pub enum DiagnosisKind { ApplicationQueuePressure, BlockingPoolPressure, ExecutorPressure, DownstreamStageDominance, InsufficientEvidence, }\n',
            'tailtriage-tracing/src/types.rs': '''impl SpanRecord {
    pub fn new() {}
    pub fn with_id(mut self, id: String) -> Self { self }
}
impl SpanRecord
where
    (): Sized,
{
    pub fn id(&self) {}
}
impl ImportOptions {
    pub fn new() {}
    pub fn configured_run_id(&self) {}
    pub fn is_strict(&self) {}
}
impl ImportOptions {}
impl ImportWarning { pub(crate) fn new() {} }
impl ImportedRun { pub(crate) fn new() {} }
''',
            'tailtriage-tracing/src/jsonl.rs': '''enum JsonlParseMode {}
fn import_jsonl_reader_with_mode() {}
fn import_jsonl_path_with_mode() {}
''',
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
    def test_controller_residual_api_cleanup_rejects_removed_surfaces(self) -> None:
        cases = {
            'public ControllerSinkTemplate': 'pub enum ControllerSinkTemplate {}',
        }
        for symbol, source in cases.items():
            with self.subTest(symbol=symbol):
                self._assert_residual_api_rejected('tailtriage-controller/src/lib.rs', source, symbol)

        for type_name, field in (
            ('TailtriageControllerTemplate', 'sink_template'),
            ('ControllerActivationTemplate', 'selected_mode'),
        ):
            with self.subTest(type_name=type_name, field=field):
                source = self._controller_source().replace(
                    f'pub struct {type_name} {{',
                    f'pub struct {type_name} {{ pub {field}: PathBuf,',
                )
                self._assert_residual_api_rejected(
                    'tailtriage-controller/src/lib.rs', source, f'public {field} field'
                )

        self._assert_controller_source_accepted(
            self._controller_source()
            + '\npub struct Unrelated { pub sink_template: PathBuf, pub selected_mode: CaptureMode }\n'
        )

    # TT-TEST: M02 secondary
    def test_controller_builder_public_new_is_scoped_and_const_aware(self) -> None:
        canonical = self._controller_source()
        for declaration in ('pub fn new() {}', 'pub const fn new() {}'):
            with self.subTest(declaration=declaration):
                source = canonical.replace(
                    'impl TailtriageControllerBuilder {',
                    f'impl TailtriageControllerBuilder {{ {declaration}',
                )
                self._assert_residual_api_rejected(
                    'tailtriage-controller/src/lib.rs', source,
                    'TailtriageControllerBuilder::new',
                )

    # TT-TEST: M02 secondary
    def test_controller_unrelated_public_new_and_restricted_builder_new_are_accepted(self) -> None:
        for declaration in ('pub fn new() {}', 'pub const fn new() {}'):
            with self.subTest(declaration=declaration):
                self._assert_controller_source_accepted(
                    self._controller_source() + f'\nstruct Other; impl Other {{ {declaration} }}\n'
                )
        self._assert_controller_source_accepted(
            self._controller_source().replace(
                'impl TailtriageControllerBuilder {',
                'impl TailtriageControllerBuilder { pub(crate) fn new() {}',
            )
        )
        self._assert_controller_source_accepted(
            self._controller_source().replace('pub const fn mode', 'pub fn mode')
        )

    # TT-TEST: M02 secondary
    def test_controller_canonical_methods_must_belong_to_owning_impl(self) -> None:
        cases = (
            ('impl TailtriageController { pub fn builder() {} }',
             'TailtriageController::builder', 'pub fn builder() {}'),
            ('impl TailtriageControllerBuilder { pub const fn mode(self, mode: CaptureMode) -> Self { self } }',
             'TailtriageControllerBuilder::mode', 'pub fn mode() {}'),
        )
        for declaration, symbol, unrelated in cases:
            with self.subTest(symbol=symbol):
                source = self._controller_source().replace(declaration, declaration.split('{')[0] + '{}')
                source += f'\nstruct Other; impl Other {{ {unrelated} }}\n'
                self._assert_residual_api_rejected(
                    'tailtriage-controller/src/lib.rs', source, symbol
                )

    # TT-TEST: M02 secondary
    def test_controller_residual_api_cleanup_accepts_canonical_surface(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root)
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 secondary
    def test_controller_request_wrappers_accept_private_tuple_representation(self) -> None:
        source = self._controller_source()
        for named, tuple_struct in (
            ('pub struct ControllerRequestHandle { kind: ControllerRequestKind }',
             'pub struct ControllerRequestHandle(ControllerRequestKind);'),
            ("pub struct ControllerQueueTimer<'a> { kind: ControllerQueueTimerKind<'a> }",
             "pub struct ControllerQueueTimer<'a>(ControllerQueueTimerKind<'a>);"),
            ("pub struct ControllerStageTimer<'a> { kind: ControllerStageTimerKind<'a> }",
             "pub struct ControllerStageTimer<'a>(ControllerStageTimerKind<'a>);"),
            ("pub struct ControllerInflightGuard<'a> { kind: ControllerInflightGuardKind<'a> }",
             "pub struct ControllerInflightGuard<'a>(ControllerInflightGuardKind<'a>);"),
        ):
            source = source.replace(named, tuple_struct)
        self._assert_controller_source_accepted(source)

    # TT-TEST: M02 secondary
    def test_controller_request_wrappers_must_be_opaque_public_structs(self) -> None:
        canonical = self._controller_source()
        declarations = (
            ("ControllerRequestHandle", "pub struct ControllerRequestHandle { kind: ControllerRequestKind }", "pub enum ControllerRequestHandle { Active(OwnedRequestHandle), Inert(InertControllerRequestHandle) }"),
            ("ControllerQueueTimer", "pub struct ControllerQueueTimer<'a> { kind: ControllerQueueTimerKind<'a> }", "pub enum ControllerQueueTimer<'a> { Active(QueueTimer<'a>), Inert }"),
            ("ControllerStageTimer", "pub struct ControllerStageTimer<'a> { kind: ControllerStageTimerKind<'a> }", "pub enum ControllerStageTimer<'a> { Active(StageTimer<'a>), Inert }"),
            ("ControllerInflightGuard", "pub struct ControllerInflightGuard<'a> { kind: ControllerInflightGuardKind<'a> }", "pub enum ControllerInflightGuard<'a> { Active(InflightGuard<'a>), Inert }"),
        )
        for name, current, exposed in declarations:
            with self.subTest(name=name):
                self._assert_residual_api_rejected(
                    'tailtriage-controller/src/lib.rs',
                    canonical.replace(current, exposed),
                    f'public enum {name}',
                )

        cases = (
            (canonical.replace('struct InertControllerRequestHandle;', 'pub struct InertControllerRequestHandle;'), 'public InertControllerRequestHandle'),
            (canonical.replace('kind: ControllerRequestKind', 'pub kind: ControllerRequestKind', 1), 'public representation field on ControllerRequestHandle'),
            (canonical.replace('kind: ControllerRequestKind', 'pub(crate) kind: ControllerRequestKind', 1), 'public representation field on ControllerRequestHandle'),
            (canonical.replace('pub struct ControllerRequestHandle { kind: ControllerRequestKind }', 'pub struct ControllerRequestHandle(pub ControllerRequestKind);'), 'public tuple representation field on ControllerRequestHandle'),
            (canonical.replace("pub struct ControllerQueueTimer<'a> { kind: ControllerQueueTimerKind<'a> }", "pub struct ControllerQueueTimer<'a>(pub ControllerQueueTimerKind<'a>);"), 'public tuple representation field on ControllerQueueTimer'),
            (canonical.replace('pub struct ControllerRequestHandle { kind: ControllerRequestKind }', 'pub struct ControllerRequestHandle(pub(crate) ControllerRequestKind);'), 'public tuple representation field on ControllerRequestHandle'),
            (canonical.replace('    pub fn is_captured(&self) -> bool { true }\n', ''), 'ControllerRequestHandle::is_captured'),
            (canonical.replace('    pub fn captured_handle(&self) -> Option<&OwnedRequestHandle> { None }\n', ''), 'ControllerRequestHandle::captured_handle'),
            (canonical.replace('Option<&OwnedRequestHandle>', 'Option<OwnedRequestHandle>'), 'ControllerRequestHandle::captured_handle'),
        )
        for source, expected in cases:
            with self.subTest(expected=expected):
                self._assert_residual_api_rejected(
                    'tailtriage-controller/src/lib.rs', source, expected
                )

    # TT-TEST: M02 secondary
    def test_runtime_sampler_template_rejects_removed_fields_and_serde(self) -> None:
        canonical = self._controller_source()
        cases = (
            (canonical.replace('pub enabled: bool', 'pub enabled: bool, pub enabled_for_armed_runs: bool'), 'RuntimeSamplerTemplate::enabled_for_armed_runs'),
            (canonical.replace('pub interval: Option<Duration>', 'pub interval: Option<Duration>, pub interval_ms: Option<u64>'), 'RuntimeSamplerTemplate::interval_ms'),
            (canonical.replace('derive(Debug, Clone, Copy, PartialEq, Eq, Default)', 'derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize)'), 'RuntimeSamplerTemplate Serialize'),
            (canonical.replace('derive(Debug, Clone, Copy, PartialEq, Eq, Default)', 'derive(Debug, Clone, Copy, PartialEq, Eq, Default, Deserialize)'), 'RuntimeSamplerTemplate Deserialize'),
            (canonical + '\nimpl Serialize for RuntimeSamplerTemplate {}\n', 'RuntimeSamplerTemplate Serialize'),
            (canonical + "\nimpl<'de> Deserialize<'de> for RuntimeSamplerTemplate {}\n", 'RuntimeSamplerTemplate Deserialize'),
        )
        for source, expected in cases:
            with self.subTest(expected=expected):
                self._assert_residual_api_rejected('tailtriage-controller/src/lib.rs', source, expected)

    # TT-TEST: M02 secondary
    def test_runtime_sampler_template_accepts_private_toml_serde_and_unrelated_milliseconds(self) -> None:
        self._assert_controller_source_accepted(
            self._controller_source()
            + '\npub struct Unrelated { pub interval_ms: Option<u64> }\n'
            + '#[derive(Deserialize)] struct RuntimeSamplerConfigToml { interval_ms: Option<u64> }\n'
            + '#[derive(Serialize)] struct TestRuntimeSamplerToml { interval_ms: u64 }\n'
        )

    def _controller_source(self) -> str:
        return '''pub struct TailtriageController;
pub struct TailtriageControllerBuilder;
pub struct TailtriageControllerTemplate { pub output_path: PathBuf, pub mode: CaptureMode }
pub struct ControllerActivationTemplate { pub output_path: PathBuf, pub mode: CaptureMode }
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RuntimeSamplerTemplate { pub enabled: bool, pub mode_override: Option<CaptureMode>, pub interval: Option<Duration>, pub max_runtime_snapshots: Option<usize> }
pub struct ControllerRequestHandle { kind: ControllerRequestKind }
pub struct ControllerQueueTimer<'a> { kind: ControllerQueueTimerKind<'a> }
pub struct ControllerStageTimer<'a> { kind: ControllerStageTimerKind<'a> }
pub struct ControllerInflightGuard<'a> { kind: ControllerInflightGuardKind<'a> }
enum ControllerRequestKind { Active(OwnedRequestHandle), Inert(InertControllerRequestHandle) }
enum ControllerQueueTimerKind<'a> { Active(QueueTimer<'a>), Inert }
enum ControllerStageTimerKind<'a> { Active(StageTimer<'a>), Inert }
enum ControllerInflightGuardKind<'a> { Active(InflightGuard<'a>), Inert }
struct InertControllerRequestHandle;
impl ControllerRequestHandle {
    pub fn is_captured(&self) -> bool { true }
    pub fn captured_handle(&self) -> Option<&OwnedRequestHandle> { None }
}
pub enum UnrelatedPublicEnum { Active, Inert }
impl TailtriageController { pub fn builder() {} }
impl TailtriageControllerBuilder { pub const fn mode(self, mode: CaptureMode) -> Self { self } }
'''

    def _assert_controller_source_accepted(self, source: str) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(
                root, {'tailtriage-controller/src/lib.rs': source}
            )
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_owned_request_method(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/collector.rs', 'pub fn begin_request_owned(&self) {}', 'Tailtriage::begin_request_owned')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_tracing_surface(self) -> None:
        cases = (
            ('impl SpanRecord { pub fn id(mut self, value: String) -> Self { self } }', 'SpanRecord::id consuming setter'),
            ('impl SpanRecord { pub fn id_ref(&self) {} }', 'SpanRecord::id_ref'),
            ('impl ImportOptions { pub fn strict_mode(&self) {} }', 'ImportOptions::strict_mode'),
            ('impl ImportWarning { pub fn new() {} }', 'ImportWarning::new'),
            ('impl ImportedRun { pub fn new() {} }', 'ImportedRun::new'),
        )
        suffix = '\nimpl SpanRecord {}\nimpl ImportOptions {}\nimpl ImportWarning {}\nimpl ImportedRun {}\n'
        for declaration, symbol in cases:
            with self.subTest(symbol=symbol):
                self._assert_residual_api_rejected(
                    'tailtriage-tracing/src/types.rs', declaration + suffix, symbol
                )

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_tracing_surface_in_later_impls(self) -> None:
        cases = (
            ('''impl SpanRecord { pub fn id(&self) {} }
impl SpanRecord { pub fn id(mut self, value: String) -> Self { self } }
impl ImportOptions {}
impl ImportWarning {}
impl ImportedRun {}
''', 'SpanRecord::id consuming setter'),
            ('''impl SpanRecord {}
impl ImportOptions { pub fn configured_run_id(&self) {} }
impl ImportOptions { pub fn strict_mode(&self) -> bool { false } }
impl ImportWarning {}
impl ImportedRun {}
''', 'ImportOptions::strict_mode'),
        )
        for source, symbol in cases:
            with self.subTest(symbol=symbol):
                self._assert_residual_api_rejected(
                    'tailtriage-tracing/src/types.rs', source, symbol
                )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_accepts_canonical_explicit_set(self) -> None:
        self._assert_facade_sources_accepted(
            'pub use artifact::{Run, RunMetadata};\npub use sink::{RunSink, SinkError};\n'
            '#[doc(hidden)]\npub mod __internal {}\n',
            'pub use tailtriage_core::{Run, RunMetadata, RunSink, SinkError};\n',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_accepts_single_explicit_item(self) -> None:
        self._assert_facade_sources_accepted(
            'pub use artifact::Run;\n', 'pub use tailtriage_core::Run;\n'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_single_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::Run as RunMetadata;\n', 'must not alias'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_equal_name_set_alias_swap(self) -> None:
        self._assert_facade_sources_rejected(
            'pub use artifact::{Run, RunMetadata};\n',
            'pub use tailtriage_core::{\n'
            '    Run as RunMetadata,\n'
            '    RunMetadata as Run,\n'
            '};\n',
            'must not alias',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_braced_whole_crate(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use {tailtriage_core};\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_braced_whole_crate_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use {tailtriage_core as core};\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_braced_whole_crate_internal_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use {tailtriage_core as __internal};\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_mixed_braced_whole_crate(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use {tailtriage_core, some_other_crate::Thing};\n',
            'whole tailtriage_core',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_glob(self) -> None:
        self._assert_facade_sources_rejected(
            'pub use artifact::{Run};\n', 'pub use tailtriage_core :: * ;\n', 'glob-reexport'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_whole_crate_reexport(self) -> None:
        self._assert_facade_bypass_rejected('pub use tailtriage_core;\n', 'whole tailtriage_core')

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_whole_crate_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core as core;\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_whole_crate_internal_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core as __internal;\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_group_self(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::{self, Run};\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_group_aliased_self(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::{self as core, Run};\n', 'whole tailtriage_core'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_nested_internal_glob(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::__internal::*;\n', 'glob-reexport'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_non_internal_nested_glob(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::some_public_module::*;\n', 'glob-reexport'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_explicit_internal_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::__internal as internal;\n', '__internal'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_grouped_internal_alias(self) -> None:
        self._assert_facade_bypass_rejected(
            'pub use tailtriage_core::{Run, __internal as internal};\n', '__internal'
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_internal_exposure(self) -> None:
        core = 'pub use artifact::{Run};\n#[doc(hidden)]\npub mod __internal {}\n'
        for facade in (
            'pub use tailtriage_core::{Run, __internal};\n',
            'pub use tailtriage_core::{Run};\npub mod __internal;\n',
        ):
            with self.subTest(facade=facade):
                self._assert_facade_sources_rejected(core, facade, '__internal')

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_missing_supported_item(self) -> None:
        self._assert_facade_sources_rejected(
            'pub use artifact::{Run, RunMetadata};\n',
            'pub use tailtriage_core::{Run};\n',
            'missing=',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_rejects_extra_item(self) -> None:
        self._assert_facade_sources_rejected(
            'pub use artifact::{Run};\n',
            'pub use tailtriage_core::{Run, Unsupported};\n',
            'extra=',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_ignores_order_grouping_and_whitespace(self) -> None:
        self._assert_facade_sources_accepted(
            'pub use artifact::{Run, RunMetadata};\npub use sink::RunSink;\n',
            'pub use tailtriage_core :: {\n RunSink,\nRunMetadata , Run\n};\n',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_accepts_doc_hidden_core_internal_module(self) -> None:
        self._assert_facade_sources_accepted(
            'pub use artifact::Run;\n#[doc(hidden)]\npub mod __internal {}\n',
            'pub use tailtriage_core::{Run};\n',
        )

    # TT-TEST: M02 secondary
    def test_facade_core_reexport_policy_accepts_private_core_use(self) -> None:
        self._assert_facade_sources_accepted(
            'pub use artifact::Run;\n',
            'pub use tailtriage_core::Run;\nuse tailtriage_core::__internal as internal;\n',
        )

    # TT-TEST: M02 secondary
    def test_removed_jsonl_parse_mode_public_enum_is_rejected(self) -> None:
        self._assert_residual_api_rejected(
            'tailtriage-tracing/src/jsonl.rs', 'pub enum JsonlParseMode {}\n', 'JsonlParseMode'
        )

    # TT-TEST: M02 secondary
    def test_removed_jsonl_reader_with_mode_public_function_is_rejected(self) -> None:
        self._assert_residual_api_rejected(
            'tailtriage-tracing/src/jsonl.rs',
            'pub fn import_jsonl_reader_with_mode<R: Read>(reader: R) {}\n',
            'import_jsonl_reader_with_mode',
        )

    # TT-TEST: M02 secondary
    def test_removed_jsonl_path_with_mode_public_function_is_rejected(self) -> None:
        self._assert_residual_api_rejected(
            'tailtriage-tracing/src/jsonl.rs',
            'pub fn import_jsonl_path_with_mode(path: &Path) {}\n',
            'import_jsonl_path_with_mode',
        )

    # TT-TEST: M02 secondary
    def test_removed_jsonl_compatibility_names_are_accepted_when_private(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root)
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    def _assert_facade_sources_accepted(self, core: str, facade: str) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root, {
                'tailtriage-core/src/lib.rs': core,
                'tailtriage/src/lib.rs': facade,
            })
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    def _assert_facade_sources_rejected(self, core: str, facade: str, error: str) -> None:
        with self.assertRaisesRegex(ValueError, error):
            self._assert_facade_sources_accepted(core, facade)

    def _assert_facade_bypass_rejected(self, bypass: str, error: str) -> None:
        core = 'pub use artifact::{Run, RunMetadata};\n'
        facade = 'pub use tailtriage_core::{Run, RunMetadata};\n' + bypass
        self._assert_facade_sources_rejected(core, facade, error)

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_tracing_surface_in_where_impl(self) -> None:
        source = '''impl SpanRecord {}
impl SpanRecord
where
    (): Sized,
{
    pub fn id(mut self, value: String) -> Self { self }
}
impl ImportOptions {}
impl ImportWarning {}
impl ImportedRun {}
'''
        self._assert_residual_api_rejected(
            'tailtriage-tracing/src/types.rs', source, 'SpanRecord::id consuming setter'
        )

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_const_removed_tracing_surface(self) -> None:
        source = '''impl SpanRecord {}
impl ImportOptions { pub const fn strict_mode(&self) -> bool { false } }
impl ImportWarning {}
impl ImportedRun {}
'''
        self._assert_residual_api_rejected(
            'tailtriage-tracing/src/types.rs', source, 'ImportOptions::strict_mode'
        )

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_accepts_canonical_tracing_surface(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root)
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_run_builder_finish(self) -> None:
        self._assert_residual_api_rejected('tailtriage-core/src/run_builder.rs', 'pub fn finish(self) -> Run { todo!() }', 'RunBuilder::finish')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_tokio_request_helpers(self) -> None:
        for symbol, source in (
            ('TokioRequestHandleExt::inflight_guard', 'pub trait TokioRequestHandleExt { fn inflight_guard(&self); }'),
            ('public InstrumentedSemaphore', 'pub struct InstrumentedSemaphore;'),
            ('public InstrumentedOwnedSemaphore', 'pub struct InstrumentedOwnedSemaphore;'),
        ):
            with self.subTest(symbol=symbol):
                self._assert_residual_api_rejected('tailtriage-tokio/src/lib.rs', source, symbol)

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_accepts_direct_tokio_semaphore_futures(self) -> None:
        source = '''pub trait TokioRequestHandleExt {
    fn semaphore<'req, 'sem>(&'req self, semaphore: &'sem Semaphore)
        -> impl Future<Output = Result<SemaphorePermit<'sem>, AcquireError>> + 'req
        where 'sem: 'req;
    fn owned_semaphore(&self, semaphore: Arc<Semaphore>)
        -> impl Future<Output = Result<OwnedSemaphorePermit, AcquireError>> + '_;
}
'''
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(root, {'tailtriage-tokio/src/lib.rs': source})
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_analyzer_methods(self) -> None:
        for method in ('with_queueing', 'with_blocking', 'with_executor', 'with_downstream',
                       'with_confidence', 'with_evidence', 'with_route', 'with_temporal'):
            with self.subTest(method=method):
                self._assert_residual_api_rejected('tailtriage-analyzer/src/options/mod.rs', f'pub fn {method}() {{}}', f'AnalyzeOptions::{method}')

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_removed_diagnosis_variants(self) -> None:
        for variant, declaration in (
            ('ApplicationQueueSaturation', 'ApplicationQueueSaturation,'),
            ('ApplicationQueueSaturation', 'ApplicationQueueSaturation'),
            ('ApplicationQueueSaturation', 'ApplicationQueueSaturation = 7,'),
            ('ExecutorPressureSuspected', 'ExecutorPressureSuspected(u8),'),
            ('DownstreamStageDominates', 'DownstreamStageDominates { source: u8 },'),
            ('ApplicationQueueSaturation', 'ApplicationQueueSaturation, // legacy spelling'),
        ):
            with self.subTest(declaration=declaration):
                source = f'pub enum DiagnosisKind {{\n    ApplicationQueuePressure,\n    {declaration}\n}}\n'
                self._assert_residual_api_rejected(
                    'tailtriage-analyzer/src/lib.rs', source, f'DiagnosisKind::{variant}'
                )

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_accepts_diagnosis_variant_boundaries(self) -> None:
        source = '''// Historical migration note: ApplicationQueueSaturation was renamed.
pub enum DiagnosisKind {
    ApplicationQueuePressure,
    BlockingPoolPressure,
    ExecutorPressure,
    DownstreamStageDominance,
    InsufficientEvidence,
    ApplicationQueueSaturationDetails,
}
'''
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            self._write_residual_api_sources(
                root, {'tailtriage-analyzer/src/lib.rs': source}
            )
            with mock.patch.object(validate_docs_contracts, 'REPO_ROOT', root):
                validate_docs_contracts.validate_residual_public_api_cleanup()

    # TT-TEST: M02 secondary
    def test_residual_public_api_cleanup_rejects_valid_override_paths_in_overrides(self) -> None:
        self._assert_residual_api_rejected(
            'tailtriage-analyzer/src/options/overrides.rs',
            'pub fn valid_override_paths() {}',
            'AnalyzeOptions::valid_override_paths',
        )

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

    # TT-TEST: M01 secondary
    def test_controller_toml_contract_accepts_flat_output_and_optional_mode(self) -> None:
        validate_docs_contracts._validate_controller_toml_shape(
            parsed={"controller": {"service_name": "checkout", "activation": {"output_path": "run.json", "run_end_policy": "auto_seal_on_limits_hit"}}},
            example_name="synthetic",
        )

    # TT-TEST: M01 secondary
    def test_controller_toml_contract_accepts_runtime_sampler_fields(self) -> None:
        validate_docs_contracts._validate_controller_toml_shape(
            parsed={"controller": {"service_name": "checkout", "activation": {
                "output_path": "run.json", "runtime_sampler": {"enabled": True,
                "mode_override": "investigation", "interval_ms": 250,
                "max_runtime_snapshots": 34}}}}, example_name="synthetic",
        )

    # TT-TEST: M01 secondary
    def test_controller_toml_contract_rejects_removed_or_mistyped_sampler_fields(self) -> None:
        cases = (
            ({"enabled_for_armed_runs": True}, "enabled_for_armed_runs"),
            ({"enabled": "true"}, "enabled must be a bool"),
            ({"enabled": True, "mode_override": "deep"}, "mode_override is invalid"),
            ({"enabled": True, "interval_ms": "250"}, "interval_ms must be an integer"),
            ({"enabled": True, "max_runtime_snapshots": False}, "max_runtime_snapshots must be an integer"),
        )
        for sampler, expected in cases:
            with self.subTest(sampler=sampler), self.assertRaisesRegex(ValueError, expected):
                validate_docs_contracts._validate_controller_toml_shape(
                    parsed={"controller": {"service_name": "checkout", "activation": {
                        "output_path": "run.json", "runtime_sampler": sampler}}},
                    example_name="synthetic",
                )

    # TT-TEST: M01 secondary
    def test_controller_toml_contract_rejects_removed_nested_forms(self) -> None:
        cases = (
            {"output_path": "run.json", "sink": {"type": "local_json", "output_path": "old.json"}},
            {"output_path": "run.json", "run_end_policy": {"kind": "auto_seal_on_limits_hit"}},
        )
        for activation in cases:
            with self.subTest(activation=activation), self.assertRaisesRegex(ValueError, "nested controller.activation.sink|run_end_policy must be a string"):
                validate_docs_contracts._validate_controller_toml_shape(
                    parsed={"controller": {"service_name": "checkout", "activation": activation}},
                    example_name="synthetic",
                )

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

    # TT-TEST: support
    def test_public_markdown_links_accept_valid_relative_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'docs' / 'guide.md'
            source.parent.mkdir()
            source.write_text('[Details](details.md)\n', encoding='utf-8')
            (source.parent / 'details.md').write_text('# Details\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_reject_missing_local_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('[Missing](missing.md)\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'missing local file'):
                validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_accept_valid_local_toml_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('[Config](examples/analyzer-config.toml)\n', encoding='utf-8')
            target = root / 'examples' / 'analyzer-config.toml'
            target.parent.mkdir()
            target.write_text('[analyzer]\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_reject_missing_local_toml_file(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('[Config](examples/analyzer-config.toml)\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'missing local file'):
                validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_accept_valid_same_file_fragment(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('# Next Check\n\n[Jump](#next-check)\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_accept_valid_cross_file_fragment(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            target = root / 'details.md'
            source.write_text('[Jump](details.md#next-check)\n', encoding='utf-8')
            target.write_text('# Next Check\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_reject_missing_fragment(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('# Existing\n\n[Jump](#missing)\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'missing heading fragment'):
                validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_ignore_external_url(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('[External](https://example.com/missing.md#missing)\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_accept_duplicate_heading_suffix(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('# Check\n\n## Check\n\n[Second](#check-1)\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_ignore_heading_inside_fenced_code(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text(
                '```bash\n# Not a real heading\n```\n\n[Jump](#not-a-real-heading)\n',
                encoding='utf-8',
            )
            with self.assertRaisesRegex(ValueError, 'missing heading fragment'):
                validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

    # TT-TEST: support
    def test_public_markdown_links_ignore_link_inside_fenced_code(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            root = Path(tmp_dir)
            source = root / 'guide.md'
            source.write_text('```text\n[Example](missing.md)\n```\n', encoding='utf-8')
            validate_docs_contracts.validate_public_markdown_links(documents=(source,), repo_root=root)

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
    def test_docs_index_contract_checks_maintainer_gateway_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            repo_root = Path(tmp_dir)
            docs_dir = repo_root / 'docs'
            docs_dir.mkdir(parents=True)
            (repo_root / 'README.md').write_text('# Root\n', encoding='utf-8')
            docs_index_path = docs_dir / 'README.md'
            docs_index_path.write_text('[Root](../README.md)\n[Maintainers](dev/README.md)\n', encoding='utf-8')
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
    def test_published_readme_rejects_external_documentation_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme = Path(tmp_dir) / 'README.md'
            readme.write_text('See [hosted guide](https://example.com/guide).\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'no external documentation links'):
                validate_docs_contracts.validate_published_crate_readmes_are_self_contained((readme,))

    # TT-TEST: M01 secondary
    def test_published_readme_rejects_external_reference_link(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme = Path(tmp_dir) / 'README.md'
            readme.write_text('[guide][g]\n\n[g]: https://example.com/guide\n', encoding='utf-8')
            with self.assertRaisesRegex(ValueError, 'no external documentation links'):
                validate_docs_contracts.validate_published_crate_readmes_are_self_contained((readme,))

    # TT-TEST: M01 secondary
    def test_published_readme_rejects_external_http_autolink(self) -> None:
        for scheme in ('http', 'https'):
            with self.subTest(scheme=scheme), tempfile.TemporaryDirectory() as tmp_dir:
                readme = Path(tmp_dir) / 'README.md'
                readme.write_text(f'See <{scheme}://example.com/guide>.\n', encoding='utf-8')
                with self.assertRaisesRegex(ValueError, 'no external documentation links'):
                    validate_docs_contracts.validate_published_crate_readmes_are_self_contained(
                        (readme,)
                    )

    # TT-TEST: M01 secondary
    def test_published_readme_accepts_literal_http_url_text(self) -> None:
        with tempfile.TemporaryDirectory() as tmp_dir:
            readme = Path(tmp_dir) / 'README.md'
            readme.write_text('Sample value: https://example.com/guide\n', encoding='utf-8')
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
