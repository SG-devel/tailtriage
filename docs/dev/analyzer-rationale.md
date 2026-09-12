# Analyzer rationale and revision criteria

This maintainer catalog explains the current design role of analyzer policies and numeric defaults. It complements the exact [analyzer behavior reference](../diagnostics.md): that page owns mechanics and the complete public option inventory; this page owns retention rationale, tradeoffs, proof owners, limitations, and revision criteria.

Analyzer output is deterministic **triage**: evidence-ranked suspects and next checks. A suspect is a lead, not proof of root cause. Scores are bounded ordering inputs within one report, confidence labels are buckets rather than probabilities, and synthetic cases characterize mechanics rather than production truth.

Each entry is classified as one of:

- **semantic contract** — changing it alters the meaning of evidence or output;
- **heuristic default** — a tunable boundary retained while its present behavior is understood;
- **compatibility policy** — preserves useful interpretation of currently accepted evidence;
- **presentation policy** — bounds or orders supporting output.

## Numeric calibration findings

The current numeric geometry is internally coherent enough to retain. Controlled sensitivity cases found active boundaries and locally monotone behavior, but no nearby retuning demonstrated a clearly better diagnostic tradeoff across the controlled cases. This does **not** establish that any exact heuristic value is statistically optimal, probability-calibrated, or universally accurate.

The percentile estimator is deterministic and non-interpolated. At p95, 19 and 20 samples select the maximum; 21 samples select the second-highest value. The shared value `20` is therefore an evidence-maturity policy threshold, not a guarantee that p95 has a resolved 5% tail or is insulated from one maximum. The shared sample-quality steps at 8, 20, 40, and 100 are active as documented.

Queue eligibility, score contributions, cap, and clean-extreme conjunction are active and locally coherent in tested neighborhoods. Blocking scoring has a mathematical maximum of `32 + 24 + 12 + 12 + 8 = 88`; consequently its soft cap of 94 and current clean-extreme bypass cannot affect output. They are inert current machinery and cleanup candidates, not a release defect or evidence that the blocking family is wrong.

Worker-normalized executor bands are monotone in the intended direction and, with corroboration, traverse Low, Medium, and High. They are step functions; their exact edges and jumps remain heuristics needing representative real-world calibration before stronger claims. Default confidence boundaries 65/85 actively partition current proportional scores, but are neither probabilities nor proven-optimal cutoffs.

Three distinct attributed requests make a downstream candidate eligible. In a sufficiently large Run, dominant tail and cumulative stage shares can then produce High confidence. Controlled evidence demonstrates this interaction; it establishes neither that the classification is wrong nor that 4 or 5 is a better default. Stage-specific confidence maturity remains a characterization question.

Route and temporal ratios and materiality thresholds are exact inclusive gates under integer/rational arithmetic. Their exact calibration is not proven optimal. In particular, small temporal segments interact sharply with the percentile estimator: p95 at 8 or 10 samples is the maximum. Synthetic sensitivity evidence exposes these behaviors; it does not establish production diagnostic accuracy or causality.

## Representation, percentiles, and ranking

### AN-MODEL-001 — Evidence ranking, not probability or causality

- **Classification:** semantic contract.
- **What it controls:** reports rank retained evidence and recommend next checks; they do not assign causal probability.
- **Tradeoff:** bounded claims are less dramatic but remain defensible with partial instrumentation.
- **Proof owner:** typed report tests and the committed diagnostic corpus.
- **Revision criteria:** require a separately approved product and evidence model; do not infer causality from score calibration alone.

### AN-SCORE-001 — Bounded score representation and fallback

- **Classification:** semantic contract plus heuristic fallback.
- **Default/value and unit:** scores are integer points clamped to `0..=100`; shares are integer permille clamped to `0..=1000`; `insufficient_evidence` receives score 50.
- **What it controls:** bounded arithmetic makes candidate ordering and serialization stable. The fallback remains visible but sorts after eligible diagnosis candidates.
- **Boundary effect:** arithmetic floors before the final score clamp. Share calculations cannot exceed the whole request. The fallback is 50—not the 20-request evidence threshold.
- **Why retain now:** current formulas, ranking, fixtures, and consumers share these representations; 50 keeps insufficient evidence neutral without competing with eligible candidates because kind ordering handles it explicitly.
- **Current evidence does not establish:** that 100 is an absolute severity scale or that 50 has probabilistic meaning.
- **Proof owner:** scoring/ranking unit tests and report fixtures.
- **Revision criteria:** change only with a coherent report-model migration and ordering tests.

