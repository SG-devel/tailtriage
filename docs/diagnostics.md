# Analyzer behavior reference

This page is the detailed reference for the current `tailtriage-analyzer`
behavior. Start with the concise [analyzer guide](analyzer-guide.md) when the
goal is simply to turn a report into a next check. This page instead records
the contracts needed to review scoring and interpretation changes. It records
behavior, not the rationale for the numeric defaults.

For why major policies exist, their tradeoffs and proof owners, and the evidence
required to revise them, see the [analyzer rationale catalog](analyzer-rationale.md).

Suspects are deterministic, evidence-ranked triage leads. Scores are not
probabilities, and suspects are **not proof** of root cause.

## Scope, inputs, and sources of truth

| Term | Meaning |
| --- | --- |
| Run artifact JSON | Persisted capture input. The CLI validates its schema and lifecycle requirements before analysis. |
| typed `Run` | `tailtriage_core::Run`, accepted directly by the in-process analyzer. |
| typed `Report` | The analyzer result and source for text or JSON rendering. |
| Report JSON | Serialization of `Report`; it is output, not a reusable Run artifact. |

`analyze_run` validates `AnalyzeOptions`, returns `AnalyzeConfigError` for invalid options, and permissively normalizes request-scoped evidence. Invalid or orphaned events are discarded or canonicalized and the result carries validation warnings. Callers that require strict in-process acceptance explicitly compose `tailtriage_core::validate_run_strict(&run)?` before `analyze_run(&run, options)?`. Saved-artifact CLI analysis is strict
by default and emits loader/lifecycle notices on stderr; see the
[`tailtriage-cli` README](../tailtriage-cli/README.md) for command and schema
mechanics.

Run JSON schema version 2 is current. `metadata.finalized_at_unix_ms` is the
sole run-level finalization timestamp (an active in-memory snapshot may contain
`null`, while a persisted CLI artifact requires numeric finalization). Schema-v1 Run JSON is
rejected by the CLI and must be regenerated. Event-level completion timestamps remain unchanged.

Completed request, queue, and stage events form the ordinary distributions.
An incomplete queue or stage observation can additionally enter a labeled
**observed lower-bound** candidate: observation ended at Drop, which does not
establish that the underlying operation completed, failed, or stopped.

Implementation and typed tests are authoritative for behavior. Public Rustdoc
owns item-level API details; this document owns analyzer interpretation.

## Field reference: percentiles, attribution, and shared units

### Percentiles and units

For a nonempty ascending series of length `n`, percentile `p/q` selects index
`ceil((n - 1) * p / q)`, clamped to `n - 1`. Empty input produces no
percentile. Thus all p95 values below use `ceil((n - 1) * 95 / 100)`; this is
not interpolation.

| Unit | Use |
| --- | --- |
| microseconds (`*_us`) | request, queue, stage, and attributed durations |
| permille (`0..=1000`) | per-request queue/service shares and aggregate stage shares; integer division floors |
| counts | queue depths, task counts, samples, and events |
| milli-tasks per worker | normalized runnable depth; `1000` means one runnable task per worker |

The public queue and service share distributions are completed-only. For each
nonzero-latency completed request, attributed completed queue wait is capped at
request latency. Queue share is `floor(wait * 1000 / latency)` and service share
is `floor((latency - wait) * 1000 / latency)`, each capped at 1000. Their
separately selected p95 values need not sum to 1000.

Queue attribution unions complete run-relative intervals, so duplicates,
nested intervals, touching intervals, and overlaps do not double count. If any
input for a request lacks a complete run-relative interval, attribution instead
saturating-sums authoritative durations and caps the result at request latency.

Stage attribution first groups by `(stage name, request_id)`. Within each group
it uses the same interval-union or capped-duration fallback and produces one
duration per distinct completed request. Stage names remain independent.
`request_samples` is therefore a distinct-request count, not a raw event count.
The cumulative share divides summed attributed stage duration by all completed
request latency. Tail share divides attributed duration for requests whose
latency is at least request p95 by total latency of those tail requests.

Observed lower-bound queue candidates include completed and partial queue
events. Observed lower-bound stage summaries likewise include both kinds.
Public queue/service distributions remain completed-only.

### Shared sample-quality contribution

All score formulas use integer arithmetic and floor division. The common
sample contribution is:

