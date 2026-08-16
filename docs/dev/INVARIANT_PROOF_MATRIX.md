# Tailtriage 0.4 invariant proof matrix

This is the repository-owned decision record for Phase 27A. It maps the frozen 0.4 product
contract to its current proof owners and proposes, but does not perform, later consolidation.
Suspects remain triage leads, not proof of root cause. A secondary proof is retained whenever it
crosses a genuinely different API, package, serialization, process, live, operational, or release
boundary.

The audit baseline is commit `490a12f92544251b6bb414b3c54b3914967a4437`. No test, fixture,
production source, script, manifest, workflow, or golden was changed while producing this matrix.

## Reading the matrix

- **Cadence:** `gate` is in the repository completion gate, `CI` is explicitly run by
  `.github/workflows/ci.yml`, `manual` is local/release/operator initiated, and `snapshot` is the
  manually dispatched validation-snapshot workflow.
- **Diagnostics:** `high` names the failing invariant/case directly; `medium` usually reports a
  scenario, command, or broad contract; `low` is primarily a compile/existence signal.
- **Decision:** `CONSOLIDATE`, `REMOVE`, `REPLACE`, and `ADD MISSING PROOF` are review proposals for
  27A2, not changes made in 27A1. Uncertain overlap defaults to `KEEP`.
- Exact fixture lineage and mutation ownership remain authoritative in
  [FIXTURE_LINEAGE.md](FIXTURE_LINEAGE.md).

## Invariant ownership

