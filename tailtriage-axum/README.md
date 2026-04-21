# tailtriage-axum

`tailtriage-axum` provides **Axum ergonomics** for `tailtriage-core` request instrumentation.

It is a focused adapter crate: middleware starts/finishes request lifecycle, and an extractor gives handlers access to the request-scoped instrumentation handle.

## When to use this crate vs others

- Use `tailtriage-core` for framework-agnostic instrumentation.
- Add `tailtriage-axum` when you want Axum middleware/extractor wiring.
- Add `tailtriage-tokio` separately if you also need runtime-pressure snapshots.

## Installation

```bash
cargo add tailtriage-core tailtriage-axum
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

## Runtime and wiring notes

- Add `middleware` before using `TailtriageRequest` extractor.
- Missing middleware causes `TailtriageExtractorError` (HTTP 500).
- Route labeling prefers Axum `MatchedPath`; fallback is raw URI path.
- This crate is ergonomics-only and does not replace analysis from `tailtriage-cli`.
