# tailtriage-axum

`tailtriage-axum` provides Axum request-boundary wiring for `tailtriage`.

Use it when you want middleware to start and finish request lifecycle automatically at the Axum boundary, while keeping queue/stage/inflight instrumentation explicit inside handlers or helper code.

## What this crate does

This crate provides:

- `middleware` for default request start/finish
- `middleware_with_status_classifier(...)` for custom HTTP status -> outcome mapping
- `TailtriageRequest` extractor for request-scoped handles in handlers

It improves integration ergonomics; it does not replace explicit instrumentation in request logic.

## Crate selection

Choose `tailtriage-axum` when you use Axum and want framework-boundary start/finish wiring.

Choose `tailtriage-core` for framework-agnostic manual instrumentation.

Choose `tailtriage` when you want the default entry point with feature-gated Axum support.

## Installation

Direct crates:

```bash
cargo add tailtriage-core tailtriage-axum
```

Via the default crate:

```bash
cargo add tailtriage --features axum
```

## Quick start

```rust,no_run
use std::sync::Arc;

use axum::{middleware::from_fn_with_state, routing::get, Router};
use tailtriage_axum::{middleware, TailtriageRequest};
use tailtriage_core::Tailtriage;

async fn checkout(TailtriageRequest(req): TailtriageRequest) {
    let _: Result<(), ()> = req
        .stage("inventory_lookup")
        .await_on(async { Ok(()) })
        .await;
}

fn app(tailtriage: Arc<Tailtriage>) -> Router {
    Router::new()
        .route("/checkout", get(checkout))
        .layer(from_fn_with_state(tailtriage, middleware))
}
```

## Automatic vs explicit responsibilities

Automatic at the Axum boundary:

- request start and finish
- request-scoped handle injection
- request `kind` set to `"http"`

Still explicit in your code:

- queue timing
- stage timing
- in-flight instrumentation
- report interpretation

## Important constraints

- Install `middleware` before using `TailtriageRequest`.
- Missing middleware yields `TailtriageExtractorError` and HTTP 500 behavior.
- Route labels prefer Axum `MatchedPath`; fallback is the raw URI path.
- Default status mapping is: 2xx/3xx => `ok`, 4xx => `rejected` (except 408 => `timeout`), 5xx => `error`.

## Minimal handler example

```rust,no_run
use tailtriage_axum::TailtriageRequest;

async fn checkout(TailtriageRequest(req): TailtriageRequest) {
    req.queue("checkout_queue").await_on(async {}).await;
    let _: Result<(), ()> = req.stage("db_call").await_on(async { Ok(()) }).await;
}
```