| ID | Invariant / contract | Source owner | Primary proof owner | Secondary boundary proof | Proof class; cadence | Fixtures used | Failure diagnostics | Decision | Rationale / evidence |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| C01 | Borrowed request start, explicit finish, outcome, and one completed request event | `tailtriage-core/src/lib.rs`, `collector.rs` | `tailtriage-core/src/tests.rs::started_request_records_request_event`, `finish_records_outcome` | `tailtriage-analyzer/tests/end_to_end_capture_analysis.rs::queue_and_stage_data_drives_ranked_suspects` | unit + integration; gate/CI | none | high | KEEP | Integration crosses capture-to-analysis; it does not replace lifecycle unit proof. |
| C02 | Owned and fractured-code request handles preserve the same lifecycle | `tailtriage-core/src/lib.rs` | `owned_started_request_maps_result_to_request_outcome`, `request_handle_supports_fractured_code_usage` | Tokio owned/borrowed helper tests | API + boundary; gate/CI | none | high | KEEP | Borrowed and owned surfaces are frozen, distinct API boundaries. |
| C03 | Dropped completion records cancellation without panic, including unwind | `tailtriage-core/src/lib.rs` | `borrowed_completion_drop_records_cancelled_request`, `owned_completion_drop_records_cancelled_request`, `completion_drop_during_unwind_is_non_panicking_and_cancelled` | controller `dropped_controller_completion_drains_closing_generation_as_cancelled` | unit + integration; gate/CI | none | high | KEEP | Controller adds a generation-drain boundary. |
| C04 | Explicit finish disarms cancellation and cannot duplicate completion | `tailtriage-core/src/lib.rs` | `explicit_finish_disarms_drop_without_duplicate_cancelled_event` | controller `completion_drain_finalizes_once_without_duplicate_side_effects` | unit + boundary; gate/CI | none | high | KEEP | Different lifecycle owners and failure modes. |
| C05 | Finalization is one-way; late completion/helper mutation is inert | `tailtriage-core/src/lib.rs`, `collector.rs` | `sink_failure_is_terminal_and_late_mutations_are_inert`, `armed_stage_and_queue_drop_after_finalization_leave_persisted_run_unchanged`, `shutdown_before_inflight_drop_keeps_late_drop_inert` | controller terminal-failure replay tests | boundary; gate/CI | none | high | KEEP | Directly owns the requested finalization-immutability audit. |
| C06 | Shutdown normalizes unfinished child evidence without fabricating request completion | `tailtriage-core/src/collector.rs`, `validation.rs` | `shutdown_normalizes_unfinished_request_children_without_fabricating_completion` | diagnostic corpus `cancelled-request-with-partial-child.json` | unit + serialization/integration; gate/CI | named corpus Run | high | KEEP | Typed lifecycle and corpus report behavior are different boundaries. |
| C07 | Strict lifecycle failure is side-effect free and retryable | `tailtriage-core/src/collector.rs` | `strict_shutdown_with_pending_is_side_effect_free_and_retryable_after_drop` | controller `drain_finalization_strict_lifecycle_failure_is_observable_and_retriable` | boundary + integration; gate/CI | none | high | KEEP | Controller proof includes drain and surfaced generation result. |
| C08 | Finalized Run schema has one finalization timestamp and schema-v2 shape | `tailtriage-core/src/events.rs`, `collector.rs` | `shutdown_sets_one_finalization_timestamp`, `serialized_completed_run_uses_schema_v2_finalization_shape` | controller `controller_output_is_finalized_schema_v2`; CLI artifact loader tests | serialization/golden + boundary; gate/CI | inline JSON | high | KEEP | Serialization, controller output, and saved-file policy are distinct. |
| V01 | Strict Run validation rejects integrity errors with stable codes | `tailtriage-core/src/validation.rs` | validation tests in `tailtriage-core/src/tests.rs` including `issue_code_labels_are_stable` | CLI strict-default tests in `cli_boundary.rs` | unit + boundary; gate/CI | inline Runs/JSON | high | KEEP | Library validation and process policy must both survive. |
| V02 | Permissive normalization deterministically excludes/repairs only documented evidence | `tailtriage-core/src/validation.rs` | `duplicates_are_all_excluded_with_children`, `timing_errors_clear_offsets_but_retain_duration_authority`, orphan/parent-state tests | `tailtriage-cli/tests/json_parity.rs::canonical_run_integrity_equivalence_matrix_across_entries` | exhaustive small-domain + equivalence; gate/CI | inline matrices | high | KEEP | The seven-case cross-entry matrix is a meaningful composition boundary. |
| V03 | Strict and permissive policies intentionally diverge | core validation plus CLI/analyzer policy owners | `cli_loader_rejects_empty_requests_but_analyzer_accepts_zero_request_run` and strict/permissive pairs in `cli_boundary.rs` | docs governance contract tests | boundary + documentation; gate/CI | inline JSON | high | KEEP | Prevents conflating permissive library diagnosis with saved-artifact policy. |
| V04 | Historical optional `worker_count` remains wire compatible; invalid zero is strict-rejected/permissive-cleared | `tailtriage-core/src/events.rs`, `validation.rs` | `runtime_snapshot_worker_count_serde_is_optional_and_schema_v2_compatible`, `invalid_worker_count_is_rejected_strictly_and_cleared_permissively` | analyzer historical worker fallback tests | serialization + unit; gate/CI | inline Runs | high | KEEP | Schema compatibility and scoring fallback are distinct. |
| K01 | Core capture limits, exact drop counts, warnings, and saturation are bounded | core collector/retention | `capture_limits_apply_to_all_sections`, `saturation_preserves_exact_drop_counts_across_sections`, `shutdown_artifact_includes_post_saturation_drops` | collector operational validation | unit + operational; gate/CI | generated operational output | high/medium | KEEP | Operational proof characterizes visible drops; it is not a no-drops claim. |
| K02 | Partial stage/queue evidence records only after polling, completed distributions remain separate | core timers/events | partial helper and serde tests in `tailtriage-core/src/tests.rs` | analyzer partial-evidence tests and diagnostic corpus | unit + serialization + integration; gate/CI | five partial/cancelled corpus Runs | high | KEEP | Each boundary proves capture, wire, and interpretation separately. |
| G01 | Enable/disable/re-enable creates isolated generations and one active generation | `tailtriage-controller/src/lib.rs` | `enable_disable_reenable_creates_distinct_generation_and_artifact`, `one_active_generation_at_a_time` | rapid-boundary isolation test | integration; gate/CI | none | high | KEEP | Central controller generation invariant. |
| G02 | Requests remain bound to their original generation across disable/re-enable | controller | `request_completion_remains_bound_to_original_generation_after_reenable` | `request_started_before_disable_can_finish_after_disable` | boundary; gate/CI | none | high | KEEP | The first owns rollover identity; the second owns closing-generation admission/drain. |
| G03 | Closing generation drains once; cancellation and sink/strict errors remain observable | controller | `completion_drain_finalizes_once_without_duplicate_side_effects` and drain failure tests | core finalization tests | integration; gate/CI | none | high | KEEP | Core cannot prove generation result replay/drain. |
| G04 | Reload is transactional, affects future generations only, and creates no generation/sampler | controller | `reload_updates_next_activation_template_only`, invalid reload tests, `sampler_template_reload_is_side_effect_free_until_enable` | TOML builder/reload resolution parity test | integration; gate/CI | temporary TOML | high | KEEP | Programmatic and TOML configuration are both frozen. |
| G05 | Config precedence, sparse defaults, parse errors, and initial activation are stable | controller | named TOML/config tests in `tailtriage-controller/src/lib.rs` | controller public examples compiled as targets | unit + API; gate/CI | temporary TOML | high/low | KEEP | Examples prove teachable compile surface, not semantic precedence. |
| T01 | Tokio queue, lock, semaphore, channel helpers preserve values/lifetimes and record queue only | `tailtriage-tokio/src/lib.rs` | `queue_and_lock_helpers_record_queue_only`, lifetime and pending-drop helper tests | core queue timer tests | API + boundary; gate/CI | none | high | KEEP | Tokio primitive boundary is not duplicated by core timers. |
| T02 | Tokio stage, timeout, blocking and in-flight helpers preserve results/laziness and do not finish requests | Tokio | `stage_helpers_and_inflight_behave_and_preserve_results`, lazy/timeout/drop tests | core stage/inflight tests | API + boundary; gate/CI | none | high | KEEP | Public extension behavior is Tokio-specific. |
| T03 | Sampler builder validates interval/runtime and prevents duplicate registration | Tokio sampler | `runtime_sampler_rejects_zero_interval`, `runtime_sampler_requires_active_runtime`, `runtime_sampler_rejects_duplicate_start_for_same_run` | controller initial-enable missing-runtime test | API + integration; gate/CI | none | high | KEEP | Confirms the builder-based canonical startup without restoring removed `start(run, interval)`. |
| T04 | Sampler mode inheritance, explicit override precedence, cap, and metadata are stable | Tokio sampler | inheritance/override/cap tests in `tailtriage-tokio/src/lib.rs` | controller effective-metadata tests | unit + integration; gate/CI | none | high | KEEP | Direct and controller-configured sampler boundaries differ. |
| T05 | Runtime snapshots preserve unavailable optional metrics and configured worker count | Tokio sampler | `unavailable_runtime_metrics_are_recorded_as_none`, `configured_multithread_worker_counts_are_captured` | core worker-count serde and analyzer fallback tests | live + serialization; gate/CI | none | high | KEEP | Live Tokio evidence connects wire compatibility to analysis. |
| X01 | Axum middleware injects a request handle and finishes from response status | `tailtriage-axum/src/lib.rs` | `tailtriage-axum/tests/axum_adapter.rs::middleware_injects_request_handle_and_finishes_from_response_status` | Axum examples compile | integration + API; gate/CI | none | high/low | KEEP | Runtime behavior and package example compilation are distinct. |
| X02 | Default and configurable HTTP outcome classification are stable | Axum | `default_status_mapping_matches_http_contract`, adapter default/custom classifier tests | none | unit + integration; gate/CI | none | high | KEEP | Unit table and middleware wiring are distinct boundaries. |
| R01 | JSONL accepts only the stable completed-span wrapper and reports structural line errors | `tailtriage-tracing/src/jsonl.rs` | named parser rejection/acceptance tests in that module | CLI wrapper-mode tests | unit + process boundary; gate/CI | `tailtriage-span-v1.jsonl` | high | KEEP | CLI guidance and file handling cannot replace parser proof. |
| R02 | Strict tracing import fails semantic errors; permissive import warns/skips deterministically | tracing conversion | strict/non-strict paired tests in `tailtriage-tracing/src/lib.rs` | CLI import strict/non-strict tests | unit + boundary; gate/CI | inline records | high | KEEP | Library and command policy remain separate. |
| R03 | Live recorder/session captures completed spans, snapshots, shutdown, and zero-request policy | `recorder.rs`, `lib.rs` | tracing module live tests plus `tests/live_api_surface.rs` | facade `tests/tracing_facade.rs` | live + API/package; gate/CI | none | high | KEEP | Direct-crate and facade feature surfaces are meaningful boundaries. |
| R04 | Tokio tracing session couples sampler lifecycle and async shutdown | `tailtriage-tracing/src/tokio.rs` | `tests/tokio_session.rs` | facade tracing-Tokio test | live + package; gate/CI | none | high | KEEP | Feature/package exposure is separate from implementation behavior. |
| R05 | Imported Run carries exact source provenance through filtering and limits | tracing conversion/private provenance | provenance tests in `tailtriage-tracing/src/lib.rs` | equivalence semantic-retention scenario | unit + equivalence; gate/CI | equivalence fixtures | high | KEEP | Unit cases diagnose mappings; oracle scenario proves public projection. |
| R06 | Completed-span persistence writes retained original source records and reimports | tracing JSONL writer/session | `stable_writer_output_reimports_with_explicit_evidence` and recorder persistence tests | `tests/tracing_examples.rs` completed-span export | serialization + integration; gate/CI | example JSONL | high | KEEP | Protects identity/fields rather than reconstructed normalized spans. |
| A01 | Candidate eligibility thresholds and minimum sample rules are exact | analyzer scoring/candidate modules | `tests/boundary_thresholds.rs` plus eligibility tests in `src/tests.rs` | analyzer goldens | exhaustive small-domain + golden; gate/CI | analyzer fixtures | high | KEEP | Arithmetic boundaries and whole-report oracle are complementary. |
| A02 | Queue, blocking, executor, downstream, and in-flight raw scoring is exact | analyzer scoring/attribution | family-specific named tests in `src/tests.rs`, `scoring.rs`, `stage_attribution.rs` | nine analyzer golden reports | unit + serialization/golden; gate/CI | analyzer fixture pairs | high | KEEP | Do not merge arithmetic proof into goldens. |
| A03 | Missing local depth is a normalized lower bound, including worker-count historical fallback | analyzer scoring | `missing_local_depth_remains_normalized_lower_bound`, `ambiguous_worker_fallbacks_match_historical_score_and_explain_the_cap`, historical arithmetic tests | `executor_pressure` golden | unit + golden; gate/CI | executor fixture | high | KEEP | Explicitly owns requested fallback/lower-bound audit. |
| A04 | Evidence caps compose into final confidence with stable notes | analyzer confidence/evidence | confidence-cap tests in `src/tests.rs` | goldens and diagnostic partial/truncation cases | unit + golden/integration; gate/CI | analyzer/diagnostic fixtures | high | KEEP | Exact cap mechanics and rendered/corpus outcomes differ. |
| A05 | Final ranking is confidence-first, then raw score, then stable kind | analyzer `lib.rs`, confidence | first four ranking tests in `src/tests.rs` | `cap_induced_primary_flip_has_exact_json/text` | unit + serialization; gate/CI | inline Runs | high | KEEP | Exact ordering plus rendered regression are both useful. |
| A06 | Ambiguity uses raw-score cluster semantics and caps members uniformly | analyzer confidence | ambiguity cluster/cap/order tests | mixed analyzer goldens | unit + golden; gate/CI | mixed fixtures | high | KEEP | Whole mixed reports protect evidence text/order beyond arithmetic. |
| A07 | Warning and evidence-quality status/caps reflect missing, partial, truncated evidence | analyzer evidence/partial modules | warning/evidence-quality/partial tests in `src/tests.rs` | diagnostic manifest report contracts and executable partial corpus | unit + integration; gate/CI | partial/truncation corpus | high | KEEP | Corpus is controlled fixture evidence, not universal accuracy. |
| A08 | Route analysis is excluded for a single/common route; divergent routes are sorted without changing global result | `route.rs`, `slicing.rs` | route tests in `src/tests.rs` and slicing tests | `scoped_route` golden | unit + golden; gate/CI | scoped route fixture | high | KEEP | Explicitly owns route inclusion/exclusion boundary. |
| A09 | Temporal analysis excludes insignificant/sparse cases, scopes runtime/inflight, and preserves global result | `temporal.rs`, `slicing.rs` | temporal tests in `src/tests.rs` and exact p95-shift tests | `scoped_temporal` golden | exhaustive small-domain + golden; gate/CI | scoped temporal fixture | high | KEEP | Explicitly owns temporal exclusion and window boundaries. |
| A10 | Completed queue/stage distributions exclude partials while lower bounds stay visible/capped | partial evidence and attribution | partial evidence tests in `src/tests.rs` | diagnostic partial corpus | unit + integration; gate/CI | partial corpus Runs | high | KEEP | Typed calculations and CLI/corpus execution remain distinct. |
| A11 | `analyze_run` validates options but remains permissive over Run integrity | analyzer public API | `analyze_run_rejects_invalid_options`, `analyze_run_still_works_with_default_options` | `tests/public_api_contract.rs`; CLI divergence test | API + boundary; gate/CI | queue fixture | high | KEEP | Canonical API and strict CLI policy are intentionally different. |
| A12 | Text, compact JSON, and pretty JSON render independently and match serde representation | `render.rs`, analyzer API | renderer equivalence tests in `src/tests.rs` | golden report test and CLI JSON parity | serialization/golden + process; gate/CI | analyzer goldens | high | KEEP | Renderer, checked-in bytes, and CLI output are distinct boundaries. |
| A13 | Report schema keys and analyzer-config schema/default transparency remain stable | analyzer report/options | `tests/report_schema_contract.rs`, descriptor/TOML/config tests | docs config contract tests | serialization + documentation; gate/CI | queue fixture, example TOML | high | KEEP | Documentation validates published paths, not typed behavior. |
| L01 | CLI command/help policy exposes analyze/import, removed strict flag stays rejected, import strict remains distinct | CLI `main.rs` | named help/removed-flag tests in `cli_boundary.rs` | docs contracts | boundary + documentation; gate/CI | none | high | KEEP | Negative tests protect the final Phase-26 command surface, not obsolete APIs. |
| L02 | Saved Run loading requires supported finalized schema and non-empty requests | CLI `artifact.rs` | its eleven unit tests | `cli_boundary.rs` strict-default cases | unit + process boundary; gate/CI | inline JSON/temp files | high | KEEP | Loader diagnostics and end-to-end exit/stderr both matter. |
| L03 | CLI analysis is strict by default; explicit ambiguous mode normalizes and emits every issue | CLI main/artifact + core validation | strict/permissive cases in `cli_boundary.rs` | `json_parity.rs` seven-case matrix | boundary + equivalence; gate/CI | inline JSON | high | KEEP | Policy and normalized cross-entry equivalence are distinct. |
| L04 | Analyzer config file/override precedence, type/range errors, and help are CLI-private behavior | CLI `analyze_config.rs`, main | seven private module tests | CLI boundary config/help tests | unit + boundary; gate/CI | temporary TOML | high | CONSOLIDATE | See backlog A1: two default/config/override paths overlap at the same config-composition boundary; retain process error/help coverage. |
| L05 | CLI JSON exactly matches analyzer renderer | CLI output delegation | `tailtriage-cli/tests/json_parity.rs::cli_json_matches_analyzer_renderer_output` | analyzer renderer unit tests | equivalence; gate/CI | queue analyzer fixture | high | KEEP | Cross-process equality is stronger than either side alone. |
| P01 | Facade default exposes core, controller, and Tokio paths | `tailtriage/Cargo.toml`, `src/lib.rs` | six compile tests in `tailtriage/src/lib.rs` under feature configurations | public example smoke script | package + API; gate/CI | examples | medium | KEEP | Facade availability is not implied by component workspace tests. |
| P02 | Facade Axum/tracing feature relationships expose direct integration APIs | facade manifest/lib | feature-gated facade unit tests and `tests/tracing_facade.rs` | docs contract feature checks | package + live; gate/CI | none | high | KEEP | Protects feature wiring and executable API. |
| P03 | Each of eight product packages compiles all targets/features with public examples | eight Cargo manifests and crate roots | workspace all-target/all-feature Cargo test plus example targets | `scripts/smoke_public_examples.py`, controller smoke | package; gate/CI | public examples | low/medium | KEEP | Direct-package boundary and facade boundary are intentionally retained. |
| F01 | Nine analyzer Run/Report pairs are independent full-report golden contracts | analyzer test fixtures/expected | `analyzer_fixtures.rs::fixture_reports_match_canonical_pretty_json_golden_files` | focused category/route/temporal tests | serialization/golden; gate/CI | 18 analyzer files | high | KEEP | Full serialized reports are not duplicate arithmetic tests. |
| F02 | Stable tracing wrapper fixture is shared only across parser and CLI boundaries | tracing fixture | `jsonl.rs::stable_wrapper_fixture_imports` | CLI wrapper fixture acceptance | serialization + boundary; gate/CI | `tailtriage-span-v1.jsonl` | high | KEEP | Same bytes, different library/process boundary. |
| F03 | Four native/tracing scenarios match independent Run and Report oracles | tracing equivalence harness | `tests/equivalence_contract.rs` oracle tests | compact live harness smoke in `tests/equivalence.rs` | equivalence + live; gate/CI | native Runs, four JSONL, eight expected files | high | KEEP | Oracles protect deterministic representation; live smoke protects recorder behavior. |
| F04 | Diagnostic executable fixture inventory/bytes/shape and observation accounting are locked | diagnostic manifest/lock | `check_diagnostic_fixture_integrity.py` and its 58 unit tests | `diagnostic_benchmark.py` | serialization + integration; CI | manifest, lock, 11 executable artifacts | high | KEEP | Integrity does not prove analyzer correctness; benchmark does. |
| F05 | Demo reports preserve nine controlled before/after scenario expectations | demo workloads/support | `check_demo_fixture_drift.py --profile dev` | eight CLI demo smoke tests and diagnostic report-contract cases | integration + serialization; CI/manual | 18 reports + downstream comparison | medium/high | KEEP | Runtime demo, CLI smoke, and report-shaped corpus uses are distinct. |
| D01 | Deterministic validation classifies analyzer execution, accuracy observations, report contracts, and non-claims | diagnostic scripts/manifest | `diagnostic_benchmark.py` and `run_diagnostic_matrix.py` tests/CI commands | fixture integrity and scorecard generator | integration + operational; CI/manual/snapshot | diagnostic corpus | high | KEEP | No claim of production truth or formal causal proof. |
| D02 | Repeated-run and mitigation matrices remain machine/workload scoped and support rerun workflow | matrix scripts | their Python unit suites and manual runners | validation documentation contract | operational; manual | generated target artifacts | medium | KEEP | Runtime-sensitive evidence cannot be replaced by deterministic fixtures. |
| O01 | Runtime-cost output is internally consistent characterization, never universal overhead | runtime-cost demo/scripts | runtime-cost unit/summary tests and CI smoke | committed status scorecard/docs contracts | operational; CI/manual | generated raw/summary | high | KEEP | Keep machine/workload/profile qualification. |
| O02 | Collector-limit output exposes bounded drops, warnings, and downgrade behavior; never “no drops” | collector demo/scripts | collector unit/summary tests and CI smoke | committed status scorecard/docs contracts | operational; CI/manual | generated raw/summary | high | KEEP | Saturation is expected evidence, not failure of the characterization. |
| M01 | User docs remain complete, linked, current, and keep product/validation claims bounded | Markdown + docs index | `scripts/validate_docs_contracts.py` and 106-test module | CI docs-contract job | documentation/boundary; gate/CI | Markdown/examples/workflow source | high | KEEP | Contract tests inspect repository source only, not hosted state. |
| M02 | Removed Phase-26 public APIs/helpers remain absent | package public sources | residual public API cleanup checks in docs validator | canonical public API compile tests | API + documentation; gate/CI | source text | high | KEEP | Negative absence checks are current-surface proof, not obsolete compatibility proof. |
| Z01 | Release preflight is check/package-only, dependency ordered, and never publishes | `scripts/check_release.py` | `scripts/tests/test_check_release.py` | `docs/dev/RELEASING.md`, docs validator, CI source | release + documentation; gate/CI/manual | manifests | high | ADD MISSING PROOF | Existing tests cover plan/error behavior but no focused negative test scans checker/workflows for executable `cargo publish`; add in the existing release test module. |
| Z02 | Release remains manual: no tag, GitHub Release, credentials, or publication automation | release docs and workflow source | docs contract statements/source checks | manual review | release; gate/CI/manual | Markdown/workflows | medium | ADD MISSING PROOF | Add one repository-source policy test rather than network/hosted inspection. |
| Q01 | Option descriptors have unique exact paths and bounded combinations are exhaustively validated | analyzer option registry | `descriptors_have_unique_and_exact_v1_paths`, validation tables/loops | CLI help/config tests | exhaustive small-domain + boundary; gate/CI | example TOML | high | KEEP | Existing finite tables are sufficient; no property-testing dependency is justified. |

