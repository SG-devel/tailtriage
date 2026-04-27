# Tailtriage Design Notes

These notes explain the main design decisions behind `tailtriage`: what problem the project tries to solve, what tradeoffs shaped the implementation, which alternatives were rejected, and which decisions remain open to revision.

The goal is not to prove that every decision was optimal. The goal is to make the reasoning reviewable.

This document is not a validation report; validation results belong in `VALIDATION.md`.

`tailtriage` is intentionally conservative. It tries to turn one captured Tokio latency run into a bounded diagnostic report: likely bottleneck family, supporting evidence, confidence, warnings, and next checks. The main tradeoff is deliberate: the project gives up broad automatic root-cause claims in exchange for explainable, testable, and reviewable triage behavior.

The design should be judged by whether it:

1. identifies known injected bottleneck families under controlled validation,
2. lowers confidence when evidence is weak,
3. surfaces missing or truncated data,
4. produces useful next checks,
5. keeps runtime and collector overhead measurable,
6. remains honest about what it cannot prove.

## Decision record summary

| Decision           | Chosen approach                              | Main reason                                     | Main cost                            |
| ------------------ | -------------------------------------------- | ----------------------------------------------- | ------------------------------------ |
| Project scope      | Triage tool, not observability backend       | Keep the project narrow and useful              | No dashboards or long-term telemetry |
| Diagnostic output  | Suspects, evidence, confidence, next checks  | Avoid false root-cause certainty                | Less dramatic output                 |
| Taxonomy           | Four bottleneck families                     | Distinguishable and actionable                  | Coarse for some incidents            |
| Instrumentation    | Explicit semantic signals                    | Better evidence quality                         | More adoption friction               |
| Capture model      | Artifact-based analysis                      | Reproducible and testable                       | Extra workflow step                  |
| Workspace          | Multiple crates                              | Dependency and responsibility separation        | More complexity                      |
| CLI                | First-class analyzer                         | Useful for validation and offline investigation | Extra public interface               |
| Collector behavior | Bounded retention with truncation summaries  | Avoid unbounded memory risk                     | Partial data under stress            |
| Overhead claims    | Machine/workload scoped                      | Credible measurement                            | Less marketable                      |
| Validation         | Controlled demos first                       | Known injected causes                           | Synthetic limits                     |
| AI usage           | AI-assisted implementation under specs/tests | Productivity without abandoning ownership       | Requires clear ownership and review  |

---

## 1. Project framing

`tailtriage` is a Rust/Tokio tail-latency triage toolkit. It is designed to help answer a narrow diagnostic question:

> Given one captured run of a Tokio service, is the tail-latency problem more consistent with application-level queueing, executor pressure, blocking-pool pressure, or a slow downstream stage?

The output is intentionally framed as triage guidance:

* ranked suspects,
* evidence,
* confidence,
* warnings,
* and suggested next checks.

It is not intended to be root-cause proof.

---

## 2. Core design principle

The central design principle is:

> Prefer bounded, explainable diagnostic claims over broad automatic diagnosis.

This affects almost every part of the project.

A tool that says “your system is slow because of X” is dangerous unless it has complete evidence. `tailtriage` instead tries to say:

> “Given the captured signals, X is the strongest current suspect; here is the evidence; here is what to check next.”

This keeps the tool useful without pretending that one run can prove causality.

---

## 3. Why focus on tail latency?

Average latency often hides the incidents that matter most operationally. A service can have acceptable median latency while p95 or p99 requests are affected by queueing, scheduler pressure, blocking work, or a slow dependency.

The project focuses on tail behavior because that is where diagnosis is often hardest:

* multiple requests overlap,
* queues hide the original source of delay,
* downstream calls can dominate only some requests,
* executor pressure can look like slow application logic,
* blocking work can interfere with otherwise async systems,
* generic tracing output can show many spans without making the next debugging step obvious.

The design therefore prioritizes p95/p99-oriented summaries and evidence-ranked suspects rather than aggregate averages alone.

---

## 4. Why a triage tool instead of another observability backend?

### Decision

`tailtriage` is not an observability backend.

### Rationale

A full observability backend would require decisions about ingestion, retention, indexing, dashboards, query languages, deployment, authentication, alerting, and storage cost. That would move the project away from its core value: helping a Rust/Tokio developer interpret one captured latency incident.

The chosen scope is smaller:

1. capture relevant signals,
2. produce an artifact,
3. analyze the artifact,
4. report likely bottleneck families,
5. suggest next checks.

