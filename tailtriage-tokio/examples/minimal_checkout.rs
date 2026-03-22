use std::time::Duration;

use tailtriage_core::Tailtriage;

#[tokio::main(flavor = "current_thread")]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output("tailtriage-run.json")
        .build()?;

    let checkout = tailtriage.request("/checkout").with_kind("http");

    checkout
        .queue("checkout_permit")
        .with_depth_at_start(4)
        .await_on(tokio::time::sleep(Duration::from_millis(8)))
        .await;

    checkout
        .stage("inventory_db")
        .await_on(async {
            tokio::time::sleep(Duration::from_millis(14)).await;
            Ok::<(), &'static str>(())
        })
        .await?;

    checkout
        .run_ok(tokio::time::sleep(Duration::from_millis(6)))
        .await;

    tailtriage.shutdown()?;
    println!("wrote tailtriage-run.json");
    println!("next: cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json");
    Ok(())
}
