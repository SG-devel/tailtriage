# tailtriage-axum

Axum adapter crate for `tailtriage` request-boundary triage wiring.

This crate isolates framework-specific middleware and extractor ergonomics so `tailtriage-tokio` can stay framework-agnostic.

## Use from this repo now

```bash
cargo run -p tailtriage-axum --example axum_service_adoption
cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json
```

## Post-publish crate add (when released)

```toml
[dependencies]
tailtriage-core = "0.1"
tailtriage-axum = "0.1"
```

## What this crate provides

- `middleware` to start and finish one tailtriage request per axum request
- `TailtriageRequest` extractor for request-scoped instrumentation handles
- `TailtriageExtractorError` rejection when middleware wiring is missing

## Minimal usage

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

Suspects in analysis output are leads, not proof of root cause.
