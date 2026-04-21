# tailtriage-axum

`tailtriage-axum` provides Axum-first ergonomics for request-boundary instrumentation with tailtriage.

It is an adapter crate: middleware starts/finishes request lifecycle and an extractor exposes the request-scoped handle in handlers.

## What this crate is for

Use this crate when you want request-boundary integration in Axum without manually wiring lifecycle calls in every handler.

## When to use this crate vs others

- **Use `tailtriage-axum`:** Axum middleware/extractor ergonomics.
- **Use `tailtriage-core` directly:** framework-agnostic manual instrumentation.
- **Add `tailtriage-tokio`:** if you also need runtime-pressure snapshots.
- **Use `tailtriage` (default crate):** default starting point with optional `axum` feature.

## Installation

Direct crates:

```bash
cargo add tailtriage-core tailtriage-axum
```

Via the default crate:

```bash
cargo add tailtriage --features axum
```

## Minimal example

```rust,no_run
use std::sync::Arc;

use axum::{extract::State, middleware::from_fn_with_state, routing::get, Router};
use tailtriage_axum::{middleware, TailtriageRequest};
use tailtriage_core::Tailtriage;

# async fn app(tailtriage: Arc<Tailtriage>) {
async fn checkout(TailtriageRequest(req): TailtriageRequest, State(_): State<()>) {
    let _: Result<(), ()> = req.stage("inventory_lookup").await_on(async { Ok(()) }).await;
}

let app: Router<()> = Router::new()
    .route("/checkout", get(checkout))
    .layer(from_fn_with_state(tailtriage, middleware))
    .with_state(());
# let _ = app;
# }
```

## Request-boundary constraints

- Install `middleware` before using `TailtriageRequest` extractor.
- Missing middleware yields `TailtriageExtractorError` (HTTP 500).
- Route labels prefer Axum `MatchedPath`; fallback is raw URI path.
- This crate handles integration ergonomics only; report generation remains in `tailtriage-cli`.

## Deeper docs

- Default crate integration path: [`../tailtriage/README.md`](../tailtriage/README.md)
- Core lifecycle semantics: [`../tailtriage-core/README.md`](../tailtriage-core/README.md)
- CLI analyzer/report docs: [`../tailtriage-cli/README.md`](../tailtriage-cli/README.md)
