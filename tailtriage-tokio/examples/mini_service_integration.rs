use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{RequestContext, RequestOptions, Tailtriage};

#[derive(Clone, Copy)]
struct CheckoutRequest {
    request_index: usize,
    cart_total_cents: u64,
}

async fn queue_for_worker(request: &RequestContext<'_>, depth: u64) {
    request
        .queue("checkout_worker")
        .with_depth_at_start(depth)
        .await_on(tokio::time::sleep(Duration::from_millis(3 + depth)))
        .await;
}

async fn load_inventory(
    request: &RequestContext<'_>,
    cart_total_cents: u64,
) -> Result<(), &'static str> {
    let extra = cart_total_cents / 120;
    request
        .stage("inventory_db")
        .await_on(async move {
            tokio::time::sleep(Duration::from_millis(8 + extra)).await;
            Ok::<(), &'static str>(())
        })
        .await
}

async fn authorize_payment(
    request: &RequestContext<'_>,
    request_index: usize,
) -> Result<(), &'static str> {
    let attempt_cost = if request_index % 3 == 0 { 11 } else { 6 };
    request
        .stage("payment_gateway")
        .await_on(async move {
            tokio::time::sleep(Duration::from_millis(attempt_cost)).await;
            Ok::<(), &'static str>(())
        })
        .await
}

async fn handle_checkout(
    tailtriage: Arc<Tailtriage>,
    request: CheckoutRequest,
) -> Result<(), &'static str> {
    let request_id = format!("req-{:02}", request.request_index);
    let request_ctx = tailtriage
        .request_with("/checkout", RequestOptions::new().request_id(request_id))
        .with_kind("http");

    {
        let _inflight = request_ctx.inflight("checkout_inflight");
        queue_for_worker(&request_ctx, 2 + (request.request_index % 4) as u64).await;
        load_inventory(&request_ctx, request.cart_total_cents).await?;
        authorize_payment(&request_ctx, request.request_index).await?;
    }

    request_ctx
        .run_result(async { Ok::<(), &'static str>(()) })
        .await
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = "tailtriage-mini-service.json";
    let tailtriage = Arc::new(
        Tailtriage::builder("mini-checkout-service")
            .output(output_path)
            .build()?,
    );

    for request in [
        CheckoutRequest {
            request_index: 0,
            cart_total_cents: 220,
        },
        CheckoutRequest {
            request_index: 1,
            cart_total_cents: 640,
        },
        CheckoutRequest {
            request_index: 2,
            cart_total_cents: 175,
        },
        CheckoutRequest {
            request_index: 3,
            cart_total_cents: 470,
        },
        CheckoutRequest {
            request_index: 4,
            cart_total_cents: 310,
        },
    ] {
        handle_checkout(Arc::clone(&tailtriage), request).await?;
    }

    tailtriage.shutdown()?;

    println!("wrote {output_path}");
    println!("next: cargo run -p tailtriage-cli -- analyze {output_path} --format json");
    Ok(())
}
