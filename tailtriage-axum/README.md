# tailtriage-axum

`tailtriage-axum` adds focused Axum middleware and an extractor to `tailtriage`.
Use it when middleware should own request-boundary start and completion while
application code records the internal queue, stage, and in-flight evidence that
makes tail-latency triage useful.

## Installation

The example below directly names Axum, Tokio, `tailtriage-core`, and this crate,
so install each as a direct dependency:

```bash
cargo add axum tailtriage-core tailtriage-axum
cargo add tokio --features macros,rt
```

## Axum adoption

This complete setup uses an in-memory sink, installs the middleware, extracts
the request handle, records an internal stage, and shuts down the collector:

```rust,no_run
use std::sync::Arc;

use axum::{middleware::from_fn_with_state, routing::get, Router};
use tailtriage_axum::{middleware, TailtriageRequest};
use tailtriage_core::{MemorySink, Tailtriage};

async fn checkout(TailtriageRequest(request): TailtriageRequest) -> &'static str {
    // Internal boundaries remain explicit application instrumentation.
    let _: Result<(), ()> = request
        .stage("inventory_lookup")
        .await_on(async {
            /* call the downstream stage */
            Ok(())
        })
        .await;
    "ok"
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Arc::new(
        Tailtriage::builder("checkout-service")
            .sink(MemorySink::new())
            .build()?,
    );

    let app: Router = Router::new()
        .route("/checkout", get(checkout))
        .layer(from_fn_with_state(
            Arc::clone(&tailtriage),
            middleware,
        ));

    // Serve `app` or exercise it in the application workload.
    let _ = app;

    // The middleware finishes each returned request; the application still
    // owns final shutdown of the overall collector/run.
    tailtriage.shutdown()?;
    Ok(())
}
```

The middleware begins capture before running the downstream service, inserts a
`TailtriageRequest` for handler extraction, and finishes after
`next.run(request)` returns a `Response`. The returned status is classified at
that point. It measures response production, not later body consumption: in
particular, polling or consuming a streaming response body occurs outside this
request boundary and cannot change its recorded outcome.

Route labels prefer Axum's normalized `MatchedPath`. When it is unavailable,
the label is the URI path (not its query string), which is not a normalized
route template and can have high cardinality when paths contain concrete IDs.
The default classifier maps 408 to timeout, other 4xx statuses to rejected, 5xx
statuses to error, and every other status to ok. Use
`middleware_with_status_classifier(...)` when the application owns a different
status mapping.

Extraction succeeds only when the request reaching the handler already has the
middleware-inserted context. Otherwise it rejects with
`TailtriageExtractorError` and HTTP 500, which generally indicates missing or
incorrect middleware wiring for that handler path.

The middleware does not automatically instrument internal queues, stages, or
in-flight work, and it does not shut down the collector. Those remain explicit
application responsibilities. Its output is scoped triage evidence for
evidence-ranked suspects and next checks, not proof of root cause, full HTTP
tracing, connection lifetime, socket flush, or client-observed latency.

## Checked examples

- `axum_service_adoption` demonstrates middleware/extractor adoption in a
  service-shaped workload.
- `axum_core_manual` demonstrates equivalent manual request wiring with
  `tailtriage-core`, without this adapter.

If the application does not use Axum, this crate is not the right integration
boundary.