**Inventory total:** 67 invariant rows across the 30 required families. The rows deliberately
split lifecycle, normalization, configuration, intake, scoring, rendering, package, fixture,
operational, documentation, and release guarantees where their proof owners or boundaries differ.

## Baseline proof-surface counts

Measured at the audit baseline with
`cargo test --workspace --all-targets --all-features --locked -- --list`:

| Package/area | Rust test targets | Listed test cases |
| --- | ---: | ---: |
| Demo binaries and `demo-support` | 12 | 16 |
| `tailtriage` facade | 2 | 9 |
| `tailtriage-analyzer` | 6 | 272 |
| `tailtriage-axum` (including two example targets) | 4 | 4 |
| `tailtriage-cli` | 5 | 122 |
| `tailtriage-controller` (including two example targets) | 3 | 57 |
| `tailtriage-core` | 1 | 141 |
| `tailtriage-tokio` (including two example targets) | 3 | 31 |
| `tailtriage-tracing` (including four example targets) | 11 | 303 |
| **Total** | **47** | **955** |

The 47 targets include zero-test binary/example compile targets. The listed 955 cases count Rust
test cases, not doctests (this all-target invocation listed no separate doctest target).

Python repository validation has **14 modules** under `scripts/tests/` and **300 declared
`test_*` methods** (294 are conventionally indented methods; six additional compactly formatted
methods in `test_generate_diagnostic_scorecard.py` are discovered by `unittest`). The focused docs
module owns 106 tests. `python3 -m unittest discover -s scripts/tests -v` is the executable count
boundary; declaration counts are descriptive, not a replacement for discovery.

