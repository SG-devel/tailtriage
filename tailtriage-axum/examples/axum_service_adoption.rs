use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::extract::State;
use axum::http::StatusCode;
use axum::middleware::from_fn_with_state;
use axum::routing::get;
use axum::{Json, Router};
use tailtriage_axum::TailtriageRequest;
use tailtriage_core::Tailtriage;
use tokio::sync::{oneshot, Semaphore};

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

    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr: SocketAddr = listener.local_addr()?;

    let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();
    let server = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(async move {
                let _ = shutdown_rx.await;
            })
            .await
    });

    let client = reqwest::Client::builder().no_proxy().build()?;
    let health_url = format!("http://{addr}/health");
    let checkout_url = format!("http://{addr}/checkout");

    let health_status = client.get(health_url).send().await?.status();
    if health_status != StatusCode::OK {
        return Err(format!("health request failed with {health_status}").into());
    }

    let mut tasks = Vec::new();
    for _ in 0..8 {
        let client = client.clone();
        let url = checkout_url.clone();
        tasks.push(tokio::spawn(async move {
            client.get(url).send().await.map(|resp| resp.status())
        }));
    }

    for task in tasks {
        let status = task.await??;
        if status != StatusCode::OK {
            return Err(format!("checkout request failed with {status}").into());
        }
    }

    let _ = shutdown_tx.send(());
    server.await??;

    tailtriage.shutdown()?;

    println!("Wrote {artifact_path}");
    println!("This is a service-shaped axum adoption example, not a production case study.");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");

    Ok(())
}
