use std::path::PathBuf;

use tailtriage_cli::analyze::analyze_run;
use tailtriage_core::{Run, Tailtriage};

fn unique_path(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tailtriage-{name}-{nanos}.json"))
}

#[tokio::test(flavor = "current_thread")]
async fn queue_and_stage_data_drives_ranked_suspects() {
    let artifact = unique_path("e2e-queue");
    let tailtriage = Tailtriage::builder("e2e-queue")
        .output(&artifact)
        .build()
        .expect("build should succeed");

    for index in 0..30 {
        let request_id = format!("req-{index}");
        let request = tailtriage
            .request_with(
                "/checkout",
                tailtriage_core::RequestOptions::new().request_id(request_id),
            )
            .with_kind("http");

        request
            .queue("ingress")
            .with_depth_at_start(12)
            .await_on(tokio::time::sleep(std::time::Duration::from_millis(4)))
            .await;
        request
            .stage("local_work")
            .await_value(tokio::time::sleep(std::time::Duration::from_millis(1)))
            .await;
        request.complete(tailtriage_core::Outcome::Ok);
    }

    tailtriage.shutdown().expect("shutdown should succeed");

    let run = load_run(&artifact);
    let report = analyze_run(&run);
    assert_eq!(
        report.primary_suspect.kind.as_str(),
        "application_queue_saturation"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn downstream_heavy_stage_is_ranked() {
    let artifact = unique_path("e2e-downstream");
    let tailtriage = Tailtriage::builder("e2e-downstream")
        .output(&artifact)
        .build()
        .expect("build should succeed");

    let request = tailtriage
        .request_with(
            "/invoice",
            tailtriage_core::RequestOptions::new().request_id("req-1"),
        )
        .with_kind("http");
    request
        .stage("downstream_db")
        .await_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(40)).await;
            Ok::<(), &'static str>(())
        })
        .await
        .expect("stage should succeed");
    request.complete(tailtriage_core::Outcome::Ok);

    tailtriage.shutdown().expect("shutdown should succeed");

    let run = load_run(&artifact);
    let report = analyze_run(&run);
    assert_eq!(
        report.primary_suspect.kind.as_str(),
        "insufficient_evidence"
    );
}

#[tokio::test(flavor = "current_thread")]
async fn low_evidence_run_yields_insufficient_signal() {
    let artifact = unique_path("e2e-insufficient");
    let tailtriage = Tailtriage::builder("e2e-insufficient")
        .output(&artifact)
        .build()
        .expect("build should succeed");

    for index in 0..3 {
        let request = tailtriage.request_with(
            "/health",
            tailtriage_core::RequestOptions::new().request_id(format!("insufficient-{index}")),
        );
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
        request.complete(tailtriage_core::Outcome::Ok);
    }

    tailtriage.shutdown().expect("shutdown should succeed");

    let run = load_run(&artifact);
    let report = analyze_run(&run);
    assert_eq!(
        report.primary_suspect.kind.as_str(),
        "insufficient_evidence"
    );
}

fn load_run(path: &std::path::Path) -> Run {
    let bytes = std::fs::read(path).expect("artifact should exist");
    serde_json::from_slice(&bytes).expect("artifact should deserialize")
}
