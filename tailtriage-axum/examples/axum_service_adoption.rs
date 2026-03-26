use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::Request;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::Router;
use tailtriage_axum::TailtriageRequest;
use tailtriage_core::Tailtriage;
use tokio::sync::Semaphore;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    queue_gate: Arc<Semaphore>,
    queue_capacity: u64,
}

async fn health_handler() -> StatusCode {
    StatusCode::OK
}

async fn checkout_handler(
    TailtriageRequest(req): TailtriageRequest,
    State(state): State<AppState>,
) -> Result<StatusCode, StatusCode> {
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

    Ok(StatusCode::OK)
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

    for _ in 0..8 {
        let status = app
            .clone()
            .oneshot(Request::builder().uri("/checkout").body(Body::empty())?)
            .await?
            .status();
        if status != StatusCode::OK {
            return Err(format!("checkout request failed with {status}").into());
        }
    }

    tailtriage.shutdown()?;

    println!("Wrote {artifact_path}");
    println!("This is a service-shaped axum adoption example, not a production case study.");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");

    Ok(())
}