### AN-PCTL-001 — Deterministic non-interpolated percentiles

- **Classification:** semantic contract.
- **Default/value and unit:** reports use p50, p95, and p99; percentile `p/q` selects `ceil((n-1)*p/q)` in an ascending nonempty series.
- **What it controls:** request latency, queue/service shares, stage evidence, and runtime series use one estimator.
- **Boundary effect:** p95 selects the maximum at `n=19` and `n=20`, then the second-highest at `n=21`. At 8 or 10 samples it is also the maximum.
- **Why retain now:** it is simple, deterministic, integer-only, and used consistently by current mechanics and tests.
- **Current evidence does not establish:** statistical superiority over interpolated estimators or robust tail resolution at small sample counts.
- **Proof owner:** percentile unit/boundary tests and deterministic sensitivity cases.
- **Revision criteria:** require corpus-wide comparison, explicit small-sample semantics, fixture review, and migration of every dependent formula.

### AN-ATTR-001 — Bounded request attribution

- **Classification:** semantic contract.
- **What it controls:** overlapping precise intervals are unioned per request; incomplete timing falls back to saturating duration sums capped by request latency. Stage samples count distinct requests.
- **Tradeoff:** prevents double counting while approximate timing loses overlap precision.
- **Proof owner:** attribution unit tests and generic Run validation tests.
- **Revision criteria:** require a more truthful representation that preserves partial-evidence labeling and request bounds.

### AN-RANK-001 — Confidence-first ordering and ambiguity

- **Classification:** semantic contract plus heuristic defaults.
- **Default/value and unit:** ambiguity requires a top score of at least 60 and includes candidates within an inclusive 4 score-point gap.
- **What it controls:** evidence-aware confidence caps run before final ordering; ambiguity uses raw scores and caps cluster members at Medium.
- **Boundary effect:** a gap of exactly 4 qualifies. Lower raw score can rank first after evidence caps.
- **Why retain now:** the gates prevent weak low-score proximity from producing noise while surfacing meaningful near-ties in current cases.
- **Current evidence does not establish:** that 60 or 4 is optimal across workloads.
- **Proof owner:** confidence/ranking tests and ambiguity fixtures.
- **Revision criteria:** representative mixed-family cases must measure warning usefulness, ordering, and confidence effects.

## Shared evidence maturity

### AN-CONF-001 — Confidence buckets

- **Classification:** heuristic defaults.
- **Default/value and unit:** Medium begins at 65 score points and High at 85, both inclusive.
- **What it controls:** the initial Low/Medium/High bucket before evidence-aware caps.
- **Boundary effect:** `<65` is Low, `65..84` Medium, and `85+` High. Caps can only reduce the result.
- **Why retain now:** sensitivity cases show both boundaries actively and meaningfully partition current proportional scores; nearby configurations did not establish a better general tradeoff.
- **Current evidence does not establish:** probability calibration, statistical optimality, or universal accuracy.
- **Proof owner:** confidence tests, option-boundary tests, and diagnostic corpus.
- **Revision criteria:** use representative labeled cases across all families and assess both classification and mixed-family ordering.

### AN-CONF-002 — Completed-request evidence threshold

- **Classification:** heuristic evidence-maturity policy.
- **Default/value and unit:** fewer than 20 completed requests yields weak quality and a Medium confidence cap; zero requests has a Low cap.
- **What it controls:** report quality, warnings, and confidence—not raw candidate scoring.
- **Boundary effect:** 19 is below the gate; 20 is not. This gate is independent of p95's order statistic.
- **Why retain now:** it provides a deterministic conservative maturity step that is active in controlled cases and compatibility-sensitive.
- **Current evidence does not establish:** statistical sufficiency, resolved 5% tails, or immunity from a single maximum at 20.
- **Proof owner:** evidence/confidence tests and sensitivity cases.
- **Revision criteria:** require representative false-positive/false-negative evidence across request counts, not percentile intuition alone.

