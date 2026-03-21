use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};
use tailtriage_core::{unix_time_ms, RuntimeSnapshot};

struct ModeSettings {
    worker_threads: usize,
    offered_requests: u64,
    fanout_tasks: usize,
    cpu_turns: usize,
    burst_pause_every: u64,
    burst_pause: Duration,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                worker_threads: 2,
                offered_requests: 320,
                fanout_tasks: 18,
                cpu_turns: 220,
                burst_pause_every: 24,
                burst_pause: Duration::from_millis(1),
            },
            DemoMode::Mitigated => Self {
                worker_threads: 6,
                offered_requests: 220,
                fanout_tasks: 10,
                cpu_turns: 120,
                burst_pause_every: 8,
                burst_pause: Duration::from_millis(2),
            },
        }
    }
}

fn main() -> anyhow::Result<()> {
    let args =
        parse_demo_args("demos/executor_pressure_service/artifacts/executor-pressure-run.json")?;
    let settings = ModeSettings::for_mode(args.mode);

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(settings.worker_threads)
        .max_blocking_threads(8)
        .enable_time()
        .build()
        .context("failed to build Tokio runtime")?;

    runtime.block_on(run_demo(args.output_path, settings))
}

async fn run_demo(output_path: PathBuf, settings: ModeSettings) -> anyhow::Result<()> {
    let tailtriage = init_collector("executor_pressure_demo", &output_path)?;

    let runnable_backlog = Arc::new(AtomicU64::new(0));
    let hot_slice_local_depth = Arc::new(AtomicU64::new(0));

    let sampler = {
        let tailtriage = Arc::clone(&tailtriage);
        let runnable_backlog = Arc::clone(&runnable_backlog);
        let hot_slice_local_depth = Arc::clone(&hot_slice_local_depth);

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(5));
            for _ in 0..220 {
                ticker.tick().await;

                let global_depth = runnable_backlog.load(Ordering::SeqCst);
                let local_depth = hot_slice_local_depth.load(Ordering::SeqCst);
                tailtriage.record_runtime_snapshot(RuntimeSnapshot {
                    at_unix_ms: unix_time_ms(),
                    alive_tasks: Some(global_depth),
                    global_queue_depth: Some(global_depth),
                    local_queue_depth: Some(local_depth),
                    blocking_queue_depth: Some(0),
                    remote_schedule_count: None,
                });
            }
        })
    };

    let mut requests = Vec::with_capacity(settings.offered_requests as usize);

    for request_number in 0..settings.offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let runnable_backlog = Arc::clone(&runnable_backlog);
        let hot_slice_local_depth = Arc::clone(&hot_slice_local_depth);

        requests.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let request = tailtriage
                .request("/executor-pressure")
                .request_id(request_id)
                .start();
            let _inflight = request.inflight("executor_pressure_inflight");
            request
                .queue("admission")
                .with_depth_at_start(runnable_backlog.fetch_add(1, Ordering::SeqCst) + 1)
                .await_on(tokio::task::yield_now())
                .await;

            let mut subtasks = Vec::with_capacity(settings.fanout_tasks);
            for _ in 0..settings.fanout_tasks {
                let local_depth = Arc::clone(&hot_slice_local_depth);
                let cpu_turns = settings.cpu_turns;
                subtasks.push(tokio::spawn(async move {
                    for turn in 0..cpu_turns {
                        local_depth.fetch_add(1, Ordering::SeqCst);
                        let mut spin = 0_u64;
                        for _ in 0..1_200 {
                            spin = spin.wrapping_add(1);
                        }
                        if spin == 0 {
                            tokio::task::yield_now().await;
                        }
                        if turn.is_multiple_of(20) {
                            tokio::task::yield_now().await;
                        }
                        local_depth.fetch_sub(1, Ordering::SeqCst);
                    }
                }));
            }

            request
                .stage("executor_hot_path")
                .await_value(async {
                    for subtask in subtasks {
                        subtask.await.expect("subtask should finish");
                    }
                })
                .await;

            runnable_backlog.fetch_sub(1, Ordering::SeqCst);
            drop(_inflight);
            request.finish("ok");
        }));

        if request_number % settings.burst_pause_every == 0 {
            tokio::time::sleep(settings.burst_pause).await;
        }
    }

    for request in requests {
        request.await.context("request task panicked")?;
    }

    sampler.await.context("sampler task panicked")?;

    tailtriage.shutdown()?;
    println!("wrote {}", output_path.display());
    Ok(())
}
