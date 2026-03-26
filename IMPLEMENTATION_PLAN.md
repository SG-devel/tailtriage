# IMPLEMENTATION_PLAN.md

Post-MVP plan for `tailtriage`.

The MVP feature set is done. The next phase is not broad feature expansion. The next phase is to put the MVP in front of real users, collect feedback, see whether it survives contact with real usage, and expand only where evidence justifies it.

## Summary

Goal: validate that `tailtriage` is useful in the wild as a focused toolkit for **Tokio tail-latency triage**.

This plan prioritizes:

- real user feedback over speculative scope expansion
- keeping the project tightly aligned with its stated purpose
- selective improvements only where they remove real friction or create outsized value
- maintaining coherence as PRs and issues arrive
- keeping docs, demos, and tests current with the actual product

## Current status

The MVP is implemented.

That means the main question now is no longer “can we build the core concept?” The question is “does this hold up for real users solving real Tokio tail-latency triage problems?”

The purpose remains the same:

- `tailtriage` is for **Tokio tail-latency triage**
- it produces **evidence-ranked suspects** and **next checks**
- it is not a root-cause proof engine
- it is not a general observability platform
- it should not drift into a different category

---

## Phase 1 — real-world validation and feedback collection

### Goals

- get the MVP into real usage
- learn where users get stuck, where the output is useful, and where it fails
- determine whether the current scope survives real-world use

### Tasks

1. collect structured feedback from early users and reviewers, including reproducible examples, run artifacts, confusing outputs, and onboarding pain points where possible
2. note where onboarding, terminology, or output interpretation breaks down
3. track recurring confusion around suspects, evidence, confidence, and next checks
4. identify whether users can reach first value quickly from the current source/workspace path
5. separate “interesting ideas” from “real repeated pain in the wild”

### Deliverables

- a short list of validated user pain points
- a short list of things users clearly value in the MVP
- evidence on whether the current product framing lands with the intended audience

### Decision rule

Only treat something as a priority if it is one of the following:

- a repeated blocker to adoption or correct use
- a repeated source of misunderstanding about the project’s core purpose
- a clearly high-leverage improvement that substantially boosts usefulness or adoption
- a credible, reproducible severe correctness, reliability, or security issue that would materially undermine trust in the tool if left unresolved

---

## Phase 2 — tighten the MVP based on real constraints

### Goals

- improve the MVP where reality shows it is too rough
- stay within reasonable bounds
- avoid drift caused by one-off requests, speculative ideas, or adjacent-tool pressure

### Tasks

1. fix issues that materially block first-time success
2. fix issues that make output interpretation unreliable or unnecessarily confusing
3. improve ergonomics only where they preserve the core triage workflow
4. reject or defer changes that broaden the project beyond Tokio tail-latency triage
5. prefer small cohesive improvements over scattered capability growth

### Deliverables

- a tighter MVP that is easier to adopt and trust
- reduced friction in the capture → analyze → next check → re-run loop
- a clearly bounded backlog grouped around real needs

### Scope rule

Expand selectively, and only in two cases:

1. **Something is holding the MVP back**
   - onboarding friction
   - confusing output
   - missing documentation for actual usage
   - reliability gaps
   - poor demo coverage for important core cases

2. **Something offers a huge boost**
   - a feature or refinement that clearly increases adoption, clarity, or practical usefulness
   - a change that strengthens the core triage promise without changing the product category

If a proposal does not fit one of those two cases, it should usually not be in scope.

---

## Phase 3 — keep the project coherent as work arrives

### Goals

- make sure PRs and issues do not pull the project in scattered directions
- keep the repository coherent, readable, and intentional
- maintain a single clear product story

### Tasks

1. review incoming issues and PRs against the stated purpose of `tailtriage`
2. reject or narrow changes that create conceptual drift
3. group related work together instead of accepting isolated feature fragments
4. preserve consistent terminology across code, docs, demos, and review discussion
5. require new changes to fit the existing product shape or explicitly justify why they belong

