mod support;

use support::equivalence_harness::assert_native_and_tracing_full_parity;
use tailtriage_analyzer::DiagnosisKind;

#[test]
fn deterministic_span_import_matches_native_run_analysis_and_rendering() {
    assert_native_and_tracing_full_parity(DiagnosisKind::ApplicationQueueSaturation);
}
