#![cfg(feature = "live")]

#[test]
fn crate_root_exports_live_recorder_default_limits_consistently() {
    let defaults = tailtriage_tracing::RecorderLimits::default();
    assert_eq!(
        defaults.max_open_spans,
        tailtriage_tracing::DEFAULT_MAX_OPEN_SPANS
    );
    assert_eq!(
        defaults.max_completed_candidate_spans,
        tailtriage_tracing::DEFAULT_MAX_COMPLETED_CANDIDATE_SPANS
    );
}