### Deliverables

- a backlog that remains compact and legible
- fewer disconnected additions
- a repo that still feels like one project rather than accumulated requests

### Governance rule

For new PRs or issues:

- keep changes within reasonable bounds
- keep related work together cleanly
- do not accept “just one extra thing” additions that push the repo sideways
- prefer cohesion over breadth
- preserve the distinction between triage leads and proof of causality

### Maintainer triage rubric

When a new public issue or PR arrives, classify it into one of these buckets:

1. **In scope now**
   - directly improves Tokio tail-latency triage
   - removes real user friction
   - fixes correctness, reliability, or documentation gaps
   - fits the current product story without broadening category

2. **Needs evidence or reproduction**
   - plausible and relevant, but missing a reproducer, run artifact, clearer expected behavior, or enough detail to act confidently

3. **Defer**
   - potentially useful, but not currently justified by repeated pain, severity, or leverage
   - keep only if there is a clear future reason to revisit

4. **Out of scope**
   - pushes the repo into observability-platform territory
   - adds adjacent but non-core capabilities
   - creates a second product story or a competing onboarding path without strong justification

Maintainers should state the bucket clearly when closing, deferring, or narrowing an issue or PR.

---

## Phase 4 — docs, demos, and tests stay current

### Goals

- ensure the repository remains trustworthy
- keep supporting materials aligned with actual behavior
- avoid stale examples or claims

### Tasks

1. update docs whenever user-facing behavior or guidance changes
2. keep demos runnable, current, and aligned with the product promise
3. keep tests aligned with intended behavior and documented guarantees
4. ensure README, docs map, diagnostics docs, and demos tell the same story
5. remove stale wording that reflects old plans rather than current reality

### Deliverables

- up-to-date docs
- up-to-date demos
- up-to-date tests
- a repo where examples and validation still support the public claims

### Maintenance rule

No meaningful product change is complete unless:

- docs reflect it
- demos still make sense with it
- tests cover the intended behavior

---

## Phase 5 — strengthen positioning if real feedback is positive

### Goals

- become better known in the relevant community if the MVP proves useful
- improve discoverability without changing the project’s direction
- make it easy for the right users to understand when `tailtriage` is for them

### Tasks

1. sharpen positioning for the intended Rust/Tokio audience
2. improve public-facing explanation of what `tailtriage` is and is not
3. highlight strong demos and credible usage paths
4. make comparisons with adjacent tools modest and grounded
5. invest more in visibility only if real user feedback is positive

### Deliverables

- stronger community awareness
- clearer category understanding
- better discoverability among people who actually have the problem `tailtriage` solves

### Visibility rule

Community visibility work should follow evidence of usefulness. It should not become a substitute for product validation.

If feedback is positive, it is reasonable to invest more in:

- clearer public documentation
- better demo presentation
- community-facing explanations and examples
- discoverability among Rust/Tokio practitioners

But this should still reinforce the same purpose, not redefine it.

---

## Explicit anti-goals for this phase

Do not use this phase to:

- turn `tailtriage` into a general observability platform
- drift into a broader telemetry product
- overreact to isolated requests that do not fit the core purpose
- add scattered features without a coherent product reason
- confuse evidence-ranked suspects with root-cause proof
- let docs, demos, and tests lag behind reality
- optimize for breadth at the expense of clarity and cohesion

---

## Success criteria

This plan succeeds when:

1. the MVP is tested by real users in realistic conditions
2. feedback shows whether `tailtriage` is genuinely useful in the wild
3. changes after MVP are selective, evidence-driven, and tightly scoped
4. incoming PRs and issues do not cause product drift
5. the repository remains coherent and aligned with Tokio tail-latency triage
6. docs, demos, and tests stay current with the actual product
7. if feedback is positive, the project becomes better known in the right community without changing direction