### AN-SCORE-002 — Shared sample-quality contribution

- **Classification:** heuristic scoring policy.
- **Default/value and unit:** series lengths `<8`, `8..19`, `20..39`, `40..99`, and `100+` contribute 0, 1, 3, 5, and 8 score points respectively.
- **What it controls:** a bounded corroboration contribution shared by queue, blocking, executor, and downstream scores.
- **Boundary effect:** discontinuous inclusive steps occur at 8, 20, 40, and 100.
- **Why retain now:** every step is active, monotone, deterministic, and small relative to family evidence contributions.
- **Current evidence does not establish:** statistical sufficiency at any step or that these exact jumps outperform nearby values.
- **Proof owner:** scoring unit tests and sensitivity matrix.
- **Revision criteria:** assess all families together because a shared change can reorder mixed-family reports.

### AN-CONF-003 — Lowest evidence cap wins

- **Classification:** semantic contract.
- **What it controls:** ambiguity, truncation, missing fields, partial lower bounds, worker limitations, and low counts compose conservatively.
- **Tradeoff:** strong raw signals can remain visibly cautious.
- **Proof owner:** confidence-cap tests and adversarial corpus cases.
- **Revision criteria:** require evidence that a specific cap is redundant or incorrectly scoped without weakening other limitations.

## Queue diagnosis

### AN-QUEUE-001 — Queue eligibility and score geometry

- **Classification:** heuristic defaults.
- **Default/value and unit:** eligibility is p95 queue share `>=300` permille. Score is base 22 plus `floor(min(share,1000)/14)`, `floor(min(depth,40)*2/3)`, 5 for known positive in-flight growth, and sample quality.
- **What it controls:** 300 admits a candidate; the configured trigger is not part of an admitted candidate's score. Depth caps at 40 and contributes at most 26.
- **Boundary effect:** eligibility is inclusive. Share and depth contributions floor; growth is a discrete corroboration bonus.
- **Why retain now:** threshold/cap behavior is active and locally coherent, and reviewed nearby triggers did not demonstrate a clearly better tradeoff.
- **Current evidence does not establish:** that 300, 22, 14, 40, 2/3, or 5 is optimal across services.
- **Proof owner:** queue scoring tests, 299/300 boundary test, corpus, and sensitivity matrix.
- **Revision criteria:** measure candidate admission, score ordering, and mixed-family false positives/negatives on representative Runs.

### AN-QUEUE-002 — Queue cap and clean extreme

- **Classification:** heuristic restraint.
- **Default/value and unit:** ordinary score cap 95; bypass requires share `>=985` permille, depth `>=12`, at least 20 share samples, and known positive in-flight growth.
- **What it controls:** reserves scores above 95 for a conjunction of dominant queue time and corroborating evidence.
- **Boundary effect:** all four inclusive conditions are required; changing one below its gate restores the cap.
- **Why retain now:** the conjunction is active and locally coherent in controlled neighborhoods.
- **Current evidence does not establish:** optimality of the cap or any exact gate.
- **Proof owner:** clean-extreme conjunction tests and sensitivity cases.
- **Revision criteria:** require representative extremes and near-misses, including partial/truncated evidence and competing families.

## Blocking diagnosis

### AN-BLOCK-001 — Persistent blocking eligibility and score

- **Classification:** heuristic defaults.
- **Default/value and unit:** if p95 is zero, at least 2 nonzero blocking-depth samples are needed. Score is base 32 plus p95 capped at 24, peak capped at 24 then divided by 2, nonzero-share permille divided by 80, and sample quality.
- **What it controls:** the alternate eligibility path retains intermittent persistence evidence; score combines typical depth, peak, persistence, and maturity.
- **Boundary effect:** peak contributes at most 12 and nonzero share at most 12. The raw mathematical maximum is `32 + 24 + 12 + 12 + 8 = 88`.
- **Why retain now:** the signal is deterministic, monotone in its components, and current tests cover zero-p95 persistence.
- **Current evidence does not establish:** optimality of 2 or any weight/divisor.
- **Proof owner:** blocking scoring/eligibility tests and deterministic sensitivity cases.
- **Revision criteria:** representative intermittent and sustained blocking cases must show improved eligibility and family ordering.

