use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};
use tailtriage_core::RequestMeta;
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

    let tailtriage = init_collector("mixed_contention_service_demo", &args.output_path)?;

    let semaphore = Arc::new(Semaphore::new(settings.service_capacity));
    let waiting_depth = Arc::new(AtomicU64::new(0));

    let mut tasks = Vec::with_capacity(settings.offered_requests as usize);

    for request_number in 0..settings.offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let semaphore = Arc::clone(&semaphore);
        let waiting_depth = Arc::clone(&waiting_depth);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let meta = RequestMeta::new(request_id.clone(), "/mixed-contention-demo");

            tailtriage
                .request_with_meta(meta, "ok", async {
                    let _inflight = tailtriage.inflight("mixed_contention_inflight");

                    let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
                    let permit = tailtriage
                        .queue(request_id.clone(), "worker_permit")
                        .with_depth_at_start(depth)
                        .await_on(semaphore.acquire())
                        .await
                        .expect("semaphore should remain open");
                    waiting_depth.fetch_sub(1, Ordering::SeqCst);

                    let _permit = permit;

                    tailtriage
                        .stage(request_id.clone(), "app_prepare")
                        .await_value(tokio::time::sleep(settings.app_stage_delay))
                        .await;

                    let extra_downstream = if request_number.is_multiple_of(4) {
                        settings.downstream_slow_delay
                    } else {
                        Duration::ZERO
                    };
                    tailtriage
                        .stage(request_id, "downstream_call")
                        .await_value(tokio::time::sleep(
                            settings.downstream_base_delay + extra_downstream,
                        ))
                        .await;
                })
                .await;
        }));

        if request_number % settings.inter_arrival_pause_every == 0 {
            tokio::time::sleep(settings.inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    tailtriage.flush()?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
