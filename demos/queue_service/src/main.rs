use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};
use tailtriage_core::{Outcome, RequestOptions};
use tokio::sync::Semaphore;

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args = parse_demo_args("demos/queue_service/artifacts/queue-run.json")?;

    let tailtriage = init_collector("queue_service_demo", &args.output_path)?;

    let (
        service_capacity,
        offered_requests,
        work_duration,
        inter_arrival_pause_every,
        inter_arrival_delay,
    ) = match args.mode {
        DemoMode::Baseline => (
            4,
            250_u64,
            Duration::from_millis(25),
            5,
            Duration::from_millis(1),
        ),
        DemoMode::Mitigated => (
            12,
            250_u64,
            Duration::from_millis(15),
            2,
            Duration::from_millis(2),
        ),
    };

    let semaphore = Arc::new(Semaphore::new(service_capacity));
    let waiting_depth = Arc::new(AtomicU64::new(0));

    let mut tasks = Vec::with_capacity(offered_requests as usize);

    for request_number in 0..offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let semaphore = Arc::clone(&semaphore);
        let waiting_depth = Arc::clone(&waiting_depth);

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let req = tailtriage
                .request_with("/queue-demo", RequestOptions::new().request_id(request_id));
            let _inflight = req.inflight("queue_service_inflight");

            let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
            let permit = req
                .queue("worker_permit")
                .with_depth_at_start(depth)
                .await_on(semaphore.acquire())
                .await
                .expect("semaphore should remain open");
            waiting_depth.fetch_sub(1, Ordering::SeqCst);

            let _permit = permit;
            req.stage("simulated_work")
                .await_value(tokio::time::sleep(work_duration))
                .await;
            drop(_inflight);
            req.complete(Outcome::Ok);
        }));

        if request_number % inter_arrival_pause_every == 0 {
            tokio::time::sleep(inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    tailtriage.shutdown()?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
