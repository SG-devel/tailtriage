mod support;

use support::equivalence_harness::assert_native_and_tracing_semantic_parity;

#[test]
fn native_and_tracing_runs_have_semantic_parity() {
    assert_native_and_tracing_semantic_parity();
}
