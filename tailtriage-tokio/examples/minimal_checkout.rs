use std::time::Duration;

use tailtriage_core::Tailtriage;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output(std::env::temp_dir().join("tailtriage-minimal-checkout.json"))
        .build()?;

    let request = tailtriage.request("/checkout").kind("http").start();

    request
        .queue("ingress_queue")
        .await_on(tokio::time::sleep(Duration::from_millis(2)))
        .await;

    request
        .stage("db_call")
        .await_value(tokio::time::sleep(Duration::from_millis(6)))
        .await;

    request.finish("ok");

    tailtriage.shutdown()?;
    Ok(())
}
