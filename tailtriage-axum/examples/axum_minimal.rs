use std::sync::Arc;
use std::time::Duration;

use axum::body::Body;
use axum::http::Request;
use axum::{extract::State, http::StatusCode, routing::get, Router};
use tailtriage_core::{RequestOptions, Tailtriage};
use tokio::sync::Semaphore;
use tower::ServiceExt;

#[derive(Clone)]
struct AppState {
    tailtriage: Arc<Tailtriage>,
    queue_gate: Arc<Semaphore>,
    queue_capacity: u64,
}

async fn checkout_handler(State(state): State<AppState>) -> StatusCode {
    let started = state
        .tailtriage
        .begin_request_with("/checkout", RequestOptions::new().kind("http"));
    let request = started.handle.clone();

    let result = async {
        let queue_depth = state
            .queue_capacity
            .saturating_sub(state.queue_gate.available_permits() as u64);

        let _permit = request
            .queue("checkout_worker_queue")
            .with_depth_at_start(queue_depth)
            .await_on(state.queue_gate.clone().acquire_owned())
            .await
            .map_err(|_closed| ())?;

        request
            .stage("inventory_lookup")
            .await_on(async {
                tokio::time::sleep(Duration::from_millis(14)).await;
                Ok::<(), ()>(())
            })
            .await?;

        request
            .stage("payment_gateway")
            .await_on(async {
                tokio::time::sleep(Duration::from_millis(20)).await;
                Ok::<(), ()>(())
            })
            .await
    }
    .await;

    if started.completion.finish_result(result).is_ok() {
        StatusCode::OK
    } else {
        StatusCode::INTERNAL_SERVER_ERROR
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let artifact_path = "tailtriage-run.json";
    let tailtriage = Arc::new(
        Tailtriage::builder("axum-adoption-starter")
            .output(artifact_path)
            .build()?,
    );

    let queue_capacity: usize = 1;
    let app_state = AppState {
        tailtriage: Arc::clone(&tailtriage),
        queue_gate: Arc::new(Semaphore::new(queue_capacity)),
        queue_capacity: queue_capacity as u64,
    };

    let app = Router::new()
        .route("/checkout", get(checkout_handler))
        .with_state(app_state);

    // Simulate concurrent checkout load in-process.
    let mut tasks = Vec::new();
    for _ in 0..6 {
        let app = app.clone();
        let request = Request::builder()
            .uri("/checkout")
            .body(Body::empty())
            .expect("request should build");
        tasks.push(tokio::spawn(async move { app.oneshot(request).await }));
    }

    for task in tasks {
        let status = task.await??.status();
        if status != StatusCode::OK {
            return Err(format!("request failed with status {status}").into());
        }
    }

    tailtriage.shutdown()?;

    println!("Wrote {artifact_path}");
    println!("This axum example is a framework adoption starter, not a production case study.");
    println!("For a larger service-shaped example, run:");
    println!("  cargo run -p tailtriage-axum --example axum_service_adoption");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");

    Ok(())
}
