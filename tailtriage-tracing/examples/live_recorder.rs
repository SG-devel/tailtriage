use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_tracing::TracingRecorder;
use tracing_subscriber::prelude::*;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let recorder = TracingRecorder::builder("checkout-service")
        .service_version("1.0.0")
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
            tt.outcome = tracing::field::Empty,
        );
        let _request_guard = request.enter();

        let stage = tracing::info_span!(
            "stage.db",
            tt.kind = "stage",
            tt.request_id = "req-1",
            tt.stage = "db",
            tt.success = true,
        );
        {
            let _stage_guard = stage.enter();
        }

        request.record("tt.outcome", "ok");
    });

    let imported = recorder.shutdown()?;
    let report = analyze_run(imported.run(), AnalyzeOptions::default());
    println!("{}", render_text(&report));
    Ok(())
}
