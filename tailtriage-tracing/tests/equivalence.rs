mod support;

use support::equivalence_harness::{
    assert_native_and_tracing_full_parity, build_parity_report, normalize_rendered_report,
    tracing_run_with_queue_name,
};
use tailtriage_core::{MemorySink, Tailtriage};

#[test]
fn native_and_tracing_runs_have_semantic_parity() {
    assert_native_and_tracing_full_parity();
}

#[test]
fn parity_reports_queue_mismatch_when_tracing_queue_changes() {
    let sink = MemorySink::default();
    let tt = Tailtriage::builder("svc")
        .sink(sink.clone())
        .build()
        .expect("tailtriage should build");
    // same canonical scenario as harness native path
    for (id, slow) in [("r1", false), ("r2", true), ("r3", false)] {
        let started = tt.begin_request_with(
            "/checkout",
            tailtriage_core::RequestOptions::new().request_id(id),
        );
        futures_executor::block_on(
            started
                .handle
                .queue("permits")
                .with_depth_at_start(3)
                .await_on(async {
                    std::thread::sleep(std::time::Duration::from_millis(if slow { 4 } else { 1 }));
                }),
        );
        futures_executor::block_on(started.handle.stage("db").await_on(async {
            std::thread::sleep(std::time::Duration::from_millis(if slow { 6 } else { 2 }));
            Ok::<(), std::io::Error>(())
        }))
        .expect("db stage should succeed");
        futures_executor::block_on(started.handle.stage("cache").await_on(async {
            std::thread::sleep(std::time::Duration::from_millis(1));
            Ok::<(), std::io::Error>(())
        }))
        .expect("cache stage should succeed");
        started.completion.finish_ok();
    }
    tt.shutdown().expect("shutdown should succeed");
    let native_run = sink.last_run().expect("native run exists");

    let (tracing_run, warnings) = tracing_run_with_queue_name("permits_changed");
    assert!(warnings.is_empty(), "warnings should be empty");

    let parity_report = build_parity_report(&native_run, &tracing_run);
    assert!(
        parity_report
            .run_report
            .mismatches
            .iter()
            .any(|m| m.contains("queue set mismatch")),
        "expected explicit queue mismatch in run report: {:?}",
        parity_report.run_report.mismatches
    );
}

#[test]
fn normalization_removes_unstable_values_but_keeps_semantic_diff() {
    let native = "Run: abc123\nGenerated: 2026-05-17T12:00:00Z\nPrimary suspect: application_queue_saturation\np95 latency: 2450µs";
    let tracing = "Run: def456\nGenerated: 2026-05-17T12:00:01Z\nPrimary suspect: downstream_stage_dominates\np95 latency: 1900µs";
    let n = normalize_rendered_report(native);
    let t = normalize_rendered_report(tracing);
    assert_ne!(
        n, t,
        "semantic difference should remain after normalization"
    );
    assert!(n.contains("Run: <normalized>"));
    assert!(t.contains("Generated: <normalized>"));
}