### Tradeoff

This keeps the tool small and reviewable, but means it depends on other tools for continuous production monitoring, distributed tracing, long-term metrics, dashboards, and alerting.

### Rejected alternative

Build a complete telemetry platform.

### Reason rejected

Too broad, too expensive to validate, and not necessary for the specific diagnostic gap the project targets.

---

## 5. Why four bottleneck families?

### Decision

The initial diagnostic taxonomy is limited to four main families:

1. application-level queueing,
2. Tokio executor pressure,
3. blocking-pool pressure,
4. downstream stage latency.

### Rationale

These categories are common enough to be useful, distinct enough to guide different next checks, and narrow enough to reason about from captured request/runtime signals.

The project deliberately avoids a large taxonomy such as:

* database root cause,
* network root cause,
* CPU root cause,
* lock contention root cause,
* allocator root cause,
* kernel scheduling root cause,
* garbage collector root cause,
* cache behavior root cause.

Many of those are either outside Rust/Tokio, require lower-level instrumentation, or cannot be inferred safely from the available signals.

### Tradeoff

A small taxonomy means some real incidents are classified coarsely. For example, database pool wait may initially appear as a queueing-like symptom rather than a database-specific diagnosis.

### Why this is acceptable

The tool is designed to recommend the next check, not produce a final root cause. A coarse but accurate bottleneck family can still guide a useful next action.

### Future possibility

The taxonomy can be extended later if validation shows that a new category is both common and distinguishable from existing signals.

---

## 6. Why suspects instead of conclusions?

### Decision

Reports use suspects, evidence, confidence, and next checks instead of definitive conclusions.

### Rationale

Tail-latency incidents often have incomplete evidence. A captured run may be missing queue spans, stage spans, runtime snapshots, or downstream timing. Even when instrumentation is present, multiple causes can overlap.

The report model therefore uses:

* `primary_suspect`,
* `secondary_suspects`,
* `confidence`,
* `evidence`,
* `warnings`,
* `next_checks`.

### Tradeoff

This makes the output more cautious and possibly less impressive than a tool that claims automatic root-cause detection.

### Why this is acceptable

A cautious report is more useful than a confident but wrong one. In production debugging, a high-confidence false diagnosis can waste more time than a low-confidence but honest hint.

---

## 7. Why require explicit instrumentation?

### Decision

`tailtriage` uses explicit request, queue, stage, and runtime instrumentation rather than relying entirely on automatic inference.

### Rationale

Tail-latency diagnosis requires knowing where time was spent. Without explicit boundaries, the tool would have to guess whether a request waited in a queue, spent time in a downstream call, or was delayed by executor pressure.

Explicit instrumentation gives the analyzer stronger evidence:

* request lifecycle timing,
* queue wait timing,
* service/stage timing,
* in-flight trends,
* optional runtime snapshots.

### Tradeoff

Manual instrumentation increases adoption friction.

### Why this is acceptable

The target user is someone actively investigating a difficult latency issue. For that user, adding focused instrumentation may be acceptable if it produces a clearer next debugging step.

### Mitigation

Adapters such as Axum integration should reduce boundary instrumentation cost. Examples and demos should show the shortest path to a useful capture.

---

## 8. Why not infer everything from tracing alone?

### Decision

`tailtriage` does not try to be a generic tracing analyzer.

### Rationale

Tracing spans are useful, but generic spans may not encode the semantic distinctions the analyzer needs. For example, a span may represent a downstream call, a queue wait, business logic, or internal framework work. Without conventions, the analyzer cannot safely interpret every span.

`tailtriage` favors a smaller set of domain-specific signals whose meaning is explicit.

### Tradeoff

This sacrifices plug-and-play compatibility with arbitrary tracing output.

### Why this is acceptable

The project values diagnostic precision over broad ingestion.

### Future possibility

Tracing import could be added later if there is a clear mapping from span metadata to `tailtriage` concepts.

---

## 9. Why separate capture and analysis?

### Decision

The design separates runtime capture from offline analysis.

### Rationale

Separating capture and analysis has several benefits:

* captured artifacts can be reviewed,
* analysis can be repeated as rules improve,
* CLI output can be tested independently,
* production services do not need to run heavy analysis inline,
* artifacts can be shared in bug reports or validation runs.

### Tradeoff

The workflow has more steps than an always-on live dashboard.

### Why this is acceptable

The primary workflow is investigation, not continuous monitoring.

