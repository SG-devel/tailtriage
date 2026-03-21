use std::time::Duration;
use tailtriage_core::{Outcome, Tailtriage};

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let output_path = std::env::temp_dir().join("tailtriage_minimal_checkout.json");

    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output(&output_path)
        .build()?;

    let req = tailtriage.request("/checkout").with_kind("place_order");
    let _inflight = req.inflight("checkout_inflight");
    req.queue("admission")
        .with_depth_at_start(3)
        .await_on(tokio::time::sleep(Duration::from_millis(4)))
        .await;
    req.stage("db")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(12)).await;
            Ok::<(), &'static str>(())
        })
        .await?;
    req.complete(Outcome::Ok);

    tailtriage.shutdown()?;
    println!("wrote {}", output_path.display());
    Ok(())
}