| Series length | Contribution |
| --- | ---: |
| `0..=7` | 0 |
| `8..=19` | 1 |
| `20..=39` | 3 |
| `40..=99` | 5 |
| `100+` | 8 |

## Candidate eligibility and scoring

Every formula is finally clamped to `0..=100`. A “soft cap” is applied before
that clamp unless the stated clean-extreme condition holds.

### Application queue saturation

The analyzer builds completed-only and observed-lower-bound p95 queue-share
candidates. A candidate is eligible when its p95 share is at least
`queueing.trigger_permille` (default 300). Let `Q` be that p95 share, `D` the
maximum retained `depth_at_start` among the candidate's queue events, `G=5`
when the selected in-flight episode has at least two samples and positive net
growth (otherwise 0), and `S` the shared contribution:

```text
score = 22 + floor(min(Q, 1000) / 14)
           + floor(min(D, 40) * 2 / 3) + G + S
```

The score is soft-capped at 95. The cap is removed when `Q >= 985`, `D >= 12`,
there are at least 20 share samples, and positive in-flight growth is known.
When both bases qualify, the higher score is selected; a tie keeps completed
evidence. Evidence states the p95 share, maximum sampled depth, positive growth,
and whether the selected value is a lower bound. Next checks target admission,
producer bursts, and a controlled parallelism comparison. Selecting the
lower-bound candidate caps confidence at Medium.

### Blocking-pool pressure

The evidence series is present `blocking_queue_depth` values. Let `P` be p95,
`K` peak, `N` nonzero samples, `T` total samples, `Z=floor(N*1000/T)`, and `S`
the shared contribution. The candidate is eligible if a percentile exists and
either `P > 0` or `N >= blocking.min_nonzero_samples_for_signal` (default 2).

```text
score = 32 + min(P, 24) + floor(min(K, 24) / 2)
           + floor(Z / 80) + S
```

The score is soft-capped at 94 unless `P >= 16`, `K >= 24`, and `Z >= 900`.
Evidence reports p95, peak, and `N/T`; next checks audit synchronous hot-path
work and `spawn_blocking` call sites. The configurable “strong blocking” test
requires all of `blocking.strong_p95_threshold`, `strong_peak_threshold`,
`strong_nonzero_share_permille`, and `strong_min_samples`. It does not alter
the blocking score; it controls correlation with blocking-looking downstream
stage names.

Runtime truncation or missing/partial key runtime fields can cap confidence.

### Executor pressure

Only snapshots containing `global_queue_depth` are relevant to worker evidence.
No such series means no executor candidate.

| Worker evidence classification | Exact condition | Scoring and confidence behavior |
| --- | --- | --- |
| worker count unavailable | every relevant snapshot lacks `worker_count` | absolute-depth fallback scoring; no worker-related cap |
| complete | every relevant snapshot has the same nonzero count | normalized scoring |
| complete, local lower bound | complete worker count but any relevant snapshot lacks `local_queue_depth` | normalized scoring; missing local contributes zero for that snapshot; Medium cap |
| partial | nonzero counts and missing counts are mixed | absolute-depth fallback scoring; Medium cap |
| inconsistent | more than one nonzero count occurs | absolute-depth fallback scoring; Medium cap |
| invalid zero | any relevant snapshot supplies zero | absolute-depth fallback scoring; Medium cap |

#### Worker-normalized mode

For every relevant snapshot:

```text
runnable_queue_depth = global_queue_depth + local_queue_depth_or_zero
queue_per_worker_milli = floor(runnable_queue_depth * 1000 / worker_count)
```

The implementation performs addition and multiplication in `u128`, divides
there, clamps the result to `u64::MAX`, then converts to `u64`. Global and local
depth are combined per snapshot **before** selecting p95. Missing local depth is
zero only for that snapshot and labels the series a lower bound.

Eligibility is normalized p95 `R` at least
`executor.min_runnable_queue_per_worker_p95_milli_for_signal` (default 500).

| Normalized p95 `R` | Queue contribution |
| --- | ---: |
| `0..=499` | 0 |
| `500..=999` | 5 |
| `1000..=1999` | 15 |
| `2000..=3999` | 25 |
| `4000..=7999` | 40 |
| `8000+` | 55 |

```text
score = 34 + normalized_queue_contribution(R) + G + S
```

