use std::{error::Error, fs};

use tailtriage_analyzer::{analyze_run, render_text, AnalyzeOptions};
use tailtriage_core::{Outcome, Run, Tailtriage};

struct TracingRecorder {
    run: Tailtriage,
}

impl TracingRecorder {
    fn new() -> Result<Self, Box<dyn Error>> {
        Ok(Self {
            run: Tailtriage::builder("tracing-intake-example")
                .output("tailtriage-run.json")
                .build()?,
        })
    }

    fn shutdown(self) -> Result<Run, Box<dyn Error>> {
        self.run.shutdown()?;
        let payload = fs::read_to_string("tailtriage-run.json")?;
        Ok(serde_json::from_str(&payload)?)
    }
}

fn main() -> Result<(), Box<dyn Error>> {
    let recorder = TracingRecorder::new()?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;

    runtime.block_on(async {
        let started = recorder.run.begin_request("/live");
        started
            .handle
            .queue("tt.queue.ingress")
            .await_on(async {})
            .await;
        started
            .handle
            .stage("tt.stage.downstream")
            .await_value(async {})
            .await;
        started.completion.finish(Outcome::Ok);
    });

    let run = recorder.shutdown()?;
    let report = analyze_run(&run, AnalyzeOptions::default());
    println!("{}", render_text(&report));
    Ok(())
}
