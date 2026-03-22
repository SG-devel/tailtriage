use std::time::Duration;

use tailtriage_core::Tailtriage;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let artifact_path = "tailtriage-run.json";
    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output(artifact_path)
        .build()?;

    let request = tailtriage.request("/checkout").with_kind("http");

    request
        .queue("checkout_worker_queue")
        .with_depth_at_start(4)
        .await_on(tokio::time::sleep(Duration::from_millis(6)))
        .await;

    request
        .stage("inventory_lookup")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(8)).await;
            Ok::<(), &'static str>(())
        })
        .await?;

    request
        .stage("payment_gateway")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(12)).await;
            Ok::<(), &'static str>(())
        })
        .await?;

    request.run_ok(async {}).await;

    tailtriage.shutdown()?;
    println!("Wrote {artifact_path}");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");
    Ok(())
}
