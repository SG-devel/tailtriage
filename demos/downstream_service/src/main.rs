use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_recorder, parse_demo_args, DemoMode};

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

    let settings = DownstreamSettings::for_mode(args.mode);

    let recorder = init_recorder(
        "downstream_service_demo",
        &args.output_path,
        args.instrumentation,
    )?;

    let offered_requests = 80_u64;
    let task_capacity = usize::try_from(offered_requests)?;
    let mut tasks = Vec::with_capacity(task_capacity);

    for request_number in 0..offered_requests {
        let recorder = Arc::clone(&recorder);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let request = recorder.begin_request("/downstream-demo", request_id.clone());

            request
                .record_stage(
                    "app_precheck",
                    tokio::time::sleep(settings.app_precheck_delay),
                )
                .await;

            request
                .record_stage(
                    "downstream_call",
                    tokio::time::sleep(settings.downstream_delay),
                )
                .await;
            request.finish(tailtriage_core::Outcome::Ok);
        }));

        if request_number.is_multiple_of(8) {
            tokio::time::sleep(Duration::from_millis(2)).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    recorder.shutdown(&args.output_path)?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
