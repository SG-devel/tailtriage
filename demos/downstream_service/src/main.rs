use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_output_arg};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let output_path = parse_output_arg("demos/downstream_service/artifacts/downstream-run.json")?;

    let tailtriage = init_collector("downstream_service_demo", &output_path)?;

    let offered_requests = 80_u64;
    let mut tasks = Vec::with_capacity(offered_requests as usize);

    for request_number in 0..offered_requests {
        let tailtriage = Arc::clone(&tailtriage);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let request = tailtriage.request_with_id("/downstream-demo", request_id.clone());

            {
                let _inflight = tailtriage.inflight("downstream_service_inflight");

                request
                    .stage("app_precheck")
                    .await_value(tokio::time::sleep(Duration::from_millis(1)))
                    .await;

                request
                    .stage("downstream_call")
                    .await_value(tokio::time::sleep(Duration::from_millis(20)))
                    .await;
            }
            request.complete("ok");
        }));

        if request_number.is_multiple_of(8) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    tailtriage.shutdown()?;
    println!("wrote {}", output_path.display());

    Ok(())
}
