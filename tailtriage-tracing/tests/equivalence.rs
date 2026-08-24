#[path = "support/live_harness.rs"]
mod live_harness;

use live_harness::assert_deterministic_span_import_full_parity;

// TT-TEST: F03 secondary
#[test]
fn deterministic_span_import_remains_a_compact_live_harness_smoke_test() {
    assert_deterministic_span_import_full_parity();
}
