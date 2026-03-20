# IMPLEMENTATION_PLAN.md

Release/polish plan for shipping the `tailtriage` MVP clearly.

The MVP feature set is implemented. Remaining work focuses on positioning, onboarding, demo storytelling, and publishability without expanding scope.

## Summary

Goal: make a new reader understand in under 30 seconds that this project is **Tokio tail-latency triage** for ordinary developers.

This plan prioritizes:
- consistent product/category language
- shortest path to first value
- modest, evidence-based comparisons with adjacent tools
- narrow, honest scope and uncertainty statements

## Phase 1 — positioning and terminology alignment

### Goals
- align product language across all top-level docs
- preserve distinction between evidence-ranked suspects and causal proof

### Tasks
1. ensure README/SPEC/docs map use triage-first wording
2. keep “diagnosis” mainly for analyzer/report actions
3. standardize “evidence-ranked suspect” and “next checks” language
4. remove observability-platform phrasing that implies broader scope

### Deliverables
- consistent, non-contradictory product positioning
- explicit non-goals preserved across docs

---

## Phase 2 — onboarding and quickstart ergonomics

### Goals
- reduce friction for first-time users
- tighten quickstart around shortest path to value

### Tasks
1. trim quickstart to one request path + one analyze command
2. ensure canonical workflow is capture -> analyze -> next check -> re-run
3. keep examples short and directly tied to suspect ranking
4. fix one or two high-impact adoption-friction issues only (no feature expansion)

### Deliverables
- faster time-to-first-triage for new users
- concise examples optimized for non-experts

---

## Phase 3 — ecosystem framing and comparisons

### Goals
- clarify fit alongside existing Tokio tooling
- keep comparisons grounded and modest

### Tasks
1. document “why not just tokio-console / tokio-metrics” in product docs
2. distinguish live debugging, raw metrics, and triage interpretation use-cases
3. avoid disparaging adjacent tools; emphasize complementarity

### Deliverables
- clear product boundary in README/docs
- improved newcomer understanding of when to use `tailtriage`

---

## Phase 4 — demo storytelling polish

### Goals
- make demo outputs clearly support the product promise
- preserve reproducible before/after triage loops

### Tasks
1. tighten demo docs around suspect ranking and evidence
2. keep fixtures deterministic and easy to re-run
3. ensure each demo links to practical next checks
4. add at most one adoption-oriented example if it improves clarity

### Deliverables
- demos that teach triage workflow, not platform ambitions
- reproducible storytelling artifacts for reviewers/users

---

## Phase 5 — release packaging and discoverability

### Goals
- improve publishability and discoverability without adding major features

### Tasks
1. confirm crate metadata/docs links are consistent
2. align repository/doc entry points for first-time readers
3. ensure changelog and docs map support release readability
4. verify rustdoc/README language coherence for crate consumers

### Deliverables
- release-ready docs surface
- coherent external-facing project story

---

## Explicit anti-goals for this plan

Do not use this phase to:
- add major diagnosis categories
- build exporters/backends
- build a live UI
- add distributed-system features
- shift into a general observability platform

## Success criteria

This release plan succeeds when:

1. product category is obvious quickly: Tokio tail-latency triage
2. target user is explicit: ordinary Rust/Tokio developers
3. docs clearly explain complementarity with tokio-console/tokio-metrics style tools
4. wording consistently states suspects are evidence-ranked leads, not proof
5. no scope expansion beyond the current MVP