Here `G=4` for known positive in-flight growth and otherwise 0; `S` uses the
number of normalized snapshots. There is no soft cap. `alive_tasks` and the
separate global/local p95 values can appear as descriptive evidence, but do not
add independent normalized contributions.

#### Absolute-depth fallback

Eligibility is global queue p95 `P` at least
`executor.min_global_queue_p95_for_signal` (default 1). Let `L` be p95 of all
present local depths (or zero), `A` p95 of present `alive_tasks` (or zero),
`G=4` for known positive in-flight growth, and `S` the sample-quality
contribution for the global series:

```text
score = 34 + floor(min(P, 150) / 4)
           + floor(min(L, 60) / 6)
           + floor(min(A, 400) / 40) + G + S
```

The score is soft-capped at 94 unless `P >= 140` and there are at least 30
global samples. Historical absence deliberately preserves this formula without
a worker-related cap. Partial, inconsistent, and invalid-zero worker evidence
uses the same formula without inventing a worker count, but caps confidence at
Medium. Evidence names the scoring mode and relevant limitation. Next checks
target long non-yielding polls, fanout, and stage isolation.

### Downstream-stage dominance

Each completed or observed-lower-bound stage summary is eligible when its
distinct-request count is at least `downstream.min_stage_samples` (default 3).
Let `T` be tail-request share permille, `C` cumulative share permille, and `S`
the shared contribution for distinct request samples:

```text
score = 24 + floor(min(T, 1000) / 11) + floor(C / 35) + S
```

The score is soft-capped at 95 unless `T >= 960`, `C >= 920`, and there are at
least 20 samples. Stage p95 is supporting evidence; `T`, `C`, and coverage drive
the score. Candidate selection is deterministic: score, then tail share, then
cumulative share, then completed evidence over lower-bound evidence, then stage
name ascending.

If the selected stage name case-insensitively contains a configured
`downstream.blocking_correlated_stage_patterns` entry and runtime blocking
evidence meets every configured strong threshold, its final score is limited to
at most `blocking_score - downstream.blocking_correlation_score_margin`
(saturating at zero). Evidence
states the correlation so blocking pressure stays prioritized. Otherwise next
checks target the named dependency, retries, and its SLO. Selecting a partial
stage path caps confidence at Medium.

## Confidence, ambiguity, and final ordering

The pipeline order is contractual:

1. compute each candidate's raw score;
2. assign initial Low/Medium/High confidence using
   `confidence.medium_score_threshold` and `confidence.high_score_threshold`;
3. find ambiguity membership from raw scores;
4. apply evidence-aware confidence caps;
5. sort the visible candidates.

An ambiguity cluster exists when the highest raw score is at least
`confidence.ambiguity_min_score` and at least two candidates also meet that
minimum and fall within `confidence.ambiguity_score_gap` of the highest raw
score. Cluster members are capped at Medium.

Caps compose conservatively (the lowest wins). Current cap families cover weak
overall evidence quality; zero completed requests (Low) or counts below
`evidence.low_completed_request_threshold` (Medium); dropped requests and the
candidate's queue, stage, or runtime family; missing queue/stage instrumentation;
missing runtime snapshots or partial key runtime fields; selected partial queue
or stage lower bounds; missing-local normalized executor evidence; ambiguous
worker counts; and raw-score ambiguity. Candidate-specific notes are emitted
when a bucket changed or when ambiguity, partial evidence, or an executor
limitation is material. Notes are stable and deduplicated.

Final ordering puts eligible diagnosis candidates before
`insufficient_evidence`, then orders by final confidence descending, raw score
descending, and this stable kind tie order:

1. `application_queue_saturation`
2. `blocking_pool_pressure`
3. `executor_pressure_suspected`
4. `downstream_stage_dominates`

Raw-score ambiguity intentionally uses different input from final visible
ordering; a lower raw score can appear first if it retains higher confidence.

## Warnings, confidence notes, and evidence quality

| Surface | Scope |
| --- | --- |
| `confidence_notes[]` | candidate-specific reasons that confidence was limited |
| `warnings[]` | additive global, route, or temporal interpretation cautions |
| `evidence_quality` | structured counts, per-family coverage, drops, overall quality, and limitations |

