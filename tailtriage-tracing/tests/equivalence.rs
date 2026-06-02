#![cfg(feature = "live")]

mod support;

use support::equivalence_harness::{
    assert_deterministic_span_import_full_parity,
    assert_single_request_native_and_live_tracing_timing_semantics,
};

#[test]
fn deterministic_span_import_matches_native_run_analysis_and_rendering() {
    assert_deterministic_span_import_full_parity();
}

#[test]
fn native_core_and_live_tracing_single_request_timing_semantics_match() {
    assert_single_request_native_and_live_tracing_timing_semantics();
}
