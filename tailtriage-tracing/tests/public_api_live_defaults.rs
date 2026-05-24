#![cfg(feature = "live")]

#[test]
fn crate_root_reexports_live_recorder_defaults_consistently() {
    let limits = tailtriage_tracing::RecorderLimits::default();
    assert_eq!(
        limits.max_open_spans,
        tailtriage_tracing::DEFAULT_MAX_OPEN_SPANS
    );
    assert_eq!(
        limits.max_completed_candidate_spans,
        tailtriage_tracing::DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS
    );
}
