use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::Tailtriage;

#[derive(Debug, Clone)]
struct CheckoutRequest {
    request_id: String,
}

async fn handle_checkout(
    tailtriage: Arc<Tailtriage>,
    request: CheckoutRequest,
) -> Result<(), Box<dyn std::error::Error>> {
    let request = tailtriage
        .request("/checkout")
        .kind("http")
        .request_id(request.request_id)
        .start();

    request
        .queue("checkout_ingress")
        .await_on(tokio::time::sleep(Duration::from_millis(3)))
        .await;

    inventory_lookup(&request).await;
    payment_authorization(&request).await;

    request.finish("ok");
    Ok(())
}

async fn inventory_lookup(request: &tailtriage_core::RequestContext<'_>) {
    request
        .stage("inventory_lookup")
        .await_value(tokio::time::sleep(Duration::from_millis(4)))
        .await;
}

async fn payment_authorization(request: &tailtriage_core::RequestContext<'_>) {
    request
        .stage("payment_authorization")
        .await_value(tokio::time::sleep(Duration::from_millis(8)))
        .await;
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Arc::new(
        Tailtriage::builder("mini-checkout-service")
            .output(std::env::temp_dir().join("tailtriage-mini-service-integration.json"))
            .build()?,
    );

    handle_checkout(
        Arc::clone(&tailtriage),
        CheckoutRequest {
            request_id: "req-123".to_string(),
        },
    )
    .await?;

    tailtriage.shutdown()?;
    Ok(())
}
