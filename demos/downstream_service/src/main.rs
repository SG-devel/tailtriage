use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{Config, RequestMeta, Tailtriage};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let output_path = std::env::args()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("demos/downstream_service/artifacts/downstream-run.json"));

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }

    let mut config = Config::new("downstream_service_demo");
    config.output_path = output_path.clone();
    let tailtriage = Arc::new(Tailtriage::init(config)?);

    let offered_requests = 80_u64;
    let mut tasks = Vec::with_capacity(offered_requests as usize);

    for request_number in 0..offered_requests {
        let tailtriage = Arc::clone(&tailtriage);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let meta = RequestMeta::new(request_id.clone(), "/downstream-demo");

            tailtriage
                .request(meta, "ok", async {
                    let _inflight = tailtriage.inflight("downstream_service_inflight");

                    tailtriage
                        .stage(request_id.clone(), "app_precheck")
                        .await_value(tokio::time::sleep(Duration::from_millis(1)))
                        .await;

                    tailtriage
                        .stage(request_id, "downstream_call")
                        .await_value(tokio::time::sleep(Duration::from_millis(20)))
                        .await;
                })
                .await;
        }));

        if request_number % 8 == 0 {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    tailtriage.flush()?;
    println!("wrote {}", output_path.display());

    Ok(())
}