Warnings are additive and stably deduplicated. Global warnings can cover
permissive normalization, partial observations, truncation, low request count,
missing runtime distinction fields, close raw scores, route divergence, and
temporal movement. Route warnings include their deliberate runtime/in-flight
exclusion and low-volume omitted routes. Temporal warnings cover wall-clock
fallback, sparse filtered runtime evidence, and overlapping windows. Artifact
loader and capture-lifecycle notices—including unfinished-request notices—stay
on CLI stderr rather than becoming analyzer report warnings.

Coverage values are `missing`, `partial`, `truncated`, or `present` for requests,
queues, stages, runtime snapshots, and in-flight snapshots. Counts and drop
counters remain explicit. Overall quality is:

- **weak** for no/low completed requests, dropped requests, or no explanatory
  queue/stage/runtime family;
- **partial** for non-request truncation, no queue and stage evidence, partial
  runtime key coverage, or any partial queue/stage observation;
- **strong** otherwise.

Missing runtime snapshots add a limitation but alone do not lower otherwise
strong queue/stage evidence. `evidence_quality` describes retained evidence; it
does not certify correctness or causality.

## Global, route, and temporal analysis

The global report owns the primary full-run diagnosis. Route and temporal
sections are supporting context and never displace that ownership.

### Route breakdowns

Requests are grouped by route. A route needs `route.min_request_count` completed
requests, and at least two routes must qualify. Each slice retains requests,
queues, and stages belonging to those request IDs. Global runtime snapshots and
in-flight gauges are deliberately excluded because they cannot be attributed to
a route.

Breakdowns are emitted only when eligible routes have divergent primary kinds
(and `route.emit_on_divergent_suspects` is enabled), the slowest/fastest p95
meets the configured ratio, or slowest/global p95 meets its configured ratio.
Comparisons use cross-multiplication and `>=`. Output orders p95 descending,
request count descending, then route ascending; it truncates to
`route.breakdown_limit`. A route-scoped warning notes low-volume omitted routes.
The global divergence warning considers only emitted breakdowns.

### Temporal segments

Temporal analysis needs `temporal.min_request_count` completed requests and at
least `temporal.min_segment_request_count` in both halves. Requests sort by
run-relative start, Unix-ms start, then request ID when every start has
run-relative time; otherwise by Unix-ms start then request ID. The split is
`floor(n/2)`: that many requests are early and the remainder late.

Request, queue, and stage evidence comes from the permissively normalized
canonical Run. Runtime and in-flight evidence is filtered from the original
Run, preserving its timestamp provenance. Segment windows prefer complete
run-relative request start/finish bounds; missing precision uses Unix-ms bounds.
Snapshots use run-relative timestamps where possible and Unix-ms fallback
otherwise.

Segments emit only for enabled suspect-kind movement, a p95 ratio movement, or
queue/service p95 share movement. P95 ratios use configured numerator and
denominator with `>=`; shares use absolute difference at least
`temporal.share_shift_permille`. By default, a runtime-dependent kind shift is
suppressed when runtime or in-flight segment coverage is sparse and there is no
supporting p95, queue-share, or service-share movement. Large p95 and enabled
suspect shifts add global warnings. Unix fallback, sparse runtime-dependent
interpretation, and concurrent early/late window overlap add segment warnings;
overlap makes timestamp attribution approximate.

## Analyzer tuning and configuration transparency

Start with `AnalyzeOptions::default()`. The Rust builder exposes matching
`with_queueing`, `with_blocking`, `with_executor`, `with_downstream`,
`with_confidence`, `with_evidence`, `with_route`, and `with_temporal` groups.
TOML nests the same groups under `[analyzer.<group>]`, as shown in
[`examples/analyzer-config.toml`](../examples/analyzer-config.toml). The CLI
loads `--analyzer-config` first, then applies repeated `--analyzer-set
PATH=VALUE` overrides in argument order, so later repeated paths win.

This inventory is mechanically owned by the analyzer option registry and is
also printed by `tailtriage analyze --help-analyzer-options`:

