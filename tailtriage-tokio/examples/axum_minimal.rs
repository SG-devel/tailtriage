use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use axum::{extract::State, http::StatusCode, routing::get, Router};
use tailtriage_core::Tailtriage;
use tokio::sync::{oneshot, Semaphore};

#[derive(Clone)]
struct AppState {
    tailtriage: Arc<Tailtriage>,
    queue_gate: Arc<Semaphore>,
    queue_capacity: u64,
}

async fn checkout_handler(State(state): State<AppState>) -> StatusCode {
    let request = state.tailtriage.request("/checkout").with_kind("http");

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

    if request.finish_result(result).is_ok() {
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

    let base_url = format!("http://{addr}/checkout");
    let client = reqwest::Client::builder().no_proxy().build()?;

    let mut tasks = Vec::new();
    for _ in 0..6 {
        let client = client.clone();
        let url = base_url.clone();
        tasks.push(tokio::spawn(async move {
            client.get(url).send().await.map(|resp| resp.status())
        }));
    }

    for task in tasks {
        let status = task.await??;
        if status != StatusCode::OK {
            return Err(format!("request failed with status {status}").into());
        }
    }

    let _ = shutdown_tx.send(());
    server.await??;

    tailtriage.shutdown()?;

    println!("Wrote {artifact_path}");
    println!("This axum example is a framework adoption starter, not a production case study.");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");

    Ok(())
}