---

## 10. Why a multi-crate workspace?

### Decision

The project is split into multiple crates rather than one monolithic crate.

### Rationale

The responsibilities are different enough to justify separation:

* core data model and capture logic,
* Tokio runtime integration,
* Axum integration,
* controller/reporting behavior,
* CLI analysis,
* top-level facade.

This separation keeps optional integrations from forcing unnecessary dependencies on users who only need the core model.

### Tradeoff

Multiple crates increase workspace complexity and can look over-engineered early in the project.

### Why this is acceptable

The dependency boundaries matter for Rust users. Someone adding lightweight core instrumentation should not necessarily pull in Axum or CLI dependencies.

### Risk

If the project remains small, the crate split may be more structure than necessary. This should be revisited if the boundaries create maintenance friction.

---

## 11. Why have a CLI?

### Decision

`tailtriage` includes a CLI for analyzing captured artifacts.

### Rationale

A CLI makes the tool usable outside application code:

* run analysis in CI,
* inspect artifacts after a load test,
* compare before/after mitigation runs,
* produce machine-readable JSON,
* support demos and validation.

### Tradeoff

The CLI creates another public interface to maintain.

### Why this is acceptable

The CLI is central to the capture → analyze → next-check workflow. It also makes validation easier because analyzer behavior can be tested from stable artifacts.

---

## 12. Why include confidence levels?

### Decision

Reports include confidence rather than only scores.

### Rationale

Numeric scores alone can imply false precision. Confidence levels provide a more human-readable indication of how much the tool trusts the diagnosis.

Confidence should be affected by:

* amount of data,
* missing instrumentation,
* truncation,
* signal strength,
* mixed causes,
* unavailable runtime fields,
* conflicting evidence.

### Tradeoff

Confidence calibration is subjective and must be validated carefully.

### Why this is acceptable

A rough but conservative confidence signal is better than an unqualified ranked list.

### Design rule

High confidence should be rare and should require strong, non-conflicting evidence.

---

## 13. Why surface warnings prominently?

### Decision

Reports include warnings for partial data, truncation, missing fields, or other limitations.

### Rationale

Diagnostic output can be actively harmful if it hides evidence quality problems. If an artifact is truncated or missing relevant signals, the report must say so.

### Tradeoff

Warnings make output noisier.

### Why this is acceptable

Warnings are part of the trust model. A report that explains its limitations is more useful than a clean but misleading result.

---

## 14. Why bounded collectors and truncation summaries?

### Decision

Collectors should use bounded retention and expose truncation/drop summaries.

### Rationale

An investigation tool should not create unbounded memory risk in the service being investigated. Bounded collection reduces operational risk.

### Tradeoff

Bounded collection can drop data under heavy load, which weakens diagnosis.

### Why this is acceptable

Dropped data is acceptable only if it is visible. Truncation must appear in the artifact and should reduce confidence where appropriate.

---

## 15. Why not optimize for zero overhead first?

### Decision

The project prioritizes bounded, measurable overhead rather than claiming zero overhead.

### Rationale

Any useful instrumentation has cost. The more honest engineering question is:

> What does this mode cost under this workload, and is that acceptable for an investigation window?

### Tradeoff

This makes the project less attractive than a tool claiming negligible overhead.

### Why this is acceptable

Measurable, documented overhead is more credible than an unsupported low-overhead claim.

---

## 16. Why machine-scoped overhead claims?

### Decision

Runtime-cost measurements should be machine-scoped and workload-scoped.

### Rationale

Overhead depends on CPU, runtime configuration, request rate, instrumentation density, sampler frequency, allocator behavior, and workload shape.

A universal overhead claim would be misleading.

### Tradeoff

Machine-scoped claims are less marketable.

### Why this is acceptable

The project values credible claims over broad claims.

---

## 17. Why demos before real production validation?

### Decision

The initial validation uses controlled demos before real service case studies.

### Rationale

Controlled demos allow known injected causes. This is necessary to test whether the analyzer can identify the correct bottleneck family.

Real production incidents are valuable, but they often lack ground truth. They are better for case studies than first-principles validation.

### Tradeoff

Synthetic demos may not capture all real-world behavior.

### Why this is acceptable

Synthetic validation and real-world validation answer different questions. The project should eventually have both.

---

## 18. Why include next checks?

### Decision

Reports include suggested next checks.

### Rationale

The practical value of triage is not just naming a suspect. It is reducing the next debugging step.

For example:

