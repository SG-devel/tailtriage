use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};

#[derive(Clone, Copy)]
struct DownstreamSettings {
    app_precheck_delay: Duration,
    downstream_delay: Duration,
}

impl DownstreamSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                app_precheck_delay: Duration::from_millis(1),
                downstream_delay: Duration::from_millis(20),
            },
            DemoMode::Mitigated => Self {
                app_precheck_delay: Duration::from_millis(1),
                downstream_delay: Duration::from_millis(9),
            },
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args = parse_demo_args("demos/downstream_service/artifacts/downstream-run.json")?;
    let output_path = args.output_path;
    let settings = DownstreamSettings::for_mode(args.mode);

    let tailtriage = init_collector("downstream_service_demo", &output_path)?;

    let offered_requests = 80_u64;
    let mut tasks = Vec::with_capacity(offered_requests as usize);

    for request_number in 0..offered_requests {
        let tailtriage = Arc::clone(&tailtriage);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let request = tailtriage.request_with(
                "/downstream-demo",
                tailtriage_core::RequestOptions::new().request_id(request_id.clone()),
            );

            {
                let _inflight = request.inflight("downstream_service_inflight");

                request
                    .stage("app_precheck")
                    .await_value(tokio::time::sleep(settings.app_precheck_delay))
                    .await;

                request
                    .stage("downstream_call")
                    .await_value(tokio::time::sleep(settings.downstream_delay))
                    .await;
            }
            request.finish(tailtriage_core::Outcome::Ok);
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