### AN-BLOCK-002 — Blocking cap and strong correlation gates

- **Classification:** inert machinery plus heuristic correlation policy.
- **Default/value and unit:** ordinary cap 94; clean bypass at p95 `>=16`, peak `>=24`, and nonzero share `>=900` permille. Strong blocking requires p95 `>=12`, peak `>=20`, share `>=700`, and at least 30 samples.
- **What it controls:** the clean bypass is intended to govern the cap; strong gates separately govern blocking-looking downstream correlation.
- **Boundary effect:** because raw blocking score cannot exceed 88, the 94 cap and clean bypass currently cannot affect output. Strong gates are an inclusive conjunction and remain active for correlation.
- **Why retain now:** inert machinery is compatibility-sensitive cleanup, not a behavior defect; strong gates bound when stage-name correlation changes ordering.
- **Current evidence does not establish:** optimal strong gates or a reason to treat the inert cap as validation failure.
- **Proof owner:** blocking/downstream correlation tests and formula-bound review.
- **Revision criteria:** cleanup should be explicit and behavior-neutral; correlation retuning requires mixed blocking/downstream cases.

## Executor diagnosis

### AN-EXEC-001 — Worker evidence classification

- **Classification:** compatibility and evidence-quality policy.
- **What it controls:** complete stable nonzero worker counts select normalized scoring. Worker-count absence selects absolute fallback without a worker cap. Partial, inconsistent, or zero evidence selects fallback with a Medium cap. Missing local depth under otherwise complete worker evidence is a normalized lower bound with a Medium cap.
- **Tradeoff:** accepted older/partial artifacts remain useful without inventing worker counts.
- **Proof owner:** worker classification, compatibility, and confidence tests.
- **Revision criteria:** require an artifact-contract change or a more truthful inference backed by captured evidence.

### AN-EXEC-002 — Worker-normalized eligibility and bands

- **Classification:** heuristic defaults.
- **Default/value and unit:** eligibility is normalized p95 `>=500` milli-tasks/worker; `1000` means one runnable task per worker. Bands `500..999`, `1000..1999`, `2000..3999`, `4000..7999`, and `8000+` contribute 5, 15, 25, 40, and 55 points. Base is 34, known positive growth adds 4, and sample quality is added.
- **What it controls:** per-snapshot `(global+local)*1000/workers` normalization makes pressure comparable across worker counts.
- **Boundary effect:** integer division floors and bands are discontinuous step functions. There is no normalized soft cap beyond the final `0..=100` clamp.
- **Why retain now:** bands are monotone in the intended direction and traverse Low/Medium/High with corroboration; reviewed nearby behavior did not demonstrate a better overall geometry.
- **Current evidence does not establish:** that 500, band edges, jump sizes, 34, or 4 is optimal or universally calibrated.
- **Proof owner:** checked-arithmetic, band-boundary, confidence, and sensitivity tests.
- **Revision criteria:** representative worker-count/workload cases must measure eligibility, discontinuities, and mixed-family ordering.

### AN-EXEC-003 — Absolute-depth compatibility scoring

- **Classification:** compatibility policy with heuristic constants.
- **Default/value and unit:** global p95 eligibility `>=1`; base 34; global contribution `floor(min(P,150)/4)`; local `floor(min(L,60)/6)`; alive tasks `floor(min(A,400)/40)`; growth 4; sample quality; ordinary cap 94 bypassed by global p95 `>=140` with at least 30 global samples.
- **What it controls:** interprets artifacts where worker normalization is unavailable or unreliable.
- **Boundary effect:** caps bound each component; the clean exception is an inclusive conjunction.
- **Why retain now:** it preserves exact useful behavior for accepted no-worker and ambiguous evidence while limitations remain visible.
- **Current evidence does not establish:** optimal absolute thresholds or parity with normalized meaning.
- **Proof owner:** exact fallback compatibility tests and evidence-cap tests.
- **Revision criteria:** do not silently remove; require migration evidence and explicit treatment of every worker classification.