The full required Rust test command baseline is **5.102 seconds elapsed on the audit container
after compilation warm-up**, measured with
`TIMEFORMAT='WALL_SECONDS=%R'; time cargo test --workspace --all-targets --all-features --locked`. Wall time is
machine/cache dependent and is not an optimization target.

## Phase 27A2 decision backlog

### A. High-confidence proposed consolidation/removal

1. **Consolidate CLI analyzer-config composition cases (L04).** In
   `tailtriage-cli/src/analyze_config.rs`, consolidate
   `default_options_without_config_or_overrides`, `config_toml_applies`, and
   `override_applies_and_beats_toml_last_wins` into one table-driven private-boundary test. Keep
   `tailtriage-cli/tests/cli_boundary.rs` config/help/error tests as the process boundary. The
   surviving primary owner is the table-driven private test; boundary and diagnostic quality stay
   equal or improve because the failing case is labeled. Benefit: one setup/composition path;
   risk: low. Validate focused CLI tests and the full gate.
2. **Remove no executable compatibility test.** The obsolescence search found no Rust/Python test
   calling `Analyzer`, `try_analyze_run`, `analyze_run_json*`, analyzer strict wrappers,
   `try_begin_request*`, `RuntimeSampler::start(run, interval)`, either removed `crate_name()`, or
   public CLI artifact helpers. Validator negative-source checks intentionally survive as M02.
   Consequently there is no high-confidence `REMOVE` proposal in 27A1.

