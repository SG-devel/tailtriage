# Architecture

`tailtriage` is a file-based triage toolkit for Tokio services.

## Product-level shape

The default user path is:

1. instrument capture in service code (`tailtriage` default crate)
2. optionally enrich with runtime sampling (`tailtriage-tokio`)
3. write local run artifact JSON
4. analyze artifact with `tailtriage-cli`

The result is a triage report with evidence-ranked suspects and next checks.

## Crate roles

### `tailtriage` (default crate, default entry point)

Provides one all-in-one entry point for:

- direct capture (`tailtriage::Tailtriage`)
- controller windows (`tailtriage::controller::TailtriageController`)
- optional runtime sampler module (`tailtriage::tokio`)
- optional Axum adapter module (`tailtriage::axum`)

### `tailtriage-controller`

Controls repeated bounded capture windows in long-lived services.

- arm/disarm generation windows
- isolate generations from each other
- support TOML-backed template config and future-generation reload

### `tailtriage-core`

Owns the core capture model:

- request lifecycle API (`StartedRequest`, `RequestHandle`, `RequestCompletion`)
- queue/stage/inflight instrumentation wrappers
- run artifact schema and sink behavior
- capture limits/truncation accounting

### `tailtriage-tokio`

Adds optional runtime-pressure snapshots to the same run artifact via `RuntimeSampler`.

### `tailtriage-axum`

Adds optional Axum request-boundary ergonomics (middleware + extractor).

### `tailtriage-cli`

Consumes run artifacts and emits diagnosis reports (text/JSON).

## Relationship model

- **Capture surfaces:** direct `Tailtriage` lifecycle and controller-managed windows feed the same artifact model.
- **Controller windows:** long-lived services can collect repeated bounded runs without restart.
- **Optional runtime enrichment:** runtime sampler increases evidence quality when runtime pressure is ambiguous.
- **Optional framework ergonomics:** Axum adapter reduces boundary wiring while keeping explicit instrumentation in business logic.
- **Artifact analysis:** CLI performs file-based diagnosis from captured evidence.

## Boundary and claims

`tailtriage` intentionally focuses on triage from one run artifact.

It does not claim:

- observability backend behavior
- distributed-system root-cause proof
- automatic causality certainty
