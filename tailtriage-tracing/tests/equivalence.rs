mod support;

use support::equivalence_harness::{
    equivalence_report, normalize_run, run_native_scenario, run_tracing_scenario,
};

#[test]
fn native_and_tracing_artifacts_are_equivalent_for_canonical_scenario() {
    let native = futures_executor::block_on(run_native_scenario());
    let tracing = futures_executor::block_on(run_tracing_scenario());

    assert!(native.runtime_snapshots.is_empty());
    assert!(tracing.runtime_snapshots.is_empty());

    let report = equivalence_report(&native, &tracing);
    assert!(report.is_equivalent, "{}", report.failure_message());
}

#[test]
fn equivalence_report_failure_message_is_actionable() {
    let native = futures_executor::block_on(run_native_scenario());
    let mut tracing = futures_executor::block_on(run_tracing_scenario());

    tracing.queues.clear();

    let report = equivalence_report(&native, &tracing);
    assert!(!report.is_equivalent);
    let message = report.failure_message();
    assert!(message.contains("queue name set mismatch"));
    assert!(
        message.contains("correlation shape mismatch")
            || message.contains("request/stage/queue correlation shape mismatch")
    );
}

#[test]
fn normalization_does_not_mutate_original_runs() {
    let native = futures_executor::block_on(run_native_scenario());
    let before = native.clone();

    let _normalized = normalize_run(&native);

    assert_eq!(native, before);
}
