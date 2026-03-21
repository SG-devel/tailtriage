use std::time::Duration;
use tailtriage_core::Tailtriage;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let tailtriage = Tailtriage::builder("minimal-checkout")
        .output("tailtriage-run.json")
        .build()?;

    let request = tailtriage.request("/checkout").with_kind("http");
    let started_at = tailtriage_core::unix_time_ms();
    let started = std::time::Instant::now();
    request
        .queue("ingress_queue")
        .await_on(tokio::time::sleep(Duration::from_millis(3)))
        .await;
    request
        .stage("db_call")
        .await_value(tokio::time::sleep(Duration::from_millis(8)))
        .await;
    request.complete(
        (started_at, tailtriage_core::unix_time_ms()),
        started.elapsed().as_micros().try_into().unwrap_or(u64::MAX),
        "ok",
    );

    tailtriage.shutdown()?;

    println!("wrote tailtriage-run.json");
    println!("next: cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json");

    Ok(())
}
