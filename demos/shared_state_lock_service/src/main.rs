use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{parse_demo_args, DemoInstrumentation, DemoMode};
use tokio::sync::RwLock;

struct ModeSettings {
    offered_requests: u64,
    inter_arrival_pause_every: u64,
    inter_arrival_delay: Duration,
    pre_lock_stage_delay: Duration,
    critical_section_delay: Duration,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                offered_requests: 220,
                inter_arrival_pause_every: 6,
                inter_arrival_delay: Duration::from_millis(1),
                pre_lock_stage_delay: Duration::from_millis(1),
                critical_section_delay: Duration::from_millis(22),
            },
            DemoMode::Mitigated => Self {
                offered_requests: 220,
                inter_arrival_pause_every: 3,
                inter_arrival_delay: Duration::from_millis(1),
                pre_lock_stage_delay: Duration::from_millis(1),
                critical_section_delay: Duration::from_millis(7),
            },
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args =
        parse_demo_args("demos/shared_state_lock_service/artifacts/shared-state-lock-run.json")?;
    let settings = ModeSettings::for_mode(args.mode);

    let instrumentation = Arc::new(DemoInstrumentation::new(
        "shared_state_lock_service_demo",
        &args.output_path,
        args.instrumentation,
    )?);

    let shared_state = Arc::new(RwLock::new(0_u64));
    let waiting_writers = Arc::new(AtomicU64::new(0));

    let capacity = usize::try_from(settings.offered_requests)?;
    let mut tasks = Vec::with_capacity(capacity);

    for request_number in 0..settings.offered_requests {
        let instrumentation = Arc::clone(&instrumentation);
        let shared_state = Arc::clone(&shared_state);
        let waiting_writers = Arc::clone(&waiting_writers);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            instrumentation
                .run_request(
                    "/shared-state-lock-demo",
                    request_id,
                    tailtriage_core::Outcome::Ok,
                    |request| async move {
                        let _inflight = request.inflight("shared_state_lock_inflight");
                        request
                            .stage(
                                "pre_lock_work",
                                tokio::time::sleep(settings.pre_lock_stage_delay),
                            )
                            .await;
                        let waiting_depth = waiting_writers.fetch_add(1, Ordering::SeqCst) + 1;
                        let guard = request
                            .queue_wait(
                                "shared_state_write_lock",
                                waiting_depth,
                                shared_state.write(),
                            )
                            .await;
                        waiting_writers.fetch_sub(1, Ordering::SeqCst);

                        let mut guard = guard;
                        request
                            .stage("shared_state_critical_section", async {
                                *guard += 1;
                                tokio::time::sleep(settings.critical_section_delay).await;
                            })
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
        .expect("no outstanding instrumentation references")
        .shutdown(&args.output_path)?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