* queueing suspect → inspect admission rate, worker count, burst behavior, queue depth,
* downstream suspect → inspect dependency latency, retries, timeout behavior, downstream concurrency,
* blocking-pool suspect → inspect `spawn_blocking` usage and blocking pool saturation,
* executor-pressure suspect → inspect runnable tasks, long polls, CPU-heavy async work, yielding behavior.

### Tradeoff

Next checks can be incomplete or too generic.

### Why this is acceptable

They are recommendations, not prescriptions. They should be reviewed and improved as validation grows.

---

## 19. Why not claim compatibility with every async framework?

### Decision

The initial integrations focus on Tokio and Axum.

### Rationale

Tokio is the runtime context where the diagnostic categories are defined. Axum is a practical first web-framework integration.

### Tradeoff

This narrows the initial audience.

### Why this is acceptable

A narrow, validated integration is better than many shallow integrations.

### Future possibility

Additional adapters can be added if they preserve the same semantic signal quality.

---

## 20. Why validate abstention behavior?

### Decision

The tool should be validated not only for correct classification, but also for correct refusal to overclaim.

### Rationale

A triage tool must know when evidence is insufficient. The most dangerous failure mode is not “low-confidence miss.” The dangerous failure mode is “high-confidence wrong diagnosis.”

Validation should therefore include:

* insufficient request count,
* missing spans,
* missing queue data,
* missing runtime fields,
* truncated artifacts,
* mixed bottlenecks,
* conflicting evidence.

### Tradeoff

Abstention can make the tool feel less powerful.

### Why this is acceptable

Honest uncertainty is a feature in diagnostic systems.

---

## 21. Decisions that should remain revisitable

The following decisions should not be treated as permanent:

1. **The four-family taxonomy**

   * Revisit if validation shows recurring distinguishable causes that deserve first-class categories.

2. **Crate boundaries**

   * Revisit if the multi-crate workspace creates more friction than benefit.

3. **Confidence calibration**

   * Revisit as repeated-run validation produces evidence about false positives and ambiguity.

4. **CLI report schema**

   * Keep stable where possible, but allow versioned evolution.

5. **Tracing integration**

   * Revisit if a reliable mapping from tracing spans to semantic tailtriage signals emerges.

6. **Runtime sampler defaults**

   * Revisit after overhead and collector-limit validation.

7. **Adapter scope**

   * Add adapters only where they preserve semantic quality and are validated.

---

## 22. Known design risks

### Risk: The tool may look over-structured for its adoption level

The project has multiple crates, docs, examples, and a careful diagnostic model. Without validation artifacts, this can look overbuilt.

Mitigation:

* publish `VALIDATION.md`,
* include raw artifacts,
* show before/after mitigation runs,
* clearly separate validated, partially validated, and future claims.

### Risk: Manual instrumentation may discourage adoption

Developers may not want to add queue and stage annotations.

Mitigation:

* make the first useful integration path short,
* provide Axum examples,
* show partial-instrumentation behavior,
* document what signal quality is lost when instrumentation is sparse.

### Risk: Diagnostic categories may be too coarse

Some incidents may be classified as queueing or downstream dominance when the real issue is more specific.

Mitigation:

* frame output as bottleneck-family triage,
* include next checks,
* add categories only after validation shows they are distinguishable.

### Risk: Confidence may be miscalibrated

Confidence labels can mislead if they are too optimistic.

Mitigation:

* validate confidence separately from suspect ranking,
* cap confidence under missing/truncated data,
* track high-confidence wrong results as a primary failure metric.

### Risk: Synthetic demos may not generalize

Controlled workloads are easier than production systems.

Mitigation:

* label synthetic results as synthetic,
* add real-world case studies later,
* avoid universal production claims.

---

## 23. How AI assistance fits into the design process

AI assistance was used during implementation, but not as a substitute for design ownership.

The project should be evaluated as spec-driven because the important engineering work is expressed in:

* the diagnostic scope,
* the constrained taxonomy,
* bounded claims,
* lifecycle semantics,
* documented non-goals,
* using tests to constrain behavior,
* review of generated code,
* controlled demos and validation scenarios,
* visible uncertainty in reports.

The key process claim is:

> AI assistance was used as an implementation accelerator, but the project’s behavior is governed by explicit specifications, tests, validation scenarios, and documented limitations.

The intended standard is that `tailtriage` should be understandable, reviewable, tested, and maintainable regardless of which tools were used during implementation.

---

