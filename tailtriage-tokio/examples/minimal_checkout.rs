use std::sync::Arc;
use std::time::Duration;

use tailtriage_core::{RequestOptions, Tailtriage};
use tailtriage_tokio::RuntimeSampler;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let artifact_path = "tailtriage-run.json";
    let tailtriage = Arc::new(
        Tailtriage::builder("minimal-checkout")
            .output(artifact_path)
            .build()?,
    );

    // Inherits Light mode defaults from core, then overrides cadence/retention explicitly.
    let sampler = RuntimeSampler::builder(Arc::clone(&tailtriage))
        .interval(Duration::from_millis(200))
        .max_runtime_snapshots(500)
        .start()?;

    let started = tailtriage.begin_request_with("/checkout", RequestOptions::new().kind("http"));
    let request = started.handle.clone();

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

    started.completion.finish_ok();

    sampler.shutdown().await;
    tailtriage.shutdown()?;
    println!("Wrote {artifact_path}");
    println!("Next step:");
    println!("  cargo run -p tailtriage-cli -- analyze {artifact_path} --format json");
    Ok(())
}
