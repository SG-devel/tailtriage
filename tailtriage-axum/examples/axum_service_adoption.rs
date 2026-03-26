use std::sync::Arc;
use std::time::Duration;

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::{Json, Router};
use tailtriage_axum::TailtriageRequest;
use tailtriage_core::Tailtriage;
use tokio::sync::Semaphore;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    queue_gate: Arc<Semaphore>,
    queue_capacity: u64,
}

#[derive(serde::Serialize)]
struct CheckoutResponse {
    status: &'static str,
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

async fn checkout_handler(
    TailtriageRequest(req): TailtriageRequest,
    State(state): State<AppState>,
) -> Result<Json<CheckoutResponse>, StatusCode> {
    let queue_depth = state
        .queue_capacity
        .saturating_sub(state.queue_gate.available_permits() as u64);

    let _permit = req
        .queue("checkout_admission")
        .with_depth_at_start(queue_depth)
        .await_on(state.queue_gate.clone().acquire_owned())
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;

    let _checkout_inflight = req.inflight("checkout_inflight");
    inventory_lookup(&req).await?;
    payment_gateway(&req).await?;

    Ok(Json(CheckoutResponse { status: "ok" }))
}

async fn inventory_lookup(req: &tailtriage_core::OwnedRequestHandle) -> Result<(), StatusCode> {
    req.stage("inventory_lookup")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(10)).await;
            Ok::<(), StatusCode>(())
        })
        .await
}

async fn payment_gateway(req: &tailtriage_core::OwnedRequestHandle) -> Result<(), StatusCode> {
    req.stage("payment_gateway")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(18)).await;
            Ok::<(), StatusCode>(())
        })
        .await
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let artifact_path = "tailtriage-run.json";
    let tailtriage = Arc::new(
        Tailtriage::builder("axum-service-adoption")
            .output(artifact_path)
            .build()?,
    );

    let queue_capacity = 2;
    let app_state = AppState {
        queue_gate: Arc::new(Semaphore::new(queue_capacity)),
        queue_capacity: queue_capacity as u64,
    };

    let app = Router::new()
        .route("/checkout", get(checkout_handler))
        .route("/health", get(health_handler))
        .layer(from_fn_with_state(
            Arc::clone(&tailtriage),
            tailtriage_axum::middleware,
        ))
        .with_state(app_state);

    let health_status = app
        .clone()
        .oneshot(Request::builder().uri("/health").body(Body::empty())?)
        .await?
        .status();
    if health_status != StatusCode::OK {
        return Err(format!("health request failed with {health_status}").into());
    }

    // Drive concurrent checkout load in-process so queueing/inflight pressure
    // remains visible without reintroducing localhost networking.
    let mut tasks = Vec::new();
    for _ in 0..8 {
        let app = app.clone();
        let request = Request::builder()
            .uri("/checkout")
            .body(Body::empty())
            .expect("request should build");
        tasks.push(tokio::spawn(async move {
            let response = app.oneshot(request).await?;
            let status = response.status();
            let body = to_bytes(response.into_body(), usize::MAX).await?;
            let payload: serde_json::Value = serde_json::from_slice(&body)?;
            Ok::<_, Box<dyn std::error::Error + Send + Sync>>((status, payload))
        }));
    }

    for task in tasks {
        let (status, payload) = task
            .await
            .map_err(|err| format!("checkout task join failed: {err}"))?
            .map_err(|err| format!("checkout task failed: {err}"))?;
        if status != StatusCode::OK {
            return Err(format!("checkout request failed with {status}").into());
        }
        if payload != serde_json::json!({ "status": "ok" }) {
            return Err(format!("checkout payload mismatch: {payload}").into());
        }
    }

    tailtriage.shutdown()?;

    println!("Wrote {artifact_path}");
    println!("This is a service-shaped axum adoption example, not a production case study.");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");

    Ok(())
}
