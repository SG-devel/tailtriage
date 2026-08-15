# Fixture and scenario lineage

This is the durable ownership map for committed fixture-like data. Paths are repository-relative.
The named consumer, rather than filename similarity, determines proof ownership. Suspects produced
from these artifacts are triage leads, not proof of root cause.

## Mutation rules

- **Manual-reviewed** means no supported generator exists. Change the source only for an intended
  contract change, review the semantic diff, and run its consumer. An expected output may then be
  updated deliberately, but changing it merely to silence its producer's failing test is not proof.
- **Deterministic** means repository-controlled bytes/behavior, not realistic production timing.
  Operational measurements are explicitly machine/workload/profile sensitive.
- A refresh command owns only the paths listed here. Generated files under `target/` are local
  evidence and are not committed contracts.

## Analyzer fixtures and goldens

All nine pairs are deterministic, manually authored `Run` inputs and independently reviewed Report
oracles. There is no supported input or golden refresh command. Each
`tailtriage-analyzer/tests/fixtures/<case>.json` is analyzed by
`tailtriage-analyzer/tests/analyzer_fixtures.rs`; its canonical pretty JSON must equal
`tailtriage-analyzer/tests/expected/<case>.report.json`. The expected report protects complete
ranking, evidence, next-check, warning, confidence, breakdown, and rendering behavior.

| Case (paired fixture and `.report.json`) | Isolated intent and additional consumers | Disposition |
| --- | --- | --- |
| `queue_saturation` | Queue wait/depth and in-flight growth; category, render, request-share, and in-flight assertions in `analyzer_fixtures.rs`; public/schema contracts; slicing unit tests | Keep pair |
| `blocking_pressure` | Blocking-pool runtime pressure; category assertion in `analyzer_fixtures.rs`; slicing unit test | Keep pair |
| `executor_pressure` | Busy/park executor pressure; category assertion in `analyzer_fixtures.rs`; worker-normalized scoring unit test | Keep pair |
| `downstream_stage` | A slow named stage dominating service time; category and request-share assertions in `analyzer_fixtures.rs`; slicing unit test | Keep pair |
| `insufficient_evidence` | Low request count with absent queue/stage/runtime signals; category assertion in `analyzer_fixtures.rs` | Keep pair; linked duplicate noted below |
| `mixed_queue_vs_blocking` | Deliberately competing queue and blocking signals; stronger-evidence ordering in `boundary_thresholds.rs` | Keep pair |
| `mixed_blocking_vs_downstream` | Deliberately competing blocking and downstream signals; stronger-evidence ordering in `boundary_thresholds.rs` | Keep pair |
| `scoped_route` | Routes with divergent queue/downstream diagnoses; explicit route-breakdown assertions in `analyzer_fixtures.rs` | Keep pair |
| `scoped_temporal` | Early/late windows with evidence movement; explicit temporal assertions in `analyzer_fixtures.rs` | Keep pair |

Every golden is used only by the common golden test. Input consumers beyond that test are named in
the table: `public_api_contract.rs` and `report_schema_contract.rs` consume `queue_saturation`, and
the analyzer's `scoring.rs`/`slicing.rs` unit tests consume the noted inputs. These pairs are
diagnosis-rule regression oracles, unlike demo teaching output, report-contract data that bypasses
analysis, or diagnostic analyzer-execution inputs that protect CLI/import boundaries.

## Tracing fixtures and independent equivalence oracles

`tailtriage-tracing/tests/fixtures/equivalence/native_runs.json`, the four scenario JSONL files, and
the eight files under `tailtriage-tracing/tests/expected/equivalence/` are manually authored,
deterministic committed inputs/oracles. Current code and documentation expose no producer or refresh
command. `tests/support/equivalence_harness.rs` loads all four sides and
`tests/equivalence_contract.rs` is authoritative. Updates must be reviewed side by side; expected
projections must not be generated from either implementation path.

