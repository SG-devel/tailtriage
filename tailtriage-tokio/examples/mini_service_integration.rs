use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{Outcome, RequestOptions, Tailtriage};
use tailtriage_tokio::RuntimeSampler;

#[derive(Clone)]
struct CheckoutRequest {
    request_id: String,
    cart_total_cents: u64,
}

async fn authorize_payment(
    request: &tailtriage_core::RequestContext<'_>,
) -> Result<(), &'static str> {
    request
        .stage("payment_authorization")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(4)).await;
            Ok::<(), &'static str>(())
        })
        .await
}

async fn handle_checkout(
    tailtriage: Arc<Tailtriage>,
    request: CheckoutRequest,
) -> Result<(), &'static str> {
    let request_ctx = tailtriage
        .request_with(
            "/checkout",
            RequestOptions::new().request_id(request.request_id),
        )
        .with_kind("http");

    {
        let _inflight = request_ctx.inflight("checkout_inflight");

        request_ctx
            .queue("checkout_permit")
            .with_depth_at_start(2)
            .await_on(tokio::time::sleep(Duration::from_millis(2)))
            .await;

        request_ctx
            .stage("inventory_lookup")
            .await_on(async {
                tokio::time::sleep(Duration::from_millis(3)).await;
                Ok::<(), &'static str>(())
            })
            .await?;

        request_ctx
            .stage("pricing")
            .await_value(tokio::time::sleep(Duration::from_millis(
                request.cart_total_cents / 50,
            )))
            .await;

        authorize_payment(&request_ctx).await?;
    }
    request_ctx.complete(Outcome::Ok);
    Ok(())
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = std::env::temp_dir().join("tailtriage-mini-service.json");
    let tailtriage = Arc::new(
        Tailtriage::builder("mini-checkout-service")
            .output(&output_path)
            .investigation()
            .build()?,
    );

    let sampler = RuntimeSampler::start(Arc::clone(&tailtriage), Duration::from_millis(5))?;

    let request = CheckoutRequest {
        request_id: "req-123".to_string(),
        cart_total_cents: 240,
    };

    handle_checkout(Arc::clone(&tailtriage), request).await?;

    sampler.shutdown().await;
    tailtriage.shutdown()?;

    println!("artifact: {}", output_path.display());
    Ok(())
}
