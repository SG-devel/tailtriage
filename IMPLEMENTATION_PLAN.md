# IMPLEMENTATION_PLAN.md

Post-MVP operating plan for `tailtriage`.

`tailtriage` already exists as a focused toolkit for **Tokio tail-latency triage**. This plan keeps the project useful, clear, and tightly scoped while we validate it in real usage.

## Current operating mode

The product is in a validation-and-tightening phase, not a broad expansion phase.

Current priority order:

1. validate real user usefulness
2. remove adoption and interpretation friction
3. keep docs/examples/demos/tests aligned with current behavior
4. reject scope drift

The core product promise remains:

- capture one run (or bounded controller windows)
- analyze into evidence-ranked suspects and next checks
- iterate with targeted re-runs

Suspects are leads, not proof of root cause.

## What we optimize now

We optimize for:

- first-time time-to-value for Tokio users
- report clarity and actionability
- reliability and correctness under real workloads
- documentation quality and consistency
- coherent APIs and crate boundaries

We do not optimize for speculative breadth.

## Evidence-driven change policy

A change is in scope when at least one is true:

1. it removes repeated real-user friction
2. it fixes correctness/reliability/security risk
3. it materially improves usefulness without changing product category

A change is out of scope when it mainly adds adjacent platform capabilities or creates a competing onboarding story.

## Active workstreams

### 1) Real-world validation

- Gather reproducible artifacts and concrete user pain points.
- Prioritize repeated problems over one-off requests.
- Confirm users can complete capture -> analyze -> next check -> re-run with current surfaces.

### 2) Product tightening

- Improve ergonomics where they reduce real triage friction.
- Improve diagnosis wording and next-check clarity when users misread results.
- Keep behavior explicit around lifecycle, truncation, and confidence limits.

### 3) Documentation and teaching surface alignment

- Keep `README.md`, `docs/`, and crate READMEs synchronized.
- Keep docs user-facing, present-tense, and truthful to current behavior.
- Keep examples/demos aligned with the same default usage story.
- Update docs contract tests when docs structure or required index links change.

### 4) Repository coherence

- Keep one coherent product story across crates and docs.
- Reject additions that push the project toward a general observability platform.
- Prefer smaller cohesive changes over scattered feature growth.

## Quality gates for changes

For scoped work, completion requires:

1. code and tests updated as needed
2. docs/examples/demos updated when user-facing behavior or guidance changes
3. formatting/lint/tests passing per repository requirements
4. no quiet scope expansion

## Explicit anti-goals in this phase

Do not use this phase to turn `tailtriage` into:

- a telemetry backend
- a distributed tracing platform
- a broad observability suite
- an automated root-cause proof engine

## Success criteria

This plan succeeds if:

1. users can repeatedly get useful triage output from real runs
2. post-MVP changes are evidence-driven and tightly scoped
3. docs/examples/demos/tests stay current with the real product
4. the project remains clearly positioned as Tokio tail-latency triage
