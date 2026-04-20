use tailtriage_controller::TailtriageController;
use tailtriage_core::CaptureLimitsOverride;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let artifact_template = "tailtriage-run.json";
    let controller = TailtriageController::builder("controller-minimal")
        .output(artifact_template)
        .capture_limits_override(CaptureLimitsOverride {
            max_requests: Some(8),
            ..CaptureLimitsOverride::default()
        })
        .build()?;

    controller.enable()?;
    let started = controller.begin_request("/checkout");
    started.completion.finish_ok();
    let _disable = controller.disable()?;

    println!("Wrote tailtriage-run-generation-1.json");
    println!("This example records exactly one request before disable().");
    Ok(())
}
