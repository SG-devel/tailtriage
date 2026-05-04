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

### 1) Validation truth and presentation

Make validation easy to understand, truthful, and reproducible.

Near-term tasks:

- remove PR-history wording from public validation docs
- keep deterministic, repeated-run, mitigation, runtime-cost, and collector-limit validation clearly separated
- make CI/manual/release status explicit for each validation path
- add generated metrics to scorecards where available
- keep validation non-claims visible
- ensure docs match the current manifest and scripts

### 2) Diagnostic corpus tightening

Improve deterministic validation without turning fixtures into marketing claims.

Near-term tasks:

- keep adversarial sparse/missing/truncated/mixed cases small and reviewable
- require next-check substrings where next-check behavior is part of the contract
- preserve confidence ceilings for weak or ambiguous evidence
- avoid labeling analyzer output as causal proof
- keep `ground_truth` defined as fixture intent, not production truth

### 3) Real-world validation

Gather reproducible artifacts and concrete user pain points.

Near-term tasks:

- collect anonymized real-service artifacts when available
- compare user interpretation against intended report semantics
- identify repeated adoption friction
- prioritize changes backed by observed confusion or real triage failures

### 4) Product tightening

Improve ergonomics where they reduce real triage friction.

Near-term tasks:

- improve diagnosis wording and next-check clarity when users misread results
- keep lifecycle, truncation, and confidence behavior explicit
- remove stale examples or docs that teach superseded paths
- avoid adding new product categories

### 5) Documentation and teaching surface alignment

Keep public docs synchronized.

Near-term tasks:

- keep `README.md`, `docs/README.md`, crate READMEs, demos, and examples aligned
- keep validation docs present-tense and stable, not PR-history oriented
- update docs contract tests when public docs structure intentionally changes

## Near-term sequencing

Before the next public promotion or release:

1. update repository guidance for validation docs and scorecards
2. update the product spec with the validation contract
3. update this implementation plan with validation-presentation work
4. clean `VALIDATION.md` public wording
5. fix stale diagnostic-validation wording
6. add generated metric summaries to the deterministic scorecard
7. clarify which validation profiles run in CI
8. run docs contract validation and relevant validation scripts

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
