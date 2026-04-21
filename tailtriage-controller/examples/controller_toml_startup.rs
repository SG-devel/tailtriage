use std::fs;
use std::path::PathBuf;

use tailtriage_controller::TailtriageController;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let config_path = PathBuf::from("tailtriage-controller.toml");
    let config = r#"[controller]
service_name = "controller-toml-startup"
initially_enabled = true

[controller.activation]
mode = "light"

[controller.activation.sink]
type = "local_json"
output_path = "tailtriage-run.json"
"#;
    fs::write(&config_path, config)?;

    let controller = TailtriageController::builder("controller-toml-startup-builder")
        .config_path(&config_path)
        .build()?;

    let started = controller.begin_request("/checkout");
    started.completion.finish_ok();
    let _disable = controller.disable()?;

    let artifact = PathBuf::from("tailtriage-run-generation-1.json");
    println!("Wrote {}", artifact.display());
    fs::remove_file(config_path)?;
    Ok(())
}
