#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! `tailtriage-cli` is the command-line artifact loader and report emitter.
//! For in-process Rust analysis/report APIs, use `tailtriage-analyzer`.

use std::fmt::Write as _;
use std::path::Path;

use tailtriage_analyzer::{analyze_option_descriptors, AnalyzeConfigError, AnalyzeOptions};

/// Artifact loading and validation helpers for CLI workflows.
pub mod artifact;

/// Builds analyzer options from defaults, optional TOML config path, and ordered CLI overrides.
///
/// Precedence order is:
/// built-in defaults < analyzer TOML < ordered `overrides`.
///
/// # Errors
/// Returns an [`AnalyzeConfigError`] for TOML parse/schema/semantic errors or override errors.
/// Returns an I/O error wrapped as [`AnalyzeConfigError::InvalidConfigValue`] when the config file
/// cannot be read.
pub fn build_analyze_options(
    analyzer_config: Option<&Path>,
    overrides: &[String],
) -> Result<AnalyzeOptions, AnalyzeConfigError> {
    let mut options = AnalyzeOptions::default();

    if let Some(path) = analyzer_config {
        let input = std::fs::read_to_string(path).map_err(|error| {
            AnalyzeConfigError::InvalidConfigValue {
                path: "analyzer.config_path",
                message: format!("failed to read {}: {error}", path.display()),
            }
        })?;
        options = options.merge_toml_str(&input)?;
    }

    options.apply_overrides(overrides.iter().map(String::as_str))?;
    Ok(options)
}

/// Returns deterministic help text for analyzer option paths exposed by `tailtriage-analyzer`.
#[must_use]
pub fn analyzer_options_help_text() -> String {
    let mut out = String::from("Analyzer options (paths for --analyzer-set PATH=VALUE):\n\n");
    for descriptor in analyze_option_descriptors() {
        let _ = writeln!(
            out,
            "- {}\n  default: {}\n  type: {}\n  affects: {}\n  description: {}",
            descriptor.path,
            descriptor.default_value,
            descriptor.value_type,
            descriptor.affects,
            descriptor.description,
        );
        if let Some(note) = descriptor.increasing {
            let _ = writeln!(out, "  increasing: {note}");
        }
        if let Some(note) = descriptor.decreasing {
            let _ = writeln!(out, "  decreasing: {note}");
        }
        out.push('\n');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config_toml(trigger_permille: u64) -> String {
        format!(
            "[analyzer]\nschema_version=1\n[analyzer.queueing]\ntrigger_permille={trigger_permille}\n"
        )
    }

    #[test]
    fn default_options_without_config_or_overrides() {
        let built = build_analyze_options(None, &[]).expect("build should succeed");
        assert_eq!(built, AnalyzeOptions::default());
    }

    #[test]
    fn config_toml_applies() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("analyzer.toml");
        std::fs::write(&path, config_toml(410)).expect("write config");

        let built = build_analyze_options(Some(&path), &[]).expect("build should succeed");
        assert_eq!(built.queueing.trigger_permille, 410);
    }

    #[test]
    fn override_applies_and_beats_toml_last_wins() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("analyzer.toml");
        std::fs::write(&path, config_toml(410)).expect("write config");

        let overrides = vec![
            "queueing.trigger_permille=420".to_string(),
            "queueing.trigger_permille=430".to_string(),
        ];
        let built = build_analyze_options(Some(&path), &overrides).expect("build should succeed");
        assert_eq!(built.queueing.trigger_permille, 430);
    }

    #[test]
    fn misspelled_path_reports_suggestion() {
        let err = build_analyze_options(None, &["queuing.trigger_permille=400".to_string()])
            .expect_err("expected error");
        let msg = err.to_string();
        assert!(msg.contains("queueing.trigger_permille"));
    }

    #[test]
    fn invalid_type_reports_expected_type() {
        let err = build_analyze_options(None, &["queueing.trigger_permille=abc".to_string()])
            .expect_err("expected error");
        let msg = err.to_string();
        assert!(msg.contains("u64"));
    }
}
