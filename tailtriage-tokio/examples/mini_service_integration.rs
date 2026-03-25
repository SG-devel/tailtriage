use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{RequestOptions, Tailtriage};

#[derive(Clone)]
struct CheckoutRequest {
    request_id: String,
    cart_total_cents: u64,
}

async fn authorize_payment(
    request: &tailtriage_core::RequestHandle<'_>,
) -> Result<(), &'static str> {
    request
        .stage("payment_authorization")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(4)).await;
            Ok::<(), &'static str>(())
        })
        .await
}

async fn reserve_inventory(
    request: &tailtriage_core::RequestHandle<'_>,
    cart_total_cents: u64,
) -> Result<(), &'static str> {
    let reserve_ms = if cart_total_cents > 700 { 9 } else { 4 };
    request
        .stage("reserve_inventory")
        .await_on(async move {
            tokio::time::sleep(Duration::from_millis(reserve_ms)).await;
            Ok::<(), &'static str>(())
        })
        .await
}

async fn handle_checkout(
    tailtriage: Arc<Tailtriage>,
    request: CheckoutRequest,
) -> Result<(), &'static str> {
    let started = tailtriage
        .begin_request_with(
            "/checkout",
            RequestOptions::new().request_id(request.request_id),
        )
        .with_kind("http");
    let request_ctx = started.handle.clone();

    let result = async {
        let _inflight = request_ctx.inflight("checkout_inflight");

        request_ctx
            .queue("checkout_permit")
            .with_depth_at_start(2)
            .await_on(tokio::time::sleep(Duration::from_millis(2)))
            .await;

        reserve_inventory(&request_ctx, request.cart_total_cents).await?;

        request_ctx
            .stage("pricing")
            .await_value(tokio::time::sleep(Duration::from_millis(
                request.cart_total_cents / 50,
            )))
            .await;

        authorize_payment(&request_ctx).await
    }
    .await;

    started.completion.finish_result(result)
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = "tailtriage-run.json";
    let tailtriage = Arc::new(
        Tailtriage::builder("mini-checkout-service")
            .output(output_path)
            .investigation()
            .build()?,
    );
    let requests = [
        CheckoutRequest {
            request_id: "req-101".to_string(),
            cart_total_cents: 180,
        },
        CheckoutRequest {
            request_id: "req-102".to_string(),
            cart_total_cents: 950,
        },
        CheckoutRequest {
            request_id: "req-103".to_string(),
            cart_total_cents: 320,
        },
    ];

    for request in requests {
        handle_checkout(Arc::clone(&tailtriage), request).await?;
    }

    tailtriage.shutdown()?;

    println!("Wrote {output_path}");
    println!("This example demonstrates a small integration flow across helper layers.");
    println!("Analyze it with:");
    println!("  cargo run -p tailtriage-cli -- analyze {output_path} --format json");
    Ok(())
}
