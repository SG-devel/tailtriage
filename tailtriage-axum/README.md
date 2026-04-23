# tailtriage-axum

`tailtriage-axum` provides Axum-first request-boundary wiring for `tailtriage`.

Use it when you want middleware to start and finish request lifecycle automatically at the Axum boundary, while still keeping queue/stage/inflight instrumentation explicit inside handlers or helper code.

## What this crate does

This crate gives you three Axum-facing pieces:

- `middleware` for default request start/finish at the boundary
- `middleware_with_status_classifier(...)` to customize HTTP-status -> outcome mapping
- `TailtriageRequest` extractor to access the request-scoped handle in handlers

This crate is about integration ergonomics. It does not replace explicit instrumentation inside the request body.

The primary integration path in this crate is `middleware`, `middleware_with_status_classifier(...)`, and `TailtriageRequest`.

## When to choose this crate

Choose `tailtriage-axum` when:

- you already use Axum
- you do not want to manually wire request start/finish in every handler
- you still want explicit queue/stage/inflight instrumentation inside the request path

Choose `tailtriage-core` directly when you want framework-agnostic manual instrumentation.

Choose `tailtriage` when you want the default entry point and feature-gated Axum support.

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

## Examples

- `axum_service_adoption`: primary service-shaped example using `tailtriage-axum` middleware + `TailtriageRequest`.
- `axum_core_manual`: manual Axum + `tailtriage-core` wiring for equivalent framework integration without `tailtriage-axum`.

## Automatic vs explicit responsibilities

Automatic at the Axum boundary:

- request start
- request finish
- request-scoped handle injection into handlers
- request `kind` is set to `"http"`

Still explicit in your code:

- queue timing
- stage timing
- in-flight instrumentation
- interpretation of the resulting artifact

That split is important: this crate helps you integrate capture at the framework boundary, but it does not diagnose the slowdown by itself.

## Important constraints

- install `middleware` before using `TailtriageRequest`
- missing middleware yields `TailtriageExtractorError` with HTTP 500 behavior
- route labels prefer Axum `MatchedPath`; the fallback is the raw URI path
- analysis still happens in `tailtriage-cli`

## Minimal handler example

```rust,no_run
use tailtriage_axum::TailtriageRequest;

async fn checkout(TailtriageRequest(req): TailtriageRequest) {
    req.queue("checkout_queue").await_on(async {}).await;
    let _: Result<(), ()> = req.stage("db_call").await_on(async { Ok(()) }).await;
}
```

## When not to use this crate

Do not add this crate just to analyze artifacts or rank suspects.

It is only for Axum integration ergonomics.

If you do not use Axum, this crate is not the right abstraction boundary.

## Related crates

- `tailtriage`: recommended default entry point
- `tailtriage-core`: framework-agnostic instrumentation primitives
- `tailtriage-tokio`: runtime-pressure sampling
- `tailtriage-cli`: artifact analysis and report generation
