use std::time::{SystemTime, UNIX_EPOCH};

use tailtriage_cli::analyze::{analyze_run, DiagnosisKind};
use tailtriage_core::{Outcome, RequestOptions, Run, Tailtriage};
use tailtriage_tokio::instrument_request;

fn temp_artifact_path(prefix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("system time before epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("tailtriage_e2e_{prefix}_{nanos}.json"))
}

#[tokio::test(flavor = "current_thread")]
async fn queue_heavy_direct_capture_flush_and_analysis_reports_queue_suspect() {
    let output_path = temp_artifact_path("queue");
    let tailtriage = Tailtriage::builder("e2e-queue")
        .output(&output_path)
        .build()
        .expect("build should succeed");

    for index in 0..6 {
        let request_id = format!("queue-{index}");
        let req =
            tailtriage.request_with("/checkout", RequestOptions::new().request_id(request_id));
        req.queue("checkout_queue")
            .with_depth_at_start(24)
            .await_on(tokio::time::sleep(std::time::Duration::from_millis(8)))
            .await;
        req.stage("local_work")
            .await_value(tokio::time::sleep(std::time::Duration::from_millis(1)))
            .await;
        req.complete(Outcome::Ok);
    }

    tailtriage.shutdown().expect("shutdown should succeed");

    let run_json = std::fs::read_to_string(&output_path).expect("artifact should be readable");
    let run: Run = serde_json::from_str(&run_json).expect("artifact should parse as Run");
    assert_eq!(run.requests.len(), 6);
    assert_eq!(run.queues.len(), 6);

    let report = analyze_run(&run);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert!(!report.primary_suspect.evidence.is_empty());

    let report_json = serde_json::to_value(&report).expect("report should serialize");
    assert!(report_json["primary_suspect"]["evidence"].is_array());

    std::fs::remove_file(output_path).expect("temp artifact should be removable");
}

#[instrument_request(
    route = "/checkout",
    kind = "place_order",
    tailtriage = tailtriage,
    request_id = request_id.to_string(),
    skip(tailtriage)
)]
async fn downstream_handler(tailtriage: &Tailtriage, request_id: &str) -> Result<(), ()> {
    tailtriage
        .request_with("/checkout", RequestOptions::new().request_id(request_id))
        .stage("downstream_db")
        .await_on(async {
            tokio::time::sleep(std::time::Duration::from_millis(12)).await;
            Ok::<(), ()>(())
        })
        .await?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn downstream_heavy_macro_capture_flush_and_analysis_reports_stage_suspect() {
    let output_path = temp_artifact_path("downstream");
    let tailtriage = Tailtriage::builder("e2e-downstream")
        .output(&output_path)
        .build()
        .expect("build should succeed");

    for index in 0..5 {
        let request_id = format!("downstream-{index}");
        downstream_handler(&tailtriage, &request_id)
            .await
            .expect("handler should succeed");
    }

    tailtriage.shutdown().expect("shutdown should succeed");

    let run_json = std::fs::read_to_string(&output_path).expect("artifact should be readable");
    let run: Run = serde_json::from_str(&run_json).expect("artifact should parse as Run");
    assert_eq!(run.requests.len(), 5);
    assert_eq!(run.stages.len(), 5);

    let report = analyze_run(&run);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::DownstreamStageDominates
    );
    assert!(!report.primary_suspect.next_checks.is_empty());

    std::fs::remove_file(output_path).expect("temp artifact should be removable");
}

#[tokio::test(flavor = "current_thread")]
async fn request_only_capture_flush_and_analysis_reports_insufficient_evidence() {
    let output_path = temp_artifact_path("insufficient");
    let tailtriage = Tailtriage::builder("e2e-insufficient")
        .output(&output_path)
        .build()
        .expect("build should succeed");

    for index in 0..3 {
        tailtriage
            .request_with(
                "/health",
                RequestOptions::new().request_id(format!("insufficient-{index}")),
            )
            .run(
                Outcome::Ok,
                tokio::time::sleep(std::time::Duration::from_millis(1)),
            )
            .await;
    }

    tailtriage.shutdown().expect("shutdown should succeed");

    let run_json = std::fs::read_to_string(&output_path).expect("artifact should be readable");
    let run: Run = serde_json::from_str(&run_json).expect("artifact should parse as Run");
    assert_eq!(run.requests.len(), 3);
    assert!(run.queues.is_empty());
    assert!(run.stages.is_empty());

    let report = analyze_run(&run);
    assert_eq!(
        report.primary_suspect.kind,
        DiagnosisKind::InsufficientEvidence
    );
    assert!(!report.primary_suspect.evidence.is_empty());

    std::fs::remove_file(output_path).expect("temp artifact should be removable");
}
