use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_demo_recorder, parse_demo_args, DemoMode};

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args = parse_demo_args("demos/queue_service/artifacts/queue-run.json")?;
    let output_path = args.output_path.clone();
    let recorder = Arc::new(init_demo_recorder(
        "queue_service_demo",
        args.instrumentation,
        &output_path,
    )?);

    let (service_capacity, offered_requests, work_duration, pause_every, inter_arrival_delay) =
        match args.mode {
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

    let semaphore = Arc::new(tokio::sync::Semaphore::new(service_capacity));
    let waiting_depth = Arc::new(AtomicU64::new(0));
    let mut tasks = Vec::with_capacity(usize::try_from(offered_requests)?);

    for request_number in 0..offered_requests {
        let recorder = Arc::clone(&recorder);
        let semaphore = Arc::clone(&semaphore);
        let waiting_depth = Arc::clone(&waiting_depth);
        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let request = recorder.start_request("/queue-demo", &request_id);

            let depth = waiting_depth.fetch_add(1, Ordering::SeqCst) + 1;
            let permit = request
                .record_queue("worker_permit", depth, semaphore.acquire())
                .await
                .expect("semaphore should remain open");
            waiting_depth.fetch_sub(1, Ordering::SeqCst);

            let _permit = permit;
            request
                .record_stage("simulated_work", true, tokio::time::sleep(work_duration))
                .await;
            request.finish(tailtriage_core::Outcome::Ok);
        }));

        if request_number % pause_every == 0 {
            tokio::time::sleep(inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    Arc::into_inner(recorder)
        .expect("single recorder owner at shutdown")
        .shutdown(&output_path)?;
    println!("wrote {}", output_path.display());
    Ok(())
}
