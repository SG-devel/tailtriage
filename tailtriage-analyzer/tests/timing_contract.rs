use tailtriage_analyzer::{analyze_run, AnalyzeOptions};
use tailtriage_core::{
    CaptureMode, RequestEvent, Run, RunMetadata, TruncationSummary, SCHEMA_VERSION,
};

#[test]
fn latency_percentiles_use_duration_fields_not_timestamp_subtraction() {
    let run = Run {
        schema_version: SCHEMA_VERSION,
        metadata: RunMetadata {
            run_id: "timing-contract-run".to_string(),
            service_name: "svc".to_string(),
            service_version: None,
            started_at_unix_ms: 1_000,
            finished_at_unix_ms: 1_001,
            finalized_at_unix_ms: Some(1_001),
            mode: CaptureMode::Light,
            effective_core_config: Some(tailtriage_core::EffectiveCoreConfig {
                mode: CaptureMode::Light,
                capture_limits: CaptureMode::Light.core_defaults(),
                strict_lifecycle: false,
            }),
            effective_tokio_sampler_config: None,
            host: None,
            pid: None,
            lifecycle_warnings: Vec::new(),
            unfinished_requests: tailtriage_core::UnfinishedRequests::default(),
            run_end_reason: None,
        },
        requests: vec![RequestEvent {
            request_id: "req-1".to_string(),
            route: "/checkout".to_string(),
            kind: None,
            started_at_unix_ms: 1_000,
            finished_at_unix_ms: 1_001,
            latency_us: 50_000,
            outcome: "ok".to_string(),
        }],
        stages: Vec::new(),
        queues: Vec::new(),
        inflight: Vec::new(),
        runtime_snapshots: Vec::new(),
        truncation: TruncationSummary::default(),
    };

    let report = analyze_run(&run, AnalyzeOptions::default());

    assert_eq!(report.p50_latency_us, Some(50_000));
    assert_eq!(report.p95_latency_us, Some(50_000));
    assert_eq!(report.p99_latency_us, Some(50_000));
}