## Downstream diagnosis

### AN-DOWN-001 — Distinct-request eligibility and score

- **Classification:** heuristic defaults.
- **Default/value and unit:** at least 3 distinct attributed requests; base 24; tail-share permille divided by 11; cumulative-share permille divided by 35; sample quality; ordinary cap 95.
- **What it controls:** request-scoped eligibility resists repeated events from one request while share dominance drives strength.
- **Boundary effect:** three requests are sufficient for eligibility. With a mature Run, dominant shares, and no other cap, an extreme three-sample stage can be High.
- **Why retain now:** current evidence demonstrates the interaction but neither shows it is wrong nor that 4 or 5 gives a better general tradeoff.
- **Current evidence does not establish:** an optimal maturity threshold, false-positive rate, or universal stage attribution accuracy.
- **Proof owner:** distinct-request attribution tests, boundary test, diagnostic corpus, and sensitivity matrix.
- **Revision criteria:** representative cases must measure false positives, false negatives, and mixed-family ordering before changing eligibility or adding a stage-specific confidence cap.

### AN-DOWN-002 — Downstream clean extreme and deterministic selection

- **Classification:** heuristic restraint plus semantic tie policy.
- **Default/value and unit:** cap bypass requires tail share `>=960`, cumulative share `>=920`, and at least 20 stage samples.
- **What it controls:** the cap reserves the upper range for broad, dominant stage evidence. Candidate ties resolve by score, tail share, cumulative share, completed basis, then stage name.
- **Boundary effect:** all clean gates are inclusive; otherwise 95 applies.
- **Why retain now:** deterministic selection and active sample gate keep output stable and evidence-led.
- **Current evidence does not establish:** optimality of 960, 920, 20, or 95.
- **Proof owner:** downstream selection/cap tests and sensitivity cases.
- **Revision criteria:** include multiple stages, partial evidence, near-gate cases, and stable ordering assertions.

### AN-DOWN-003 — Blocking-correlated stage margin

- **Classification:** heuristic interpretation policy.
- **Default/value and unit:** configured case-insensitive stage-name patterns (`spawn_blocking`, `blocking_path`, `blocking`) combine with strong blocking evidence; downstream is limited to `blocking_score - 2` score points, saturating at zero.
- **What it controls:** the names only identify possible correlation; the 2-point margin keeps independently strong runtime blocking evidence ahead.
- **Boundary effect:** without both a matching name and every strong-blocking gate, no margin applies.
- **Why retain now:** it avoids double-reading one blocking path as a stronger downstream lead while keeping evidence explicit.
- **Current evidence does not establish:** that names prove implementation behavior or that 2 is optimal.
- **Proof owner:** correlation and configuration tests.
- **Revision criteria:** require representative false-match, true-match, and mixed-family cases.

## Route and temporal context

### AN-ROUTE-001 — Structural route minimum and bounded output

- **Classification:** structural and presentation policy.
- **Default/value and unit:** at least 3 completed requests per route, at least 2 eligible routes, and at most 10 emitted breakdowns.
- **What it controls:** 3 and 2 make comparison structurally possible; 10 bounds report size.
- **Boundary effect:** routes below 3 are omitted and warned; fewer than two eligible routes emits no comparison.
- **Why retain now:** these minima prevent one-route/non-comparison output and keep reports usable.
- **Current evidence does not establish:** statistical sufficiency at three requests.
- **Proof owner:** route eligibility/order/limit tests.
- **Revision criteria:** distinguish comparison structure from statistical claims and preserve deterministic ordering.

### AN-ROUTE-002 — Route materiality gates

