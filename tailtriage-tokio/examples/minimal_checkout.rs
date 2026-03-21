use std::time::Duration;
use tailtriage_core::{Outcome, Tailtriage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output("tailtriage-run.json")
        .build()?;
    let req = tailtriage.request("/checkout").with_kind("http");
    req.queue("ingress_queue")
        .await_on(tokio::time::sleep(Duration::from_millis(3)))
        .await;
    req.stage("db_call")
        .await_value(tokio::time::sleep(Duration::from_millis(8)))
        .await;
    req.complete(Outcome::Ok);

    tailtriage.shutdown()?;

    println!("wrote tailtriage-run.json");
    println!("next: cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json");

    Ok(())
}
