use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use demo_support::{init_collector, parse_demo_args, DemoMode};
use tailtriage_core::{RequestMeta, Tailtriage};

#[derive(Clone, Copy)]
struct ModeSettings {
    offered_requests: u64,
    inter_arrival_pause_every: u64,
    inter_arrival_delay: Duration,
    app_precheck_delay: Duration,
    max_retries: u8,
    retry_backoff_base: Duration,
    jitter_divisor: u64,
    breaker_fail_threshold: u8,
    breaker_cooldown: Duration,
}

impl ModeSettings {
    fn for_mode(mode: DemoMode) -> Self {
        match mode {
            DemoMode::Baseline => Self {
                offered_requests: 180,
                inter_arrival_pause_every: 6,
                inter_arrival_delay: Duration::from_millis(1),
                app_precheck_delay: Duration::from_millis(1),
                max_retries: 5,
                retry_backoff_base: Duration::from_millis(1),
                jitter_divisor: 0,
                breaker_fail_threshold: u8::MAX,
                breaker_cooldown: Duration::ZERO,
            },
            DemoMode::Mitigated => Self {
                offered_requests: 180,
                inter_arrival_pause_every: 3,
                inter_arrival_delay: Duration::from_millis(2),
                app_precheck_delay: Duration::from_millis(1),
                max_retries: 1,
                retry_backoff_base: Duration::from_millis(3),
                jitter_divisor: 3,
                breaker_fail_threshold: 1,
                breaker_cooldown: Duration::from_millis(2),
            },
        }
    }
}

#[derive(Clone, Copy)]
enum DownstreamResult {
    Ok,
    Err,
}

fn attempt_stage_name(attempt: u8) -> String {
    format!("downstream_attempt_{}", attempt + 1)
}

fn deterministic_jitter(request_number: u64, attempt: u8, divisor: u64) -> Duration {
    if divisor == 0 {
        return Duration::ZERO;
    }

    let bucket = (request_number + u64::from(attempt)) % divisor;
    Duration::from_millis(bucket)
}

fn downstream_outcome(request_number: u64, attempt: u8) -> (Duration, DownstreamResult) {
    if request_number.is_multiple_of(5) && attempt < 2 {
        return (Duration::from_millis(12), DownstreamResult::Err);
    }

    if request_number.is_multiple_of(7) && attempt == 0 {
        return (Duration::from_millis(26), DownstreamResult::Ok);
    }

    if request_number.is_multiple_of(11) && attempt == 0 {
        return (Duration::from_millis(16), DownstreamResult::Err);
    }

    (Duration::from_millis(6), DownstreamResult::Ok)
}

async fn run_downstream_with_retries(
    tailtriage: Arc<Tailtriage>,
    request_id: String,
    request_number: u64,
    settings: ModeSettings,
) {
    let mut consecutive_failures = 0_u8;

    for attempt in 0..=settings.max_retries {
        let stage = attempt_stage_name(attempt);
        let (latency, outcome) = downstream_outcome(request_number, attempt);

        let succeeded = tailtriage
            .stage(request_id.clone(), stage)
            .await_value(async {
                tokio::time::sleep(latency).await;
                matches!(outcome, DownstreamResult::Ok)
            })
            .await;

        if succeeded {
            return;
        }

        consecutive_failures = consecutive_failures.saturating_add(1);
        if attempt == settings.max_retries {
            return;
        }

        if consecutive_failures >= settings.breaker_fail_threshold {
            tailtriage
                .stage(request_id.clone(), "retry_circuit_open")
                .await_value(tokio::time::sleep(settings.breaker_cooldown))
                .await;
            return;
        }

        let backoff = settings
            .retry_backoff_base
            .saturating_mul(u32::from(attempt) + 1)
            + deterministic_jitter(request_number, attempt, settings.jitter_divisor);

        tailtriage
            .stage(request_id.clone(), "retry_backoff_wait")
            .await_value(tokio::time::sleep(backoff))
            .await;
    }
}

#[tokio::main(flavor = "multi_thread", worker_threads = 2)]
async fn main() -> anyhow::Result<()> {
    let args = parse_demo_args("demos/retry_storm_service/artifacts/retry-storm-run.json")?;
    let mode_settings = ModeSettings::for_mode(args.mode);

    let tailtriage = init_collector("retry_storm_service_demo", &args.output_path)?;

    let mut tasks = Vec::with_capacity(mode_settings.offered_requests as usize);

    for request_number in 0..mode_settings.offered_requests {
        let tailtriage = Arc::clone(&tailtriage);
        let settings = mode_settings;

        tasks.push(tokio::spawn(async move {
            let request_id = format!("request-{request_number}");
            let meta = RequestMeta::new(request_id.clone(), "/retry-storm-demo");

            tailtriage
                .request_with_meta(meta, "ok", async {
                    let _inflight = tailtriage.inflight("retry_storm_inflight");

                    tailtriage
                        .stage(request_id.clone(), "app_precheck")
                        .await_value(tokio::time::sleep(settings.app_precheck_delay))
                        .await;

                    tailtriage
                        .stage(request_id.clone(), "downstream_total")
                        .await_value(run_downstream_with_retries(
                            Arc::clone(&tailtriage),
                            request_id,
                            request_number,
                            settings,
                        ))
                        .await;
                })
                .await;
        }));

        if request_number % mode_settings.inter_arrival_pause_every == 0 {
            tokio::time::sleep(mode_settings.inter_arrival_delay).await;
        }
    }

    for task in tasks {
        task.await.context("request task panicked")?;
    }

    tailtriage.flush()?;
    println!("wrote {}", args.output_path.display());

    Ok(())
}
