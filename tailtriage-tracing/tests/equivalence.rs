mod support;

use support::equivalence_harness::assert_native_and_tracing_full_parity;

#[test]
fn native_and_tracing_runs_have_full_parity() {
    assert_native_and_tracing_full_parity();
}
