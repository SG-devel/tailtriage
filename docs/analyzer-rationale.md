# Analyzer rationale and revision criteria

This is the repository-owned rationale catalog for the analyzer's major
defaults, policies, compatibility rules, and interpretation boundaries. It is
for maintainers deciding whether a behavior should change. The
[behavior reference](diagnostics.md) remains authoritative for current
mechanics, formulas, threshold tables, and fallback matrices; code and focused
tests remain authoritative when prose and implementation disagree.

Each entry uses one of four classifications:

- **Hard contract** — an interpretation, determinism, or safety invariant that
  callers and reports rely on.
- **Compatibility obligation** — behavior retained so older supported artifacts
  or public output continue to mean the same thing.
- **Conservative policy** — a deliberate bias against overconfident diagnosis
  when evidence is incomplete or ambiguous.
- **Calibrated heuristic** — a tunable cutoff or weight used to rank useful
  leads; it is not a universal physical constant.

Provenance is stated separately: **recorded intent** is directly supported by
durable documentation, history, or a focused test; **present-purpose
inference** is a conservative explanation of the failure mode protected by the
current behavior, not a historical claim; **unknown provenance** means the
repository protects the exact choice but does not record why that exact value
was selected. An entry can have recorded intent for the policy while the exact
numeric calibration remains unknown.

## Interpretation and ranking

### AN-MODEL-001 — Evidence ranking, not probability or causality

