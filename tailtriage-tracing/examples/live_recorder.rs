use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = TracingRecorder::builder("checkout-service")
        .service_version("0.1.0")
        .run_id("live-recorder-example")
        .strict(true)
        .build();

    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request_span = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-100",
            tt.route = "/checkout"
        );

        let request_guard = request_span.enter();
        {
            let stage_span = tracing::info_span!(
                "checkout.db",
                tt.kind = "stage",
                tt.request_id = "req-100",
                tt.stage = "db_call",
                tt.success = true
            );
            let _stage_guard = stage_span.enter();
        }
        drop(request_guard);
    });

    let imported = recorder.shutdown()?;
    let report = analyze_run(imported.run(), AnalyzeOptions::default());
    println!("{}", render_text(&report));

    Ok(())
}
