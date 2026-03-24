use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, CohortStart, DemoMode};
use tailtriage_core::{unix_time_ms, RuntimeSnapshot};

struct ModeSettings {
    worker_threads: usize,
    offered_requests: u64,
    fanout_tasks: usize,
    cpu_turns: usize,
    warmup_requests: u64,
    snapshot_depth_scale: u64,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                worker_threads: 2,
                offered_requests: 240,
                fanout_tasks: 22,
                cpu_turns: 260,
                warmup_requests: 20,
                snapshot_depth_scale: 8,
            },
            DemoMode::Mitigated => Self {
                worker_threads: 2,
                offered_requests: 240,
                fanout_tasks: 10,
                cpu_turns: 120,
                warmup_requests: 20,
                snapshot_depth_scale: 3,
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
    let measured_requests = settings.offered_requests;
    let total_requests = measured_requests + settings.warmup_requests;
    let start_gate = CohortStart::new(total_requests as usize);

    let runnable_backlog = Arc::new(AtomicU64::new(0));
    let hot_slice_local_depth = Arc::new(AtomicU64::new(0));
    let capture_done = Arc::new(std::sync::atomic::AtomicBool::new(false));

    let sampler = {
        let tailtriage = Arc::clone(&tailtriage);
        let runnable_backlog = Arc::clone(&runnable_backlog);
        let hot_slice_local_depth = Arc::clone(&hot_slice_local_depth);
        let capture_done = Arc::clone(&capture_done);
        let snapshot_depth_scale = settings.snapshot_depth_scale;

        tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_millis(1));
            let mut captured = 0_u64;
            while !capture_done.load(Ordering::SeqCst) || captured < 50 {
                ticker.tick().await;
                captured = captured.saturating_add(1);

                let global_depth = runnable_backlog.load(Ordering::SeqCst);
                let local_depth = hot_slice_local_depth.load(Ordering::SeqCst);
                let amplified_global_depth = global_depth.saturating_mul(snapshot_depth_scale);
                let amplified_local_depth = local_depth.saturating_mul(snapshot_depth_scale);
                tailtriage.record_runtime_snapshot(RuntimeSnapshot {
                    at_unix_ms: unix_time_ms(),
                    alive_tasks: Some(amplified_global_depth),
                    global_queue_depth: Some(amplified_global_depth),
                    local_queue_depth: Some(amplified_local_depth),
                    blocking_queue_depth: Some(0),
                    remote_schedule_count: Some(amplified_local_depth),
                });
            }
        })
    };

    let mut requests = Vec::with_capacity(total_requests as usize);

    for request_number in 0..total_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let runnable_backlog = Arc::clone(&runnable_backlog);
        let hot_slice_local_depth = Arc::clone(&hot_slice_local_depth);
        let start_gate = start_gate.clone();

        requests.push(tokio::spawn(async move {
            start_gate.wait().await;
            let request_id = format!("request-{request_number}");
            let request = tailtriage.request_with(
                "/executor-pressure",
                tailtriage_core::RequestOptions::new().request_id(request_id.clone()),
            );

            {
                let _inflight = request.inflight("executor_pressure_inflight");
                runnable_backlog.fetch_add(1, Ordering::SeqCst);

                let mut subtasks = Vec::with_capacity(settings.fanout_tasks);
                for _ in 0..settings.fanout_tasks {
                    let local_depth = Arc::clone(&hot_slice_local_depth);
                    let cpu_turns = settings.cpu_turns;
                    subtasks.push(tokio::spawn(async move {
                        local_depth.fetch_add(1, Ordering::SeqCst);
                        for turn in 0..cpu_turns {
                            let mut spin = 0_u64;
                            for _ in 0..6_400 {
                                spin = spin.wrapping_add(1);
                            }
                            if spin == 0 {
                                tokio::task::yield_now().await;
                            }
                            if turn.is_multiple_of(24) {
                                tokio::task::yield_now().await;
                            }
                        }
                        local_depth.fetch_sub(1, Ordering::SeqCst);
                    }));
                }

                for subtask in subtasks {
                    subtask.await.expect("subtask should finish");
                }

                tokio::time::sleep(Duration::from_micros(250)).await;

                runnable_backlog.fetch_sub(1, Ordering::SeqCst);
            }
            request.finish(tailtriage_core::Outcome::Ok);
        }));
    }

    for request in requests {
        request.await.context("request task panicked")?;
    }

    capture_done.store(true, Ordering::SeqCst);
    sampler.await.context("sampler task panicked")?;

    tailtriage.shutdown()?;
    println!("wrote {}", output_path.display());
    Ok(())
}