| Scenario | Native and tracing sources | Expected projections | Contract purpose / disposition |
| --- | --- | --- | --- |
| `duration_only_legacy` | Entry in `native_runs.json`; `fixtures/equivalence/duration_only_legacy.jsonl` | `expected/equivalence/duration_only_legacy.{run,report}.json` | Legacy duration-only conversion warnings and equal diagnosis; keep |
| `precise_route_divergent` | Entry in `native_runs.json`; `fixtures/equivalence/precise_route_divergent.jsonl` | `expected/equivalence/precise_route_divergent.{run,report}.json` | Precise interval conversion plus divergent route diagnoses; keep |
| `precise_temporal_movement` | Entry in `native_runs.json`; `fixtures/equivalence/precise_temporal_movement.jsonl` | `expected/equivalence/precise_temporal_movement.{run,report}.json` | Precise interval conversion plus early/late movement; keep |
| `semantic_retention_limits` | Entry in `native_runs.json`; `fixtures/equivalence/semantic_retention_limits.jsonl` | `expected/equivalence/semantic_retention_limits.{run,report}.json` | Equal semantic retention order/drop counts under limits; keep |

For each row the Run oracle proves importer conversion into the representable completed request,
stage, queue, and semantic-truncation projection. The Report oracle separately proves analyzer-result
equivalence. Both native and imported values must match each oracle; native-equals-imported alone is
insufficient. Run-only partial events, in-flight/runtime evidence, and lifecycle metadata remain
outside this completed-span contract.

`tailtriage-tracing/tests/fixtures/tailtriage-span-v1.jsonl` is a separate manually authored stable
wrapper-format/import fixture. `tailtriage-tracing/src/jsonl.rs` and
`tailtriage-cli/tests/cli_boundary.rs` consume it to protect library parsing and CLI intake. It has
no generator and is not an equivalence scenario; keep it distinct.

## Diagnostic manifest, corpus, and integrity lock

`validation/diagnostics/manifest.json` is the sole classification/accounting owner. Every referenced
artifact is in exactly one class:

| Lineage class | Exact committed artifacts | Execution / accuracy / proof | Mutation and integrity |
| --- | --- | --- | --- |
| `analyzer_execution` / `run_artifact` | `corpus/run-artifacts/{low-request-insufficient,no-queue-events,no-runtime-snapshots,no-stage-events,truncated-queues}.json` and `corpus/{partial-evidence-completed-only,partial-evidence-mixed,partial-evidence-queue-only,partial-evidence-stage-only,cancelled-request-with-partial-child}.json` | `scripts/diagnostic_benchmark.py` loads through real CLI Run intake and analyzer; the ten controlled observations are accuracy eligible. `tailtriage-analyzer/src/tests.rs` also deserializes the five direct-corpus partial/cancelled Runs to protect defaults and partial flags. | Manually authored deterministic inputs; integrity-lock protected; keep |
| `analyzer_execution` / `tracing_span_jsonl` | `corpus/tracing-queue-dominant.jsonl` | Benchmark executes the public tracing importer then analyzer; one accuracy-eligible observation | Manually authored deterministic input; integrity-lock protected; keep |
| `report_contract` / `analysis_report` | The 22 manifest paths in `demos/{queue_service,blocking_service,executor_pressure_service,downstream_service,mixed_contention_service,cold_start_burst_service,db_pool_saturation_service,shared_state_lock_service,retry_storm_service}/fixtures/` (before/after or baseline/mitigated plus four sample aliases) | Benchmark inspects already report-shaped teaching outputs; analyzer/importer does not execute; accuracy ineligible | Owned/generated by the demo boundary below, not the diagnostic lock; keep linked use |
| `report_contract` / `synthetic_analysis_report` | All 16 committed `.json` files directly in `validation/diagnostics/corpus/` other than the five partial/cancelled Run inputs above | Benchmark checks deliberately report-shaped warning, humility, adversarial, route, and truncation contracts; analyzer does not execute; accuracy ineligible | Manually authored deterministic report contracts; no refresh command; keep |

