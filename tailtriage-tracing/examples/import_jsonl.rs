use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::{import_jsonl_path, ImportOptions};

fn main() -> Result<(), tailtriage_tracing::ImportError> {
    let imported = import_jsonl_path(
        "examples/tracing_spans.jsonl",
        ImportOptions::new("checkout-service")
            .service_version("example")
            .run_id("jsonl-import-example")
            .strict(false),
    )?;

    let report = analyze_run(imported.run(), AnalyzeOptions::default());

    println!("=== JSONL import run summary ===");
    println!(
        "requests={} stages={} queues={} warnings={}",
        imported.run().requests.len(),
        imported.run().stages.len(),
        imported.run().queues.len(),
        imported.warnings().len()
    );
    println!("\n=== Analyzer text report ===\n{}", render_text(&report));

    Ok(())
}
