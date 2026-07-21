use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{parse_demo_args, DemoInstrumentation, DemoMode};
use tokio::sync::Semaphore;

struct ModeSettings {
    service_capacity: usize,
    offered_requests: u64,
    inter_arrival_pause_every: u64,
    inter_arrival_delay: Duration,
    app_stage_delay: Duration,
    downstream_base_delay: Duration,
    downstream_slow_delay: Duration,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                service_capacity: 5,
                offered_requests: 220,
                inter_arrival_pause_every: 6,
                inter_arrival_delay: Duration::from_millis(1),
                app_stage_delay: Duration::from_millis(1),
                downstream_base_delay: Duration::from_millis(11),
                downstream_slow_delay: Duration::from_millis(20),
            },
            DemoMode::Mitigated => Self {
                service_capacity: 14,
                offered_requests: 220,
                inter_arrival_pause_every: 2,
                inter_arrival_delay: Duration::from_millis(2),
                app_stage_delay: Duration::from_millis(1),
                downstream_base_delay: Duration::from_millis(11),
                downstream_slow_delay: Duration::from_millis(20),
            },
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args =
        parse_demo_args("demos/mixed_contention_service/artifacts/mixed-contention-run.json")?;
    let settings = ModeSettings::for_mode(args.mode);

    let instrumentation = Arc::new(DemoInstrumentation::new(
        "mixed_contention_service_demo",
        &args.output_path,
        args.instrumentation,
        args.capture_config(),
    )?);

    let semaphore = Arc::new(Semaphore::new(settings.service_capacity));
    let waiting_depth = Arc::new(AtomicU64::new(0));

    let capacity = usize::try_from(settings.offered_requests)?;
    let mut tasks = Vec::with_capacity(capacity);

    for request_number in 0..settings.offered_requests {
        let semaphore = Arc::clone(&semaphore);
        let waiting_depth = Arc::clone(&waiting_depth);
        let instrumentation = Arc::clone(&instrumentation);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            instrumentation
                .run_request(
                    "/mixed-contention-demo",
                    request_id,
                    tailtriage_core::Outcome::Ok,
                    |request| async move {
                        let _inflight = request.inflight("mixed_contention_inflight");

                        let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
                        let permit = request
                            .queue_wait("worker_permit", depth, semaphore.acquire())
                            .await
                            .expect("semaphore should remain open");
                        waiting_depth.fetch_sub(1, Ordering::SeqCst);

                        let _permit = permit;

                        request
                            .stage("app_prepare", tokio::time::sleep(settings.app_stage_delay))
                            .await;

                        let extra_downstream = if request_number.is_multiple_of(4) {
                            settings.downstream_slow_delay
                        } else {
                            Duration::ZERO
                        };
                        request
                            .stage(
                                "downstream_call",
                                tokio::time::sleep(
                                    settings.downstream_base_delay + extra_downstream,
                                ),
                            )
                            .await;
                    },
                )
                .await;
        }));

        if request_number % settings.inter_arrival_pause_every == 0 {
            tokio::time::sleep(settings.inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    Arc::into_inner(instrumentation)
        .expect("instrumentation still referenced")
        .shutdown(&args.output_path)
        .await?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