The manifest's `observation_id` is the logical accuracy unit: only unique accuracy-eligible
observations enter top-1/top-2 accuracy. Multiple encodings may share one ID; different paths or
bytes do not imply independent evidence.

`validation/diagnostics/analyzer-fixtures.lock.json` is a derived integrity lock, consumed by
`scripts/check_diagnostic_fixture_integrity.py`. It covers exactly manifest-owned
`analyzer_execution` artifacts because those are executable analyzer/importer inputs whose stable
inventory and provenance affect accuracy. It checks inventory, exact bytes, UTF-8/LF formatting,
compact structural shape, and rejects identical bytes assigned to distinct accuracy observations.
Report contracts bypass execution and are accuracy-ineligible, so expanding this lock to them would
not strengthen its stated boundary.

`python3 scripts/check_diagnostic_fixture_integrity.py --refresh` deterministically rewrites only
`validation/diagnostics/analyzer-fixtures.lock.json`; it never edits source artifacts. Refresh only
after manual artifact/manifest review, then review hashes and shapes. Check mode omits `--refresh`.

`tailtriage-analyzer/tests/fixtures/insufficient_evidence.json` and
`validation/diagnostics/corpus/run-artifacts/low-request-insufficient.json` are byte-identical. The
latter is a linked copy sourced from the former, not independent evidence: the analyzer copy owns a
typed diagnosis golden, while the diagnostics-root copy is required by the diagnostic checker's
contained artifact-path and integrity boundary and owns CLI/analyzer execution accounting. Keep both.

`validation/diagnostics/latest/scorecard.md` is a stable committed status/reference note, not the
latest run on a machine. `scripts/generate_diagnostic_scorecard.py` produces machine/environment
qualified snapshot output under `target/validation/diagnostics/`; the manually dispatched
`validation-snapshot.yml` workflow is the only durable snapshot workflow. Updating the committed
note is manual-reviewed; normal CI does not overwrite it.

## Demo analysis and comparison artifacts

The source of truth for each demo is its workload in `demos/<scenario>/src/main.rs` plus shared
`demos/demo_support/src/lib.rs`. `scripts/check_demo_fixture_drift.py` runs baseline/before and
mitigated/after modes through the real demo and CLI analyzer. Its dev-profile normalized reports are
deterministic contract fixtures; raw execution timing is not promoted as a byte-exact guarantee.

| Scenario | Committed analysis roles | Consumers and disposition |
| --- | --- | --- |
| `queue_service` | `before-analysis.json`, `after-analysis.json`; `sample-analysis.json` aliases before | Drift checker, diagnostic report contracts, CLI sample smoke, docs contract sample; keep alias as a derived teaching/test path, not independent evidence |
| `blocking_service` | before, after, sample alias | Same boundaries (except docs contract); keep alias |
| `executor_pressure_service` | before, after, sample alias | Same boundaries; keep alias |
| `downstream_service` | before, after, sample alias | Same boundaries plus drift-owned comparison; keep alias |
| `mixed_contention_service` | `baseline-analysis.json`, `mitigated-analysis.json` | Drift checker, diagnostic contracts, CLI smoke; keep |
| `cold_start_burst_service` | before and after | Drift checker, diagnostic contracts, CLI smoke; keep |
| `db_pool_saturation_service` | before and after | Drift checker, diagnostic contracts, CLI smoke; keep |
| `shared_state_lock_service` | before and after | Drift checker and diagnostic contracts; keep |
| `retry_storm_service` | before and after | Drift checker, diagnostic contracts, CLI smoke; keep |

The canonical check is `python3 scripts/check_demo_fixture_drift.py --profile dev`; refresh is the
same command plus `--refresh`. The script's `_scenario_specs()` is the single mutation map. Refresh
may rewrite exactly the 22 analysis files described above and
`demos/downstream_service/fixtures/before-after-comparison.json`—23 paths total. Check and refresh
use that same map. It does **not** own every file in a fixture directory.