### B. Intentionally retained overlap

- Keep analyzer arithmetic/eligibility tests beside full Report goldens: they cross unit versus
  serialization boundaries and the unit failures isolate rules better.
- Keep core validation, CLI strict policy, and analyzer permissive behavior: these are three
  deliberately different acceptance boundaries.
- Keep native/tracing deterministic oracles beside live recorder smoke: serialized equivalence
  cannot prove live span lifecycle, and live equality alone could let both paths drift together.
- Keep facade and direct-package compile tests: feature/re-export availability is not implied by
  component compilation.
- Keep parser, CLI, and example consumers of completed-span JSONL fixtures: they prove library,
  process/file, and teaching surfaces respectively.
- Keep deterministic corpus, demo drift, repeated-run, mitigation, runtime-cost, and collector
  validation separate because their cadence, determinism, and non-claims differ.
- Keep the byte-identical low-request fixture copies described in `FIXTURE_LINEAGE.md`: one owns an
  analyzer golden and the contained copy owns integrity/CLI execution accounting.

### C. Missing-proof additions

1. **Release no-publication proof (Z01).** Add a focused test to
   `scripts/tests/test_check_release.py` that inspects the generated executable plan and repository
   workflow commands, proving publication commands are printed inert instructions only. Primary
   owner remains the release test module; class `release`; risk low; run that module, docs
   contracts, and full gate.
