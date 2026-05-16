mod support;

#[test]
fn native_and_tracing_semantics_are_equivalent() {
    let report = support::equivalence_harness::run_equivalence();
    assert!(
        report.details.is_empty(),
        "semantic parity mismatches: {}",
        report.details.join("; ")
    );
}
