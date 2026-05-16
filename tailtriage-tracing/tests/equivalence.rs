mod support;

use support::equivalence_harness::{assert_semantic_equivalence, build_equivalence_report};

#[test]
fn native_and_tracing_paths_are_semantically_equivalent() {
    let report = build_equivalence_report().expect("equivalence report should build");
    assert_semantic_equivalence(&report);
}
