use std::time::Duration;
use tailtriage_core::{Config, RequestMeta, Tailtriage};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut config = Config::new("minimal-checkout");
    config.output_path = "tailtriage-run.json".into();

    let tailtriage = Tailtriage::init(config)?;

    let meta = RequestMeta::for_route("/checkout").with_kind("http");
    let request_id = meta.request_id.clone();

    tailtriage
        .request(meta, "ok", async {
            tailtriage
                .queue(request_id.clone(), "ingress_queue")
                .await_on(tokio::time::sleep(Duration::from_millis(3)))
                .await;

            tailtriage
                .stage(request_id, "db_call")
                .await_value(tokio::time::sleep(Duration::from_millis(8)))
                .await;
        })
        .await;

    tailtriage.flush()?;

    println!("wrote tailtriage-run.json");
    println!("next: cargo run -p tailtriage-cli -- analyze tailtriage-run.json --format json");

    Ok(())
}
