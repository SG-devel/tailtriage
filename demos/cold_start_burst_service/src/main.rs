use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};
use tokio::sync::Semaphore;

struct ModeSettings {
    service_capacity: usize,
    offered_requests: u64,
    cold_start_cohort: u64,
    warmup_extra_delay: Duration,
    steady_stage_delay: Duration,
    inter_arrival_pause_every: u64,
    inter_arrival_delay: Duration,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                service_capacity: 4,
                offered_requests: 220,
                cold_start_cohort: 56,
                warmup_extra_delay: Duration::from_millis(55),
                steady_stage_delay: Duration::from_millis(7),
                inter_arrival_pause_every: 32,
                inter_arrival_delay: Duration::from_millis(1),
            },
            DemoMode::Mitigated => Self {
                service_capacity: 18,
                offered_requests: 220,
                cold_start_cohort: 2,
                warmup_extra_delay: Duration::from_millis(10),
                steady_stage_delay: Duration::from_millis(7),
                inter_arrival_pause_every: 1,
                inter_arrival_delay: Duration::from_millis(3),
            },
        }
    }

    fn stage_delay_for(&self, request_number: u64) -> Duration {
        if request_number < self.cold_start_cohort {
            self.steady_stage_delay + self.warmup_extra_delay
        } else {
            self.steady_stage_delay
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args =
        parse_demo_args("demos/cold_start_burst_service/artifacts/cold-start-burst-run.json")?;
    let settings = ModeSettings::for_mode(args.mode);

    let tailtriage = init_collector("cold_start_burst_service_demo", &args.output_path)?;

    let semaphore = Arc::new(Semaphore::new(settings.service_capacity));
    let waiting_depth = Arc::new(AtomicU64::new(0));

    let mut tasks = Vec::with_capacity(settings.offered_requests as usize);

    for request_number in 0..settings.offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let semaphore = Arc::clone(&semaphore);
        let waiting_depth = Arc::clone(&waiting_depth);
        let stage_delay = settings.stage_delay_for(request_number);

        tasks.push(tokio::spawn(async move {
            let request = tailtriage.request("/cold-start-burst-demo");
            let _inflight = request.inflight("cold_start_burst_inflight");

            let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
            let permit = request
                .queue("worker_admission")
                .with_depth_at_start(depth)
                .await_on(semaphore.acquire())
                .await
                .expect("semaphore should remain open");
            waiting_depth.fetch_sub(1, Ordering::SeqCst);

            let _permit = permit;

            request
                .stage("cold_start_stage")
                .await_value(tokio::time::sleep(stage_delay))
                .await;
            request.complete("ok");
        }));

        if request_number % settings.inter_arrival_pause_every == 0 {
            tokio::time::sleep(settings.inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    tailtriage.shutdown()?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
