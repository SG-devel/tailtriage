# Architecture

`tailtriage` is a file-based triage toolkit for Tokio services.

## Product-level shape

The default user path is:

1. instrument capture in service code (`tailtriage` default crate)
2. optionally enrich with runtime sampling (`tailtriage-tokio`)
3. finalize capture to a sink:
   - default local run artifact JSON via `LocalJsonSink`
   - optional finalized typed `Run` in memory via `MemorySink`
   - optional no persisted finalized run via `DiscardSink`
4. analyze with `tailtriage-analyzer` in process (typed `Report`, text rendering, Report JSON rendering) or with `tailtriage-cli` for file artifacts

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

### `tailtriage-analyzer`

Owns in-process analysis/report generation from completed runs:

- typed `Report` model
- `analyze_run` entry point
- `render_text` human-readable rendering
- analyzer-owned Report JSON rendering (`render_json`, `render_json_pretty`)

### `tailtriage-cli`

Consumes run artifacts from disk, validates schema/loader rules, invokes `tailtriage-analyzer`, and emits text/JSON output. CLI report rendering delegates to analyzer-owned renderers.

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


## Conceptual dependency flow

`tailtriage-core -> tailtriage-analyzer -> tailtriage-cli`

The CLI performs file-based analysis by loading artifacts, then consuming the analyzer crate for triage/report generation.
