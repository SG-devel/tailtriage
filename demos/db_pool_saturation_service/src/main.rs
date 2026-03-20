use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};
use tailtriage_core::RequestMeta;
use tokio::sync::Semaphore;

struct ModeSettings {
    db_pool_size: usize,
    offered_requests: u64,
    inter_arrival_pause_every: u64,
    inter_arrival_delay: Duration,
    app_precheck_delay: Duration,
    db_query_delay: Duration,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                db_pool_size: 4,
                offered_requests: 220,
                inter_arrival_pause_every: 5,
                inter_arrival_delay: Duration::from_millis(1),
                app_precheck_delay: Duration::from_millis(1),
                db_query_delay: Duration::from_millis(18),
            },
            DemoMode::Mitigated => Self {
                db_pool_size: 12,
                offered_requests: 220,
                inter_arrival_pause_every: 2,
                inter_arrival_delay: Duration::from_millis(2),
                app_precheck_delay: Duration::from_millis(1),
                db_query_delay: Duration::from_millis(10),
            },
        }
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args =
        parse_demo_args("demos/db_pool_saturation_service/artifacts/db-pool-saturation-run.json")?;
    let settings = ModeSettings::for_mode(args.mode);

    let tailtriage = init_collector("db_pool_saturation_service_demo", &args.output_path)?;

    let db_pool = Arc::new(Semaphore::new(settings.db_pool_size));
    let waiting_depth = Arc::new(AtomicU64::new(0));

    let mut tasks = Vec::with_capacity(settings.offered_requests as usize);

    for request_number in 0..settings.offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let db_pool = Arc::clone(&db_pool);
        let waiting_depth = Arc::clone(&waiting_depth);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let meta = RequestMeta::new(request_id.clone(), "/db-pool-saturation-demo");

            tailtriage
                .request(meta, "ok", async {
                    let _inflight = tailtriage.inflight("db_pool_saturation_inflight");

                    tailtriage
                        .stage(request_id.clone(), "app_precheck")
                        .await_value(tokio::time::sleep(settings.app_precheck_delay))
                        .await;

                    let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
                    let permit = tailtriage
                        .queue(request_id.clone(), "db_pool")
                        .with_depth_at_start(depth)
                        .await_on(db_pool.acquire())
                        .await
                        .expect("db pool semaphore should remain open");
                    waiting_depth.fetch_sub(1, Ordering::SeqCst);

                    let _permit = permit;

                    tailtriage
                        .stage(request_id, "db_query")
                        .await_value(tokio::time::sleep(settings.db_query_delay))
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
