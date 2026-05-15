use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = TracingRecorder::builder("checkout-service")
        .run_id("live-recorder-example")
        .strict(false)
        .build();

    let subscriber = tracing_subscriber::registry().with(recorder.layer());
    tracing::subscriber::with_default(subscriber, || {
        let request = tracing::info_span!(
            "http.request",
            tt.kind = "request",
            tt.request_id = "req-1",
            tt.route = "/checkout",
            tt.outcome = "ok"
        );
        let _request_entered = request.enter();

        let stage = tracing::info_span!(
            "db.query",
            tt.kind = "stage",
            tt.request_id = "req-1",
            tt.stage = "db",
            tt.success = true
        );
        drop(stage);
    });

    let imported = recorder.shutdown()?;
    let report = analyze_run(imported.run(), AnalyzeOptions::default());
    println!("{}", render_text(&report));

    Ok(())
}