- **Rule or default:** Reports contain deterministic evidence-ranked suspects
  and next checks, not probabilities or causal findings; see
  [scope and sources of truth](diagnostics.md#scope-inputs-and-sources-of-truth)
  and [non-claims](diagnostics.md#non-claims-and-known-limitations).
- **Classification:** Hard contract.
- **Problem addressed:** A numerical rank can otherwise be mistaken for proof,
  predicted likelihood, or a complete diagnosis of the service.
- **Why this shape:** Recorded intent: the product is a bounded triage layer
  over retained evidence. Determinism makes the same artifact and options
  reviewable and reproducible without claiming a statistical model.
- **Tradeoff:** Clear, repeatable leads instead of probabilistic calibration or
  automated causal conclusions.
- **Proof owner:** `tailtriage-analyzer/src/tests.rs` rendering and deterministic
  report tests; diagnostic corpus expectations in
  `validation/diagnostics/manifest.json` protect representative rankings.
- **Revision criteria:** Require an explicitly approved product/model change,
  a defined probability or causality semantics, calibration evidence, schema
  and compatibility analysis, and updated validation non-claims.
- **Provenance:** Recorded intent (`SPEC.md`, `docs/dev/DESIGN_NOTES.md`, and validation
  documentation).

### AN-SCORE-001 — Scores only order evidence within one report

- **Rule or default:** Raw scores rank candidates inside the current report and
  are not cross-run severity; see
  [candidate scoring](diagnostics.md#candidate-eligibility-and-scoring).
- **Classification:** Hard contract.
- **Problem addressed:** Comparing heuristic points across different workloads,
  captures, or option sets can imply improvement or regression that the inputs
  do not establish.
- **Why this shape:** Recorded intent: scores combine heterogeneous retained
  signals solely to choose investigation order.
- **Tradeoff:** Useful within-report ranking without a universal severity scale;
  reruns must compare underlying evidence as well as suspect movement.
- **Proof owner:** Typed scoring tests in `tailtriage-analyzer/src/tests.rs` and
  boundary tests in `tailtriage-analyzer/tests/boundary_thresholds.rs`.
- **Revision criteria:** Require a normalized cross-run metric definition,
  representative workload calibration, versioning rules, and mitigation
  validation showing meaningful comparisons across captures.
- **Provenance:** Recorded intent; exact score weights have unknown provenance.

### AN-RANK-001 — Confidence-first final order and stable ties

- **Rule or default:** Eligible candidates sort by final confidence, then raw
  score, then stable kind order; `insufficient_evidence` remains after eligible
  diagnoses. Raw-score ambiguity is computed before evidence caps; see
  [confidence, ambiguity, and final ordering](diagnostics.md#confidence-ambiguity-and-final-ordering).
- **Classification:** Hard contract.
- **Problem addressed:** Weakly supported high scores must not outrank better
  supported leads, while evidence caps must not erase close-score alternatives;
  ties must not depend on collection or map order.
- **Why this shape:** Recorded intent: confidence represents support quality and
  therefore controls the visible investigation order. Raw scores still own
  ambiguity because evidence caps answer a different question. The fallback is
  last so it cannot displace an eligible diagnosis candidate.
- **Tradeoff:** A lower raw score may become primary, and the fixed kind order
  introduces a deliberate final tie bias in exchange for stable output.
- **Proof owner:** `final_ranking_uses_confidence_then_score_then_stable_kind`
  and ambiguity/fallback tests in `tailtriage-analyzer/src/tests.rs`; canonical
  JSON golden tests in `tailtriage-analyzer/tests/analyzer_fixtures.rs`.
- **Revision criteria:** Require a demonstrated misleading ordering class,
  focused counterexamples, a report compatibility assessment, and replacement
  tests that preserve deterministic ties and evidence humility.
- **Provenance:** Recorded intent (`SPEC.md` and typed tests).

## Percentiles and attribution

### AN-PCTL-001 — Deterministic non-interpolated percentiles

- **Rule or default:** Percentiles select the documented ceiling index without
  interpolation; see
  [percentiles and units](diagnostics.md#percentiles-and-units).
- **Classification:** Hard contract.
- **Problem addressed:** Integer samples need one reproducible arithmetic rule;
  interpolation would invent values not present in a capture.
- **Why this shape:** Present-purpose inference: direct selection keeps report
  arithmetic simple and deterministic.
- **Tradeoff:** Results move in steps on small series.
- **Proof owner:** Percentile unit tests in
  `tailtriage-analyzer/src/scoring.rs` and boundary tests in
  `tailtriage-analyzer/tests/boundary_thresholds.rs`.
- **Revision criteria:** Require an explicit arithmetic compatibility decision,
  fixture inventory, and report-output impact analysis.
- **Provenance:** Present-purpose inference; historical reason for the exact
  estimator is unknown.

### AN-PCTL-002 — p95 tail-signal selection

- **Rule or default:** Tail-oriented analyzer signals use p95; mechanics remain
  owned by [percentiles and units](diagnostics.md#percentiles-and-units).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Median-only analysis can hide tail pressure, while a
  more extreme percentile can be unstable in bounded captures.
- **Why this shape:** Present-purpose inference: p95 is a pragmatic tail signal.
- **Tradeoff:** It is less sensitive to the most extreme tail than p99.
- **Proof owner:** Signal and boundary tests in
  `tailtriage-analyzer/src/tests.rs` and
  `tailtriage-analyzer/tests/boundary_thresholds.rs`.
- **Revision criteria:** Require representative capture-size analysis and
  diagnostic calibration showing another percentile improves useful ranking.
- **Provenance:** Present-purpose inference for the policy; exact p95 selection
  has unknown provenance.

### AN-ATTR-001 — Overlap-safe bounded attribution

- **Rule or default:** Complete request-relative intervals are unioned; if any
  interval is incomplete, authoritative durations are saturating-summed and
  capped by parent request latency; see
  [percentiles and units](diagnostics.md#percentiles-and-units).
- **Classification:** Conservative policy.
- **Problem addressed:** Retries, nesting, duplicates, and overlapping helpers
  can double-count time, while older duration-only evidence cannot be precisely
  unioned.
- **Why this shape:** Recorded intent: prefer precise interval union when
  available, but retain legacy/partial-precision evidence without allowing
  attributed time to exceed the request.
- **Tradeoff:** The fallback can overattribute one component within the parent
  bound and loses concurrency detail; rejecting it would discard useful older
  evidence.
- **Proof owner:** Focused overlap, nesting, fallback, and overflow tests in
  `tailtriage-analyzer/src/attribution.rs` and
  `tailtriage-analyzer/src/stage_attribution.rs`.
- **Revision criteria:** Require richer artifact timing that is reliably
  available, schema/legacy analysis, and adversarial overlap tests proving the
  replacement remains bounded.
- **Provenance:** Recorded intent (`SPEC.md`, partial-evidence design history,
  and focused tests).

### AN-EVID-001 — Completed distributions and partial lower bounds

- **Rule or default:** Public queue/service distributions remain completed-only;
  completed plus partial observations may form explicitly labeled queue or
  stage lower-bound candidates, capped at Medium when selected; see
  [percentiles and units](diagnostics.md#percentiles-and-units) and
  [confidence](diagnostics.md#confidence-ambiguity-and-final-ordering).
- **Classification:** Conservative policy.
- **Problem addressed:** Mixing abandoned observations into ordinary duration
  distributions misstates completion latency, but discarding them can hide the
  only visible pressure signal.
- **Why this shape:** Recorded intent: retain what was actually observed from
  first poll to Drop without interpreting Drop as operation completion or
  failure.
- **Tradeoff:** Lower bounds can surface a useful candidate but cannot support
  High confidence; completed-only summary fields can differ from candidate
  evidence.
- **Proof owner:** Partial queue/stage selection and confidence tests in
  `tailtriage-analyzer/src/tests.rs`; completed-only report golden contracts in
  `tailtriage-analyzer/tests/analyzer_fixtures.rs`.
- **Revision criteria:** Require a lifecycle signal that distinguishes operation
  completion from helper Drop, or validation showing the policy systematically
  suppresses/creates wrong leads; assess schema and completed-output stability.
- **Provenance:** Recorded intent (`SPEC.md`, `docs/dev/VALIDATION.md`, and typed tests).

### AN-ATTR-002 — Request-scoped stage samples

- **Rule or default:** Stage attribution groups by `(stage name, request_id)`;
  one attributed duration per distinct completed request owns sample coverage;
  see [percentiles and units](diagnostics.md#percentiles-and-units).
- **Classification:** Hard contract.
- **Problem addressed:** Retries or repeated spans in one request could inflate
  sample count and make a single request satisfy downstream eligibility.
- **Why this shape:** Present-purpose inference: diagnosis asks how broadly a
  stage affects requests, while same-name intervals within one request are an
  attribution problem rather than independent population samples.
- **Tradeoff:** Repeated operations contribute duration but not independent
  sample count, sacrificing invocation-level detail for request-level coverage.
- **Proof owner:** `tailtriage-analyzer/src/stage_attribution.rs` tests for
  distinct requests, same-name overlap, and independent stage names.
- **Revision criteria:** Require an explicit invocation-level reporting model,
  representative retry/fanout evidence, and compatibility analysis for
  `request_samples` semantics.
- **Provenance:** Present-purpose inference; the current contract is recorded in
  typed tests and diagnostics.

## Queue and blocking diagnosis

### AN-QUEUE-001 — Queue eligibility and multi-signal scoring

- **Rule or default:** Queue p95 share controls eligibility; queue share,
  retained start depth, positive in-flight growth, and sample quality contribute
  to score; see
  [application queue saturation](diagnostics.md#application-queue-saturation).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Queue presence alone is common and is not saturation;
  the candidate should require material request impact and rank corroborating
  pressure more strongly.
- **Why this shape:** Present-purpose inference: time share is closest to user
  impact, while depth, growth, and coverage corroborate persistence and breadth.
- **Tradeoff:** Short bursts or poorly instrumented queues may be missed, while
  retained depth is only a sample and can promote a transient event.
- **Proof owner:** Queue formula/threshold tests in
  `tailtriage-analyzer/src/tests.rs` and
  `queue_share_threshold_uses_300_permille_boundary` in
  `tailtriage-analyzer/tests/boundary_thresholds.rs`.
- **Revision criteria:** Require labeled real-service captures or deterministic
  calibration cases showing false positives/negatives, with before/after
  top-1/top-2 and confidence results. Preserve option-path compatibility or
  make an explicit release decision.
- **Provenance:** Present-purpose inference; exact trigger and weights have
  unknown provenance.

### AN-SCORE-002 — Soft caps and clean-extreme exceptions

- **Rule or default:** Queue, blocking, legacy executor, and downstream scores
  are soft-capped unless family-specific extreme evidence is clean; see
  [candidate eligibility and scoring](diagnostics.md#candidate-eligibility-and-scoring).
- **Classification:** Conservative policy.
- **Problem addressed:** Additive weak signals could otherwise reach the same
  ceiling as broad, extreme, well-sampled evidence.
- **Why this shape:** Present-purpose inference: reserve maximum ranking strength
  for unusually clear cases without making the cap absolute.
- **Tradeoff:** Strong but nonconforming cases cluster below the cap; exception
  boundaries add policy complexity.
- **Proof owner:** Family soft-cap boundary tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require corpus evidence that caps distort useful
  ordering or that a simpler general cap performs at least as well across all
  families; review exact-score golden output.
- **Provenance:** Present-purpose inference; exact cap and exception values have
  unknown provenance.

### AN-BLOCK-001 — Persistent blocking eligibility

- **Rule or default:** Blocking is eligible when p95 is nonzero or enough
  retained samples are nonzero; p95, peak, nonzero share, and sample quality
  drive score; see
  [blocking-pool pressure](diagnostics.md#blocking-pool-pressure).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** A sparse percentile can be zero even when blocking
  pressure recurs, while a single nonzero sample should not automatically
  establish sustained pressure.
- **Why this shape:** Present-purpose inference: the alternate persistence path
  retains repeated weak pressure without turning any isolated queueing into a
  diagnosis candidate.
- **Tradeoff:** Very short genuine spikes may be omitted, and repeated tiny
  depths can become eligible despite low p95.
- **Proof owner:** Blocking eligibility/formula tests in
  `tailtriage-analyzer/src/tests.rs` and
  `blocking_and_executor_pressure_require_nonzero_p95_depth` in the boundary
  test module.
- **Revision criteria:** Require sampled-runtime traces demonstrating a better
  persistence measure at realistic sampling intervals and calibration against
  false-positive blocking cases.
- **Provenance:** Present-purpose inference; exact persistence default and score
  weights have unknown provenance.

### AN-BLOCK-002 — Strong-blocking calibration

- **Rule or default:** Configured thresholds define when blocking-pool evidence
  is independently strong for downstream correlation; they do not change the
  blocking score;
  see [blocking-pool pressure](diagnostics.md#blocking-pool-pressure) and
  [downstream dominance](diagnostics.md#downstream-stage-dominance).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Ordinary blocking eligibility is too weak to justify
  constraining a separately observed stage.
- **Why this shape:** Present-purpose inference: require independently material
  runtime evidence without adding it to the blocking score.
- **Tradeoff:** True relationships below either boundary are not correlated.
- **Proof owner:** Strong-blocking boundary tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require representative blocking captures and
  false-correlation cases demonstrating better strong-evidence boundaries.
- **Provenance:** Present-purpose inference for the policy; exact thresholds
  have unknown provenance.

## Executor diagnosis and compatibility

### AN-EXEC-001 — Per-snapshot worker normalization with checked arithmetic

- **Rule or default:** Global plus local runnable depth is combined for each
  snapshot before percentile selection, then expressed in milli-tasks per
  worker using widened, clamped arithmetic; see
  [worker-normalized mode](diagnostics.md#worker-normalized-mode).
- **Classification:** Hard contract.
- **Problem addressed:** Adding independently selected percentiles invents a
  state that may never have occurred; absolute depth is not comparable across
  worker counts; native-width overflow could wrap extreme artifacts.
- **Why this shape:** Recorded intent: normalize the contemporaneous runnable
  state by available executor capacity and keep hostile/extreme input
  deterministic and non-wrapping.
- **Tradeoff:** Integer milli-task precision and clamping lose fractional or
  out-of-range detail, while worker count still approximates effective capacity.
- **Proof owner:** Executor arithmetic and per-snapshot distribution tests in
  `tailtriage-analyzer/src/scoring.rs` and worker-normalization tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require a better capacity denominator or precision
  model, overflow proof, representative runtime validation, and report/output
  compatibility analysis.
- **Provenance:** Recorded intent in focused tests and diagnostics; exact
  milli-task scale is a present-purpose choice.

### AN-EXEC-002 — Normalized executor evidence model

- **Rule or default:** Normalized p95 owns runnable-pressure evidence.
  `alive_tasks` and separate global/local p95 values are
  descriptive, not independent normalized score terms; see
  [worker-normalized mode](diagnostics.md#worker-normalized-mode).
- **Classification:** Hard contract.
- **Problem addressed:** Task population and queue redistribution can correlate
  with workload size without proving runnable pressure, and counting component
  queues again would double-count the normalized signal.
- **Why this shape:** Recorded intent: capacity-normalized contemporaneous depth
  is the pressure model, so correlated population and component views must not
  be counted again.
- **Tradeoff:** Alive-task surges and queue redistribution cannot independently
  raise the normalized score.
- **Proof owner:**
  `normalized_executor_score_ignores_alive_tasks_and_queue_redistribution`,
  worker normalization mode tests in `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require an explicit replacement evidence model,
  double-counting analysis, and compatibility review for normalized reports.
- **Provenance:** Recorded intent for capacity normalization and exclusions.

### AN-EXEC-006 — Normalized executor trigger and contribution bands

- **Rule or default:** Configured normalized-p95 boundaries control eligibility
  and banded score contribution; see
  [AN-EXEC-002](#an-exec-002--normalized-executor-evidence-model) and
  [worker-normalized mode](diagnostics.md#worker-normalized-mode).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Material runnable pressure must be separated from
  ordinary scheduler activity without overreacting to tiny numeric changes.
- **Why this shape:** Present-purpose inference: discrete bands provide stable
  ranking regions on the normalized capacity scale.
- **Tradeoff:** Nearby values on opposite sides of a boundary score differently,
  while values within a band lose fine-grained ordering.
- **Proof owner:** Normalized trigger and band boundary tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require labeled executor-pressure captures showing
  better discrimination and before/after diagnostic calibration.
- **Provenance:** Present-purpose inference for banding; exact trigger and bands
  have unknown provenance.

### AN-EXEC-003 — Exact no-worker legacy compatibility

- **Rule or default:** Artifacts where every relevant snapshot lacks
  `worker_count` retain exact legacy absolute-depth scoring and receive no
  worker-related confidence cap; see
  [legacy compatibility mode](diagnostics.md#legacy-compatibility-mode).
- **Classification:** Compatibility obligation.
- **Problem addressed:** Adding a field in newer capture versions must not
  silently downgrade or rerank historical artifacts that never could provide
  it.
- **Why this shape:** Recorded intent: absence in the historical shape is not
  evidence loss relative to the contract under which it was captured.
- **Tradeoff:** Old and new artifacts use different scales, and historical
  absolute depth cannot account for worker capacity.
- **Proof owner:** Historical fixture goldens and
  `worker_count_enables_normalized_executor_scoring` plus legacy-compatibility
  tests in `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require an explicit compatibility-removal/release
  decision, migration guidance, fixture impact inventory, and evidence that
  preserving legacy reports is more harmful than reranking them.
- **Provenance:** Recorded intent in tests, diagnostics, and changelog history.

### AN-EXEC-004 — Ambiguous worker evidence falls back with a Medium cap

- **Rule or default:** Partial, inconsistent, or zero worker evidence uses
  legacy scoring without inventing a worker count and caps confidence at
  Medium; see
  [executor pressure](diagnostics.md#executor-pressure).
- **Classification:** Conservative policy.
- **Problem addressed:** Normalization with an assumed, changing, or invalid
  denominator creates false precision, but discarding all runnable evidence
  loses a useful lead.
- **Why this shape:** Present-purpose inference: legacy scoring is the only
  defined denominator-free fallback and Medium communicates the unresolved
  capacity ambiguity.
- **Tradeoff:** The fallback may rank differently from the true normalized
  state and preserves two scoring modes.
- **Proof owner:** Partial/inconsistent/zero worker classification and confidence
  tests in `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require a trustworthy way to reconstruct worker count
  per snapshot or a unified denominator-free model validated against all four
  worker-evidence classes.
- **Provenance:** Recorded current policy; historical reason for choosing legacy
  rather than suppressing the candidate is unknown.

### AN-EXEC-005 — Missing local depth is a normalized lower bound

- **Rule or default:** With complete worker counts, missing local depth
  contributes zero only in affected snapshots, labels normalized pressure a
  lower bound, and caps confidence at Medium; see
  [worker-normalized mode](diagnostics.md#worker-normalized-mode).
- **Classification:** Conservative policy.
- **Problem addressed:** Treating an unavailable queue as observed zero
  overstates measurement completeness; abandoning normalization wastes known
  worker and global-depth evidence.
- **Why this shape:** Recorded intent: zero is an arithmetic lower bound, not an
  imputation of the missing measurement.
- **Tradeoff:** Executor pressure can be underestimated, while the candidate
  remains available with bounded confidence.
- **Proof owner:** `missing_local_depth_remains_normalized_lower_bound` and
  `normalized_lower_bound_cap_keeps_higher_score_executor_below_high_confidence_stage`
  in `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require complete local metrics or a validated imputation
  policy that reports uncertainty at least as clearly and improves rankings.
- **Provenance:** Recorded intent in focused tests.

## Downstream diagnosis

### AN-DOWN-001 — Distinct-request minimum and contribution-led score

- **Rule or default:** A stage needs a minimum number of distinct requests;
  tail and cumulative request-latency shares plus coverage drive score, while
  stage p95 is supporting evidence; see
  [downstream-stage dominance](diagnostics.md#downstream-stage-dominance).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** One slow request or many retries within one request can
  make a stage look dominant, and a high standalone stage p95 need not explain
  much total or tail request latency.
- **Why this shape:** Present-purpose inference: breadth and attributed
  contribution better answer “does this stage dominate observed latency?” than
  raw duration alone.
- **Tradeoff:** Rare but severe downstream failures may not qualify; broad
  moderate stages can rank above sharper isolated events.
- **Proof owner:** Distinct-request attribution tests and
  `downstream_stage_requires_at_least_three_samples` in
  `tailtriage-analyzer/tests/boundary_thresholds.rs`; formula tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require real-service or fixture evidence across retries,
  fanout, and rare failures, with accuracy and confidence comparisons. Exact
  defaults need option-compatibility review.
- **Provenance:** Present-purpose inference; exact minimum and weights have
  unknown provenance.

### AN-DOWN-002 — Deterministic stage selection

- **Rule or default:** Stage candidates tie-break by score, tail share,
  cumulative share, completed over lower-bound evidence, then name; see
  [downstream-stage dominance](diagnostics.md#downstream-stage-dominance).
- **Classification:** Hard contract.
- **Problem addressed:** Iteration order must not select a stage, and partial
  evidence should not win an otherwise exact tie.
- **Why this shape:** Present-purpose inference: prioritize explanatory strength
  and evidence completeness, then use lexical order solely for determinism.
- **Tradeoff:** Lexical order is arbitrary at a complete tie.
- **Proof owner:** Stage candidate tie tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require a more meaningful stable identity, deterministic
  adversarial tests, and report-output compatibility analysis.
- **Provenance:** Present-purpose inference; the exact tie sequence has unknown
  historical provenance.

### AN-DOWN-003 — Blocking-correlated stage limit

- **Rule or default:** A stage matching configured blocking patterns stays below
  independently strong blocking-pool evidence by the configured score margin;
  see [AN-BLOCK-002](#an-block-002--strong-blocking-calibration) and
  [downstream-stage dominance](diagnostics.md#downstream-stage-dominance).
- **Classification:** Conservative policy.
- **Problem addressed:** A wrapper-like stage should not outrank strong runtime
  evidence that it mirrors.
- **Why this shape:** Present-purpose inference: pattern correlation preserves
  blocking as the actionable family only when it is independently corroborated.
- **Tradeoff:** Name matching can correlate unrelated stages or miss renamed
  wrappers, and the margin can constrain a genuinely dominant stage.
- **Proof owner:** Blocking-pattern, strong-evidence, and score-margin tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require relationship evidence beyond naming or focused
  false-correlation cases, plus ranking and margin calibration analysis.
- **Provenance:** Present-purpose inference for correlation; exact patterns and
  margin have unknown provenance.

## Confidence and evidence policy

### AN-CONF-001 — Default confidence boundaries

- **Rule or default:** Raw score maps to Low/Medium/High at the configured
  default boundaries before caps; see
  [confidence, ambiguity, and final ordering](diagnostics.md#confidence-ambiguity-and-final-ordering)
  and [option inventory](diagnostics.md#analyzer-tuning-and-configuration-transparency).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Reports need a coarse, readable support level without
  presenting heuristic score points as probability.
- **Why this shape:** Present-purpose inference: three buckets communicate
  investigation priority while leaving room for evidence-aware downgrade.
- **Tradeoff:** Hard bucket edges make adjacent scores appear different and do
  not carry statistical calibration.
- **Proof owner:** Confidence boundary and custom-option tests in
  `tailtriage-analyzer/src/tests.rs`; option registry default tests.
- **Revision criteria:** Require corpus calibration by confidence bucket,
  especially high-confidence-wrong outcomes, and compatibility review for
  report movement. Historical rationale for the exact boundaries is not enough
  by itself to retain or change them.
- **Provenance:** Unknown provenance for exact values; present-purpose inference
  for the three-bucket model.

### AN-CONF-002 — Low completed-request threshold

- **Rule or default:** Completed-request counts below the configured threshold
  reduce evidence quality and candidate confidence; see
  [confidence](diagnostics.md#confidence-ambiguity-and-final-ordering) and
  [evidence quality](diagnostics.md#warnings-confidence-notes-and-evidence-quality).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Sparse evidence can produce extreme percentiles that
  appear better supported than they are.
- **Why this shape:** Present-purpose inference: the default count is a practical
  warning line, not a statistical sufficiency guarantee.
- **Tradeoff:** Small services and short incident windows may remain Medium/Low
  even when their retained evidence is accurate.
- **Proof owner:** Zero/low-request boundary and custom-threshold tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require sample-size studies across representative
  distributions and diagnostic calibration at candidate thresholds.
- **Provenance:** Present-purpose inference; the exact default has unknown
  provenance.

### AN-CONF-003 — Lowest applicable confidence cap wins

- **Rule or default:** All applicable confidence caps compose by selecting the
  lowest confidence; see
  [confidence](diagnostics.md#confidence-ambiguity-and-final-ordering).
- **Classification:** Conservative policy.
- **Problem addressed:** A later or less severe limitation must never undo a
  stronger evidence downgrade.
- **Why this shape:** Recorded intent: independent limitations accumulate
  conservatively rather than depending on evaluation order.
- **Tradeoff:** Multiple limitations cannot offset one another even when their
  evidence is correlated.
- **Proof owner:** Evidence-cap composition, truncation, ambiguity, and
  evaluation-order tests in `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require proof that a replacement remains order
  independent and cannot raise confidence while a stricter limitation applies.
- **Provenance:** Recorded intent in focused confidence-cap tests.

### AN-EVID-002 — Limitations apply at candidate and report scopes

- **Rule or default:** Missing, sparse, partial, and truncated evidence affect
  only relevant candidate families where possible, while report-level quality
  summarizes the retained capture; see
  [warnings, confidence notes, and evidence quality](diagnostics.md#warnings-confidence-notes-and-evidence-quality).
- **Classification:** Conservative policy.
- **Problem addressed:** A global downgrade for every absent optional signal
  would hide strong queue/stage evidence, while ignoring family-specific drops
  would overstate support.
- **Why this shape:** Recorded intent: partial instrumentation remains useful;
  limitation scope follows the evidence a candidate actually consumes.
- **Tradeoff:** Scope-sensitive policy is more complex than one global cap, and
  overall quality need not equal any candidate's confidence.
- **Proof owner:** Candidate-family drop/missing/runtime cap tests and
  evidence-quality coverage tests in `tailtriage-analyzer/src/tests.rs`;
  schema shape in `tailtriage-analyzer/tests/report_schema_contract.rs`.
- **Revision criteria:** Require a concrete mis-scoped limitation with focused
  fixtures; changes must preserve usefulness with partial instrumentation and
  avoid unrelated-family downgrades.
- **Provenance:** Recorded intent (`SPEC.md`, validation docs, and typed tests).

### AN-EVID-003 — Notes, warnings, and structured quality have separate owners

- **Rule or default:** `confidence_notes` explain one candidate's cap;
  `warnings` carry additive interpretation cautions; `evidence_quality` owns
  structured coverage, counts, drops, quality, and limitations; see
  [warnings, confidence notes, and evidence quality](diagnostics.md#warnings-confidence-notes-and-evidence-quality).
- **Classification:** Hard contract.
- **Problem addressed:** Conflating scopes makes machine consumers parse prose,
  repeats every capture limitation on every suspect, or hides why a particular
  candidate was downgraded.
- **Why this shape:** Recorded intent: each surface answers a distinct question
  and all remain deterministic and stably deduplicated.
- **Tradeoff:** Some related limitation information appears in more than one
  surface, requiring readers to inspect all three.
- **Proof owner:** Typed warning/note/evidence-quality tests in
  `tailtriage-analyzer/src/tests.rs`; JSON schema and golden rendering tests.
- **Revision criteria:** Require a versioned report-model proposal that preserves
  machine-readable coverage and candidate-local explanations, plus migration
  analysis for JSON consumers.
- **Provenance:** Recorded intent in report contracts and tests.

## Route and temporal context

### AN-ROUTE-001 — Attributable multi-route slices only

- **Rule or default:** Route analysis needs at least two eligible routes and
  includes only request-attributed request, queue, and stage evidence; global
  runtime and in-flight evidence are excluded; see
  [route breakdowns](diagnostics.md#route-breakdowns).
- **Classification:** Conservative policy.
- **Problem addressed:** A single route has no comparative context, and global
  executor/in-flight samples cannot be assigned to a route without inventing
  attribution.
- **Why this shape:** Recorded intent: route breakdowns explain divergence in
  evidence with stable request identity, not global pressure causality.
- **Tradeoff:** Route-local executor or blocking effects cannot be diagnosed,
  and low-volume routes are omitted.
- **Proof owner:** Route slicing and attribution tests in
  `tailtriage-analyzer/src/route.rs` and scoped fixture tests in
  `tailtriage-analyzer/tests/analyzer_fixtures.rs`.
- **Revision criteria:** Require a capture schema with defensible route-scoped
  runtime attribution or a documented comparison use case; retain explicit
  warnings and global ownership.
- **Provenance:** Recorded intent in focused route tests and diagnostics.

### AN-ROUTE-002 — Route emission calibration and bounded output

- **Rule or default:** Breakdowns emit only for configured suspect or material
  p95 divergence and stop at the configured limit;
  see [route breakdowns](diagnostics.md#route-breakdowns).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Always emitting every route creates noisy, potentially
  huge reports and encourages interpretation of sampling variation.
- **Why this shape:** Present-purpose inference: require a visible reason to
  inspect route context and bound report size.
- **Tradeoff:** Modest or low-volume route differences can be hidden, and the
  output limit may omit a meaningful route.
- **Proof owner:** Route threshold, equality, omission-warning, and limit tests
  in `tailtriage-analyzer/src/route.rs` and analyzer typed tests.
- **Revision criteria:** Require route-volume/report-size evidence and labeled
  divergence cases showing better signal/noise and an appropriate output bound.
- **Provenance:** Present-purpose inference; exact ratios, minimum, and limit
  have unknown provenance.

### AN-ROUTE-003 — Deterministic route ordering

- **Rule or default:** Emitted route breakdowns use the documented stable
  ordering before the configured output limit is applied; see
  [route breakdowns](diagnostics.md#route-breakdowns).
- **Classification:** Hard contract.
- **Problem addressed:** Map or collection order must not change report content,
  especially which routes survive a bound.
- **Why this shape:** Recorded intent: the same artifact and options produce the
  same ordered route context.
- **Tradeoff:** Stable tie-breakers impose an arbitrary final preference.
- **Proof owner:** Route ordering and tied-route limit tests in
  `tailtriage-analyzer/src/route.rs`.
- **Revision criteria:** Require deterministic replacement tests and report
  output/compatibility analysis for reordered or newly omitted routes.
- **Provenance:** Recorded intent in focused route tests; exact tie order has
  unknown provenance.

### AN-TEMP-001 — Temporal eligibility and movement calibration

- **Rule or default:** Temporal analysis requires configured minimum total and
  per-half completed counts and emits only on configured suspect, p95, or share
  movement; see
  [temporal segments](diagnostics.md#temporal-segments).
- **Classification:** Calibrated heuristic.
- **Problem addressed:** Tiny phases and ordinary sample variation can produce
  dramatic-looking changes; unconstrained segmentation invites post-hoc
  narratives.
- **Why this shape:** Present-purpose inference: minimum populations and material
  movement provide a simple drift hint without becoming a change-point engine.
- **Tradeoff:** Short, middle-of-run, or multi-phase incidents can be diluted or
  missed.
- **Proof owner:** Minimum-count, movement-boundary, and suppression tests in
  `tailtriage-analyzer/src/temporal.rs` and scoped temporal fixture tests.
- **Revision criteria:** Require representative temporal captures showing
  improved signal/noise from different eligibility or movement thresholds.
- **Provenance:** Present-purpose inference; exact counts and movement defaults
  have unknown provenance.

### AN-TEMP-003 — Deterministic early/late segmentation

- **Rule or default:** Requests use the documented deterministic ordering and
  balanced early/late split; see
  [temporal segments](diagnostics.md#temporal-segments).
- **Classification:** Hard contract.
- **Problem addressed:** Input collection order must not change segment
  membership, and the same request must not drift between reruns.
- **Why this shape:** Recorded intent: one fixed two-window segmentation keeps
  temporal context reproducible and bounded.
- **Tradeoff:** A balanced split can dilute short, middle-of-run, or multi-phase
  incidents and imposes a deterministic tie preference.
- **Proof owner:** Request-order, equal-timestamp, and early/late split tests in
  `tailtriage-analyzer/src/temporal.rs`.
- **Revision criteria:** Require deterministic replacement tests and analysis of
  segment-membership and report-output compatibility.
- **Provenance:** Recorded intent for deterministic segmentation; historical
  reason for the exact balanced split is unknown.

### AN-TEMP-002 — Timestamp provenance and sparse-runtime restraint

- **Rule or default:** Runtime/in-flight evidence is timestamp-filtered from the
  original Run, prefers run-relative time, falls back to Unix time, warns on
  overlap, and by default suppresses unsupported runtime-kind shifts when
  filtered evidence is sparse; see
  [temporal segments](diagnostics.md#temporal-segments).
- **Classification:** Conservative policy.
- **Problem addressed:** Canonical request filtering must not fabricate snapshot
  membership; wall-clock fallback and concurrent windows are approximate; a
  handful of snapshots can flip an executor/blocking kind without latency or
  share corroboration.
- **Why this shape:** Recorded intent: preserve timestamp provenance, disclose
  approximation, and require supporting movement before elevating sparse
  runtime-only change.
- **Tradeoff:** Genuine brief runtime shifts can be suppressed, and Unix-clock
  behavior is less precise than run-relative timing.
- **Proof owner:** Timestamp filtering, Unix fallback, overlap warning, sparse
  runtime and supporting-movement tests in
  `tailtriage-analyzer/src/temporal.rs`.
- **Revision criteria:** Require denser/stronger timestamp capture or validation
  showing suppressed shifts are reliably actionable; replacement must retain
  provenance and approximation warnings.
- **Provenance:** Recorded intent in focused tests.

### AN-SCOPE-001 — Global diagnosis remains primary

- **Rule or default:** Route and temporal results are supporting context and
  never replace the global `primary_suspect`; see
  [global, route, and temporal analysis](diagnostics.md#global-route-and-temporal-analysis).
- **Classification:** Hard contract.
- **Problem addressed:** Multiple scoped primaries would create competing
  report entry points and overstate attribution from smaller slices.
- **Why this shape:** Recorded intent: the report-to-next-check workflow starts
  with one full-run lead, then uses slices to decide where or when to inspect.
- **Tradeoff:** A severe route- or phase-specific issue remains contextual rather
  than becoming the headline diagnosis.
- **Proof owner:** Route/temporal report construction tests and canonical scoped
  fixture goldens.
- **Revision criteria:** Require an approved report/schema redesign and user
  evidence that scoped ownership improves triage without fragmenting the
  primary workflow.
- **Provenance:** Recorded intent in the analyzer guide and typed report tests.

## Configuration and API boundaries

### AN-CONFIG-001 — Semantic groups and one option registry

- **Rule or default:** Rust builders, TOML groups, CLI overrides, descriptors,
  help, valid paths, defaults, and non-default summaries share semantic option
  groups and one registry; see
  [analyzer tuning](diagnostics.md#analyzer-tuning-and-configuration-transparency).
- **Classification:** Hard contract.
- **Problem addressed:** Duplicated option inventories drift in paths, types,
  defaults, validation, or help and create entry-point-specific tuning behavior.
- **Why this shape:** Recorded intent: progressive disclosure uses one conceptual
  surface across Rust, config files, and CLI.
- **Tradeoff:** Registry machinery is more centralized and every new option must
  supply complete metadata.
- **Proof owner:** Registry tests in
  `tailtriage-analyzer/src/options/overrides.rs` and descriptor/default/path
  tests in `tailtriage-analyzer/src/tests.rs`; CLI help/config tests.
- **Revision criteria:** Require a replacement single source of truth with typed
  parity tests; do not add an option in only one entry point.
- **Provenance:** Recorded intent (`SPEC.md` and option-registry tests).

### AN-CONFIG-002 — Only non-default configuration is reported

- **Rule or default:** Default reports omit `analyzer_config`; non-default
  overrides are included; see
  [analyzer tuning](diagnostics.md#analyzer-tuning-and-configuration-transparency).
- **Classification:** Compatibility obligation.
- **Problem addressed:** Adding configuration transparency must not churn the
  established default JSON shape, while tuned reports must disclose why their
  interpretation may differ.
- **Why this shape:** Recorded intent: preserve default report compatibility and
  make deviations reproducible without serializing the full defaults table.
- **Tradeoff:** A default report does not embed the analyzer defaults/version,
  so exact later reproduction also depends on the analyzer version.
- **Proof owner:** `analyzer_config_transparency_default_report_omits_config`
  and `analyzer_config_transparency_non_default_report_includes_config` in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require an explicit report-versioning and reproducibility
  decision with JSON compatibility analysis.
- **Provenance:** Recorded intent (`SPEC.md` and focused tests).

### AN-CONFIG-004 — Deterministic reported override order

- **Rule or default:** Reported non-default analyzer overrides use stable
  deterministic ordering; see
  [analyzer tuning](diagnostics.md#analyzer-tuning-and-configuration-transparency).
- **Classification:** Hard contract.
- **Problem addressed:** Registry or map iteration must not churn otherwise
  identical report output.
- **Why this shape:** Recorded intent: tuned interpretation remains reproducible
  and exact rendering stays stable.
- **Tradeoff:** The fixed order is presentation policy rather than semantic
  priority.
- **Proof owner:** Exact analyzer-config render and registry-order tests in
  `tailtriage-analyzer/src/tests.rs`.
- **Revision criteria:** Require deterministic replacement ordering and explicit
  JSON output/compatibility analysis.
- **Provenance:** Recorded intent in exact rendering tests.

### AN-API-001 — Checked and panicking entry points validate identically

- **Rule or default:** Free and reusable analyzer entry points share semantic
  option validation; panicking forms panic on invalid options and checked forms
  return `AnalyzeConfigError`; see
  [scope and sources of truth](diagnostics.md#scope-inputs-and-sources-of-truth).
- **Classification:** Hard contract.
- **Problem addressed:** Reusing `Analyzer` or choosing convenience functions
  must not admit an invalid configuration that another equivalent entry point
  rejects.
- **Why this shape:** Recorded intent: one checked implementation owns validity;
  panic versus `Result` is caller ergonomics, not a policy difference.
- **Tradeoff:** Convenience APIs can panic on programmer-supplied invalid
  options; checked users carry explicit error handling.
- **Proof owner:** Entry-point parity and invalid-option tests in
  `tailtriage-analyzer/src/tests.rs` and public API contract tests.
- **Revision criteria:** Any API evolution must keep a shared validation path and
  parity tests; changing panic behavior requires public API compatibility review.
- **Provenance:** Recorded intent in implementation and focused tests.

### AN-API-002 — Permissive library normalization, strict saved artifacts

- **Rule or default:** In-process analysis canonically normalizes request-scoped
  evidence and warns; default CLI saved-artifact analysis strictly rejects
  invalid relationships; see
  [scope and sources of truth](diagnostics.md#scope-inputs-and-sources-of-truth).
- **Classification:** Compatibility obligation.
- **Problem addressed:** Typed in-process callers need useful partial analysis
  of snapshots and legacy/ambiguous inputs, while persisted artifacts are a
  trust boundary where silently discarding malformed relationships would hide
  data-integrity failures.
- **Why this shape:** Recorded intent: core owns one deterministic normalization
  policy and explicit strict validation; CLI defaults to the safer artifact
  contract while exposing an intentional permissive escape hatch.
- **Tradeoff:** The same malformed Run can produce a warned library report but a
  default CLI error, so callers must understand the boundary.
- **Proof owner:** Strict/permissive analyzer tests in
  `tailtriage-analyzer/src/tests.rs`; artifact boundary and JSON parity tests in
  `tailtriage-cli/tests/cli_boundary.rs` and `json_parity.rs`.
- **Revision criteria:** Require a product-level boundary decision, migration
  plan, and parity tests that preserve deterministic normalization and visible
  integrity notices.
- **Provenance:** Recorded intent (`SPEC.md`, Rustdoc, and CLI/analyzer tests).

### AN-CONFIG-003 — Tuning changes interpretation, not evidence

- **Rule or default:** Analyzer options tune interpretation of already captured
  evidence; they cannot restore missing fields, partial completions, truncation,
  or dropped events; see
  [analyzer tuning](diagnostics.md#analyzer-tuning-and-configuration-transparency)
  and [known limitations](diagnostics.md#non-claims-and-known-limitations).
- **Classification:** Hard contract.
- **Problem addressed:** Lowering thresholds can be mistaken for repairing a
  weak capture and can produce a more confident-looking but no better-supported
  lead.
- **Why this shape:** Recorded intent: capture policy and analyzer interpretation
  are separate surfaces; evidence-aware caps still apply under tuning.
- **Tradeoff:** Users must rerun with better instrumentation rather than tune
  around absent evidence.
- **Proof owner:** Custom-options/evidence-cap tests in
  `tailtriage-analyzer/src/tests.rs`; option registry limits the configurable
  surface to interpretation controls.
- **Revision criteria:** A proposed option that changes evidence must belong to
  capture configuration with lifecycle/retention tests, not be added as an
  analyzer threshold.
- **Provenance:** Recorded intent (`SPEC.md`, analyzer guide, and option tests).

## Deferred simplification candidates

The focused simplification review resolved the duplicated route/temporal ratio
arithmetic: one private checked comparison now owns inclusive boundaries,
zero-baseline rejection, and exact `u128` cross-multiplication. Route and
temporal call sites continue to own which signals qualify as movement, their
distinct emission knobs, and their attribution limits. Analyzer option paths are
preserved, and ordinary committed fixture-scale output is preserved. The exact
widened arithmetic removes saturation-related false-positive movement detection
for extreme valid integer values; the implementation and focused tests remain
authoritative for this boundary.

The remaining questions require broader evidence and stay inputs to the
holistic review, not rationales or proposed behavior:

1. **Soft-cap calibration:** cap application is already mechanical through one
   helper, while each clean-extreme predicate represents family-specific
   evidence requirements. Any further consolidation risks changing exact scores
   or eligibility and requires the corpus comparison and calibration evidence
   in AN-SCORE-002.
2. **Confidence limitation ownership:** limitation text spans intentionally
   distinct candidate-note, report-warning, and evidence-quality scopes. A
   single typed model would currently add scope and rendering branches without
   eliminating the family policy decisions. Reconsider only with a complete
   limitation-to-scope matrix and exact global, route, temporal, ordering,
   deduplication, and JSON parity tests; public typing remains separate schema
   work.
3. **Dual executor modes:** history and focused tests establish exact behavior
   for artifacts that predate worker counts, but the repository cannot establish
   the external historical-artifact population. AN-EXEC-003 remains binding.
   Retirement requires an explicit release decision, supported-version and
   artifact-population evidence, migration guidance, and corpus-wide ranking and
   fixture impact analysis.

These candidates do not justify changing exact heuristics merely because their
historical calibration is unknown. Revision still requires the evidence named
in the relevant entries.