2. **Repository release-boundary policy (Z02).** Add one docs-validator policy test covering no
   executable `cargo publish`, tag, Release, or credential step in checked-in scripts/workflows.
   Do not query hosted state. Primary owner becomes the docs validator test module; class
   `release/documentation`; risk low-to-medium due to source-scanner false positives.

### D. Deferred/uncertain candidates (retain by default)

- The CLI private config tests and process tests share inputs, but only the three positive
  composition cases in A1 are approved candidates. Retain misspelling, invalid type, missing file,
  invalid TOML, help, stderr, and exit-status cases because their diagnostics/boundaries differ.
- `tailtriage-tracing/tests/equivalence.rs` overlaps deterministic equivalence assertions, but it
  uses the live recorder harness; retain until a line-by-line proof shows the recorder lifecycle is
  independently owned with equal diagnostics.
- Analyzer `public_api_supports_report_text_and_json_contract_fields`,
  `public_api_contract.rs`, and `report_schema_contract.rs` overlap superficially. Retain: typed
  usability, external integration-crate visibility, and serialized documented keys differ.
- `scripts/tests/test_validate_docs_contracts.py` contains many accept/reject pairs and setup
  repetition. Do not mechanically merge them: mutation-focused failures identify individual
  public contracts. Revisit only with exact same-boundary assertion equivalence.
