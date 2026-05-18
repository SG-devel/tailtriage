mod support;

use support::equivalence_harness::assert_deterministic_full_parity;

#[test]
fn deterministic_span_import_matches_native_run_analysis_and_rendering() {
    assert_deterministic_full_parity();
}
