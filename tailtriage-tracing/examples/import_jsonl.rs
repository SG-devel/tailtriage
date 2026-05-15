use std::error::Error;

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::{import_jsonl_path, ImportOptions};

fn main() -> Result<(), Box<dyn Error>> {
    let imported = import_jsonl_path(
        "examples/tracing_spans.jsonl",
        ImportOptions::new("checkout-service")
            .service_version("example")
            .run_id("jsonl-example")
            .strict(false),
    )?;

    let report = analyze_run(imported.run(), AnalyzeOptions::default());
    println!("{}", render_text(&report));

    Ok(())
}