- **Classification:** heuristic defaults.
- **Default/value and unit:** slowest:fastest p95 `>=3/2`; slowest:global p95 `>=5/4`, both exact inclusive ratios.
- **What it controls:** emits supporting route slices when latency disparity is material even without suspect-kind divergence.
- **Boundary effect:** checked cross-multiplication avoids floating point; equality passes.
- **Why retain now:** gates are deterministic, active, and separate local route context from the global diagnosis.
- **Current evidence does not establish:** optimal calibration of either ratio.
- **Proof owner:** exact ratio boundary and route emission tests.
- **Revision criteria:** representative heterogeneous-route cases must assess useful output versus noise.

### AN-TEMP-001 — Temporal structure and movement

- **Classification:** structural minima plus heuristic movement defaults.
- **Default/value and unit:** at least 20 total requests and 8 per segment; exactly two halves with `floor(n/2)` early requests; queue/service share movement `>=200` permille; p95 movement `>=3/2`.
- **What it controls:** enables early/late supporting context and determines material movement.
- **Boundary effect:** gates and ratios are inclusive. At the minimum 20 requests, halves have 10 each; at 8 or 10 samples p95 is the segment maximum, so one observation can control it.
- **Why retain now:** exact arithmetic and two deterministic halves keep output reproducible; current controlled cases characterize active boundaries.
- **Current evidence does not establish:** that 20 or 8 is statistically sufficient, or that 200 and 3/2 are optimal.
- **Proof owner:** temporal split/order, exact ratio/share boundary, and sensitivity tests.
- **Revision criteria:** representative phased and steady workloads must measure false movement, missed movement, and interaction with other configured temporal conditions. A synthetic segment count alone must not be treated as isolated proof of the share gate when another movement condition also passes.

### AN-TEMP-002 — Timing precision and sparse-runtime restraint

- **Classification:** evidence-quality policy.
- **What it controls:** complete run-relative timing is preferred; Unix-ms fallback is warned. Runtime-dependent suspect shifts are suppressed by default when segment evidence is sparse and no supporting movement exists.
- **Tradeoff:** avoids over-reading approximate attribution while retaining useful request/stage/queue context.
- **Proof owner:** temporal precision and sparse-runtime tests.
- **Revision criteria:** require better attributable runtime evidence or demonstrated suppression error.

### AN-SCOPE-001 — Global diagnosis remains primary

- **Classification:** semantic/presentation contract.
- **What it controls:** route and temporal analyses are supporting context and never replace the full-Run primary suspect.
- **Proof owner:** report-shape and route/temporal tests.
- **Revision criteria:** changing this would require a separately approved product model, not numeric retuning.

## Configuration and proof boundaries

### AN-CONFIG-001 — One semantic option registry

- **Classification:** API/configuration contract.
- **What it controls:** Rust fields, TOML, CLI overrides, descriptors, validation, and reported non-defaults converge on one `AnalyzeOptions` model.
- **Proof owner:** option registry parity, TOML, CLI, and Rustdoc tests.
- **Revision criteria:** preserve one coherent tuning surface and exact default parity.

### AN-CONFIG-002 — Tuning changes interpretation, not capture

- **Classification:** product boundary.
- **What it controls:** options re-interpret already captured evidence; they cannot recover missing/truncated data or change capture limits.
- **Proof owner:** analyzer configuration/report tests and public docs contracts.
- **Revision criteria:** capture behavior belongs to capture configuration, not analyzer defaults.

### AN-API-001 — Checked analyzer operation and artifact boundary

- **Classification:** API/compatibility contract.
- **What it controls:** `analyze_run` validates options and permissively normalizes typed Runs; saved CLI artifacts are strict unless the explicit permissive flag is used.
- **Proof owner:** analyzer API, core validation, and CLI artifact tests.
- **Revision criteria:** preserve explicit errors and do not conflate artifact validity with diagnostic calibration.

## Deferred simplification candidates

The blocking 94 soft cap and its `p95>=16`, `peak>=24`, `nonzero share>=900` bypass are inert under the current maximum raw score of 88. They may be considered for explicit behavior-neutral cleanup, but their presence is not a release defect and this catalog does not recommend a scoring change. Any cleanup should prove report equivalence across blocking and blocking-correlated downstream cases.