| Option path | Default | Unit/type | Behavioral ownership |
| --- | --- | --- | --- |
| `queueing.trigger_permille` | 300 | permille | queue candidate trigger |
| `blocking.min_nonzero_samples_for_signal` | 2 | samples | zero-p95 blocking eligibility |
| `blocking.strong_p95_threshold` | 12 | depth | blocking-correlation strength |
| `blocking.strong_peak_threshold` | 20 | depth | blocking-correlation strength |
| `blocking.strong_nonzero_share_permille` | 700 | permille | blocking-correlation strength |
| `blocking.strong_min_samples` | 30 | samples | blocking-correlation strength |
| `executor.min_global_queue_p95_for_signal` | 1 | depth | legacy executor trigger |
| `executor.min_runnable_queue_per_worker_p95_milli_for_signal` | 500 | milli-tasks/worker | normalized executor trigger |
| `downstream.min_stage_samples` | 3 | distinct requests | stage eligibility |
| `downstream.blocking_correlated_stage_patterns` | `spawn_blocking, blocking_path, blocking` | string list | stage/blocking correlation |
| `downstream.blocking_correlation_score_margin` | 2 | score points | correlated-stage score limit |
| `confidence.medium_score_threshold` | 65 | score | initial Medium boundary |
| `confidence.high_score_threshold` | 85 | score | initial High boundary |
| `confidence.ambiguity_min_score` | 60 | score | ambiguity eligibility |
| `confidence.ambiguity_score_gap` | 4 | score points | ambiguity proximity |
| `evidence.low_completed_request_threshold` | 20 | requests | quality warnings and caps |
| `route.min_request_count` | 3 | requests/route | route eligibility |
| `route.breakdown_limit` | 10 | entries | route output limit |
| `route.emit_on_divergent_suspects` | true | bool | divergence emission/warning |
| `route.slowest_to_fastest_p95_ratio_numerator` | 3 | ratio numerator | route p95 movement |
| `route.slowest_to_fastest_p95_ratio_denominator` | 2 | ratio denominator | route p95 movement |
| `route.slowest_to_global_p95_ratio_numerator` | 5 | ratio numerator | route/global movement |
| `route.slowest_to_global_p95_ratio_denominator` | 4 | ratio denominator | route/global movement |
| `temporal.min_request_count` | 20 | requests | global temporal eligibility |
| `temporal.min_segment_request_count` | 8 | requests/segment | segment eligibility |
| `temporal.share_shift_permille` | 200 | permille | queue/service movement |
| `temporal.p95_shift_ratio_numerator` | 3 | ratio numerator | temporal p95 movement |
| `temporal.p95_shift_ratio_denominator` | 2 | ratio denominator | temporal p95 movement |
| `temporal.emit_on_suspect_shift` | true | bool | kind-shift emission/warning |
| `temporal.suppress_runtime_sparse_suspect_shift_without_supporting_movement` | true | bool | sparse shift suppression |

Semantic validation requires permille values to be at most 1000; confidence
thresholds, ambiguity values, and score margin to be at most 100; Medium not to
exceed High; nonempty, nonblank stage patterns; nonzero route limit; nonzero
ratio components with numerator at least denominator; nonzero temporal segment
minimum; and twice that segment minimum not to exceed the global temporal
minimum.

Tuning changes interpretation; it cannot repair missing or truncated evidence.
Default options omit `analyzer_config`. Non-default options include schema
version 1 and only non-default path/value summaries, sorted by path.

## Determinism and rendering guarantees

- Candidate selection, final suspect ordering, route ordering, and temporal
  early/late ordering are deterministic for the same normalized input/options.
- `Report` serialization has stable named fields; `route_breakdowns` and
  `temporal_segments` are always present, including when empty.
- `analyzer_config` is absent at defaults and present only for non-defaults.
- `render_json(&Report)` and `render_json_pretty(&Report)` are the canonical compact and
  pretty serializers of the typed `Report`; analysis and rendering are separate operations.
- `render_text(&Report)` renders the typed report. CLI JSON output serializes the
  same report model; text output uses the text renderer.

Representative prose inside text evidence is not a general byte-for-byte
contract beyond behavior explicitly protected by tests.

## Non-claims and known limitations

- Scores rank evidence only within one report; they are not cross-run severity.
- Confidence is ranking confidence under retained evidence, not causal certainty.
- Tuning cannot recover missing instrumentation or truncated events.
- Route and temporal sections are supporting context, not independent diagnoses.
- Unix-ms fallback and overlapping windows reduce temporal attribution precision.
- Partial queue and stage durations are observed lower bounds.
- Runtime field availability depends on sampler and Tokio build/runtime
  capabilities.
- Worker-normalized executor evidence is exact only to retained global/local
  queue inputs and worker counts; missing local depth makes it a lower bound.
- Even complete retained evidence supports next checks, not root-cause proof.
