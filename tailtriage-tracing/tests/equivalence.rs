mod support;

use support::equivalence_harness::{
    build_parity_report, deterministic_native_run, deterministic_tracing_run,
};
use tailtriage_analyzer::{analyze_run, AnalyzeOptions, DiagnosisKind};

#[test]
fn deterministic_span_import_matches_native_run_analysis_and_rendering() {
    let native = deterministic_native_run();
    let (tracing, warnings) = deterministic_tracing_run();
    assert!(warnings.is_empty(), "unexpected warnings: {warnings:?}");

    let native_report = analyze_run(&native, AnalyzeOptions::default());
    let tracing_report = analyze_run(&tracing, AnalyzeOptions::default());
    assert_eq!(
        native_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );
    assert_eq!(
        tracing_report.primary_suspect.kind,
        DiagnosisKind::ApplicationQueueSaturation
    );

    let parity = build_parity_report(&native, &tracing);
    assert!(
        parity.mismatches.is_empty(),
        "deterministic parity mismatches: {:?}",
        parity.mismatches
    );
}
