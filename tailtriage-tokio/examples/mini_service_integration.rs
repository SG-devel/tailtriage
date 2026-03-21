use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{Config, RequestMeta, Tailtriage};
use tokio::sync::{mpsc, oneshot};

#[derive(Debug)]
struct CheckoutRequest {
    request_id: String,
    sku: &'static str,
    quantity: u32,
}

#[derive(Debug)]
struct WorkItem {
    request: CheckoutRequest,
    completion_tx: oneshot::Sender<Result<(), &'static str>>,
}

async fn handle_checkout(
    tailtriage: &Tailtriage,
    tx: &mpsc::Sender<WorkItem>,
    request: CheckoutRequest,
) -> Result<(), &'static str> {
    let meta = RequestMeta::new(request.request_id.clone(), "/checkout").with_kind("http");
    let request_id = meta.request_id.clone();

    tailtriage
        .request(meta, "ok", async {
            let (completion_tx, completion_rx) = oneshot::channel();

            tailtriage
                .queue(request_id.clone(), "checkout_ingress")
                .await_on(tx.send(WorkItem {
                    request,
                    completion_tx,
                }))
                .await
                .map_err(|_| "worker channel closed")?;

            completion_rx.await.map_err(|_| "worker dropped response")?
        })
        .await
}

async fn run_worker(tailtriage: Arc<Tailtriage>, mut rx: mpsc::Receiver<WorkItem>) {
    while let Some(work) = rx.recv().await {
        let request_id = work.request.request_id.clone();

        tailtriage
            .stage(request_id.clone(), "inventory_lookup")
            .await_value(tokio::time::sleep(Duration::from_millis(4)))
            .await;

        let payment_delay_ms = if work.request.quantity > 2 { 11 } else { 6 };
        tailtriage
            .stage(request_id, "payment_authorization")
            .await_value(tokio::time::sleep(Duration::from_millis(payment_delay_ms)))
            .await;

        let _ = work.completion_tx.send(Ok(()));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new("mini-checkout-service");
    config.output_path = "tailtriage-run.json".into();

    let tailtriage = Arc::new(Tailtriage::init(config)?);
    let (tx, rx) = mpsc::channel(8);

    let worker = tokio::spawn(run_worker(Arc::clone(&tailtriage), rx));

    for index in 0..12 {
        let request = CheckoutRequest {
            request_id: format!("checkout-{index}"),
            sku: "SKU-123",
            quantity: if index % 4 == 0 { 3 } else { 1 },
        };

        if request.sku.starts_with("SKU-") {
            handle_checkout(&tailtriage, &tx, request).await?;
        }
    }

    drop(tx);
    worker.await?;

    tailtriage.flush()?;

    println!("wrote tailtriage-run.json from mini_service_integration");
    println!("next: cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json");
    println!("inspect first: primary_suspect.kind, evidence[], next_checks[]");

    Ok(())
}
