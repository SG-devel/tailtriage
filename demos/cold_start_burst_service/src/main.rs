use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use tailtriage_core::{Config, RequestMeta, Tailtriage};
use tokio::sync::Semaphore;

#[derive(Clone, Copy)]
enum DemoMode {
    Baseline,
    Mitigated,
}

impl DemoMode {
    fn from_arg(value: Option<String>) -> anyhow::Result<Self> {
        match value.as_deref() {
            None | Some("baseline") | Some("before") => Ok(Self::Baseline),
            Some("mitigated") | Some("after") => Ok(Self::Mitigated),
            Some(other) => anyhow::bail!(
                "unsupported mode '{other}', expected one of: baseline, before, mitigated, after"
            ),
        }
    }
}

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
    let mut args = std::env::args().skip(1);
    let output_path = args.next().map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from("demos/cold_start_burst_service/artifacts/cold-start-burst-run.json")
    });
    let mode = DemoMode::from_arg(args.next())?;
    let settings = ModeSettings::for_mode(mode);

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create artifact directory {}", parent.display()))?;
    }

    let mut config = Config::new("cold_start_burst_service_demo");
    config.output_path = output_path.clone();
    let tailtriage = Arc::new(Tailtriage::init(config)?);

    let semaphore = Arc::new(Semaphore::new(settings.service_capacity));
    let waiting_depth = Arc::new(AtomicU64::new(0));

    let mut tasks = Vec::with_capacity(settings.offered_requests as usize);

    for request_number in 0..settings.offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let semaphore = Arc::clone(&semaphore);
        let waiting_depth = Arc::clone(&waiting_depth);
        let stage_delay = settings.stage_delay_for(request_number);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let meta = RequestMeta::new(request_id.clone(), "/cold-start-burst-demo");

            tailtriage
                .request(meta, "ok", async {
                    let _inflight = tailtriage.inflight("cold_start_burst_inflight");

                    let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
                    let permit = tailtriage
                        .queue(request_id.clone(), "worker_admission")
                        .with_depth_at_start(depth)
                        .await_on(semaphore.acquire())
                        .await
                        .expect("semaphore should remain open");
                    waiting_depth.fetch_sub(1, Ordering::SeqCst);

                    let _permit = permit;

                    tailtriage
                        .stage(request_id, "cold_start_stage")
                        .await_value(tokio::time::sleep(stage_delay))
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
    println!("wrote {}", output_path.display());

    Ok(())
}