All four `sample-analysis.json` files intentionally have the corresponding before report as their
single canonical source. Their explicit CLI, manifest, drift, and (for queue) documentation-contract
paths make them public/test teaching aliases, but they are neither independent scenarios nor
accuracy observations. Consolidating/repointing those consumers is deferred to Phase 25B.

Eight committed comparison files are derived teaching snapshots created by
`scripts/_demo_runner.py` from the two analyses:

- `demos/downstream_service/fixtures/before-after-comparison.json` is checked and refreshed by the
  drift owner and is referenced generically by `demos/README.md`; keep.
- The comparison files for `blocking_service`, `cold_start_burst_service`,
  `db_pool_saturation_service`, `executor_pressure_service`, `mixed_contention_service`,
  `retry_storm_service`, and `shared_state_lock_service` have no exact code/test consumer and are
  not in `_scenario_specs()`. They are committed teaching copies covered only by the generic demo
  guide. Preserve them as manually reviewed derived output for 25A; Phase 25B should either assign
  explicit teaching consumers/drift ownership or remove them.

`queue_service` has no committed comparison. No refresh ownership was changed in this packet.

## Operational validation references

| Domain | Generated evidence and commands | Committed `latest/` role and mutation |
| --- | --- | --- |
| Runtime cost | `python3 scripts/measure_runtime_cost.py` produces raw/summary files; `python3 scripts/validate_runtime_cost_summary.py --raw <raw> --summary <summary>` validates them. `python3 scripts/run_operational_validation.py --domain runtime-cost --profile <dev|release>` orchestrates output under `target/operational-validation/`. | `validation/runtime-cost/latest/scorecard.md` is a stable checked-in availability/status note, not measured latest output. Manual-reviewed only; keep. Measurements are machine/workload/profile sensitive. |
| Collector limits | `python3 scripts/measure_collector_limits.py` produces raw/summary files; `python3 scripts/validate_collector_limits_summary.py --raw <raw> --summary <summary>` validates them. `python3 scripts/run_operational_validation.py --domain collector-limits --profile <dev|release>` orchestrates output under `target/operational-validation/`. | `validation/collector-limits/latest/scorecard.md` is a stable checked-in availability/status note. Manual-reviewed only; keep. Evidence is machine/workload/profile sensitive and proves visible bounded drops, warnings, and downgrade behavior—not no drops. |

`scripts/run_operational_validation.py --domain all` composes both domains. Generated `target/`
files are local/CI evidence, not committed contracts and not sources for automatic `latest/`
refresh. The diagnostic scorecard described above is a third, separate deterministic-corpus status
reference, not an operational measurement.

## Cross-family decisions and Phase 25B inputs

- Analyzer Runs versus diagnostic Runs: same scenario concept (and once the same bytes), different
  consumer boundary; typed golden proof versus contained CLI/import accuracy input.
- Analyzer expected Reports versus diagnostic report contracts: independently reviewed analyzer
  oracle versus already report-shaped contract input; the latter cannot prove analyzer execution.
- Demo Reports reused by diagnostics: same bytes and same demo source, different consumer boundary;
  diagnostics links to them without creating an independent observation.
- Equivalence Reports versus analyzer goldens: independently reviewed oracle for two intake paths
  versus one analyzer-rule regression input; keep distinct.
- Sample aliases versus before Reports: derived teaching aliases, not independent validation
  evidence. Phase 25B candidate: repoint all explicit consumers coherently, then remove aliases if
  the named sample contract is no longer useful.
- Committed `latest/` versus generated `target/`: stable reference/status material versus generated
  machine- or snapshot-scoped evidence, never an automatic promotion.
- Phase 25B candidate: decide the seven unowned comparison teaching copies as one coherent demo
  simplification—add explicit ownership only where teaching value remains, otherwise remove them.
