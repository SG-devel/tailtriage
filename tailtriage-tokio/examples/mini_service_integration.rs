use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{Outcome, RequestOptions, Tailtriage};
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
    let req = tailtriage
        .request_with(
            "/checkout",
            RequestOptions::new().request_id(request.request_id.clone()),
        )
        .with_kind("http");

    let (completion_tx, completion_rx) = oneshot::channel();

    req.queue("checkout_ingress")
        .await_on(tx.send(WorkItem {
            request,
            completion_tx,
        }))
        .await
        .map_err(|_| "worker channel closed")?;

    req.stage("worker_roundtrip")
        .await_on(async { completion_rx.await.map_err(|_| "worker dropped response")? })
        .await?;

    req.complete(Outcome::Ok);
    Ok(())
}

async fn run_worker(mut rx: mpsc::Receiver<WorkItem>) {
    while let Some(work) = rx.recv().await {
        let payment_delay_ms = if work.request.quantity > 2 { 11 } else { 6 };
        tokio::time::sleep(Duration::from_millis(4 + payment_delay_ms)).await;
        let _ = work.completion_tx.send(Ok(()));
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Arc::new(Tailtriage::builder("mini-checkout-service").build()?);
    let (tx, rx) = mpsc::channel(8);

    let worker = tokio::spawn(run_worker(rx));

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

    tailtriage.shutdown()?;

    println!("wrote tailtriage-run.json from mini_service_integration");
    println!("next: cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json");
    println!("inspect first: primary_suspect.kind, evidence[], next_checks[]");

    Ok(())
}