- Demo smoke fixtures overlap drift and diagnostic consumers. Retain until each smoke assertion is
  compared with the exact manifest field; process/package and report-contract boundaries may be
  lost.

## Phase-26 obsolescence audit

The executable-source search found **no obsolete compatibility test candidate**. Current uses of
`CliAnalyzeConfigError`, `build_analyze_options`, and `analyzer_options_help_text` are private CLI
implementation and private tests; they do not preserve a public library surface. The docs
validator's banned-token fixtures and scans intentionally prove that removed APIs stay absent.
`scripts/tests/test_validate_all.py::test_skip_cargo` protects the current opt-out flag; no test or
implementation reference to removed `--include-cargo` remains. Historical Markdown mentions are
migration records and are outside executable compatibility proof.

## Property-testing decision

Do not add a property-testing dependency in 27A2. Candidate eligibility, ratios, ordering,
normalization, option paths, and temporal shifts already use explicit boundary cases, finite
tables, or bounded/exhaustive loops with better named diagnostics. The audited domains are cheaply
enumerable; a new dependency would add weight without materially improving discovery or shrinking.

## 27A2 acceptance rule

No proposal may be implemented unless its surviving owner, meaningful boundary, diagnostics,
fixture/package/platform impact, and validation command are still accurate at the implementation
baseline. Any uncertainty means `KEEP`. After accepted work, record after-counts and a comparable
warm-cache wall-time without treating speed as evidence of redundancy.
