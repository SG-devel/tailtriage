use std::path::{Path, PathBuf};

use tailtriage_core::{
    __internal::escape_control_chars, decode_run_json_path, normalize_run_permissive, Run,
    RunJsonDecodeError, RunValidationReport,
};

fn display_path(path: &Path) -> String {
    escape_control_chars(&path.display().to_string())
}

/// A decoded run artifact plus non-fatal loader warnings.
#[derive(Debug)]
pub(crate) struct LoadedArtifact {
    /// Permissively normalized [`Run`].
    ///
    /// This is suitable for command-level policy checks, such as whether any
    /// request remains after normalization, and for callers that deliberately
    /// want normalized evidence without the original validation findings. It is
    /// not the preferred analyzer input when canonical findings from the
    /// original candidate must remain visible.
    pub run: Run,
    /// Original decoded [`Run`] before permissive normalization.
    ///
    /// This is the required input for strict validation and the preferred
    /// analyzer input when canonical findings from the original candidate must
    /// remain visible.
    pub original_run: Run,
    /// Complete canonical findings from permissive normalization of the original run.
    pub validation_report: RunValidationReport,
    /// Non-fatal loader findings that did not block loading.
    pub warnings: Vec<String>,
}

/// Errors returned when loading and validating run artifacts from disk.
#[derive(Debug)]
pub(crate) enum ArtifactLoadError {
    /// The file could not be read from disk.
    Read {
        /// Path that failed to read.
        path: PathBuf,
        /// Underlying I/O failure.
        source: std::io::Error,
    },
    /// JSON parsing or schema-shape decoding failed.
    Parse {
        /// Path that failed to parse.
        path: PathBuf,
        /// Human-readable parse or decoding error detail.
        message: String,
    },
    /// `schema_version` did not match this binary's supported version.
    UnsupportedSchemaVersion {
        /// Artifact path that contained the unsupported version.
        path: PathBuf,
        /// Found schema version in the artifact.
        found: u64,
        /// Supported schema version expected by this binary.
        supported: u64,
    },
    /// Additional validation rejected the artifact contents.
    Validation {
        /// Artifact path that failed validation.
        path: PathBuf,
        /// Validation failure detail.
        message: String,
    },
}

impl std::fmt::Display for ArtifactLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read run artifact '{}': {source}", display_path(path))
            }
            Self::Parse { path, message } => {
                write!(f, "failed to parse run artifact '{}': {}", display_path(path), escape_control_chars(message))
            }
            Self::UnsupportedSchemaVersion {
                path,
                found,
                supported,
            } => write!(
                f,
                "unsupported run artifact schema_version={found}; this tailtriage version supports schema_version={supported}. Regenerate the artifact with a current tailtriage version. ('{}')",
                display_path(path)
            ),
            Self::Validation { path, message } => write!(
                f,
                "invalid run artifact '{}': {message}",
                display_path(path),
                message = escape_control_chars(message)
            ),
        }
    }
}

impl std::error::Error for ArtifactLoadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        if let Self::Read { source, .. } = self {
            Some(source)
        } else {
            None
        }
    }
}

/// Loads and decodes a tailtriage run artifact from disk, then applies
/// permissive core normalization for command-level checks.
///
/// Core owns canonical streaming Run JSON decoding. The CLI owns path diagnostics,
/// finalization policy, warnings, normalization, and the analyze command's
/// requirement that at least one request remains after normalization.
///
/// Loader warnings are non-fatal findings and are returned in
/// [`LoadedArtifact::warnings`].
///
/// # Errors
/// Returns [`ArtifactLoadError`] when the file cannot be read, the JSON is malformed,
/// the schema is unsupported, or required sections are missing.
#[cfg(test)]
pub(crate) fn load_run_artifact(path: &Path) -> Result<LoadedArtifact, ArtifactLoadError> {
    let loaded = decode_run_artifact(path)?;
    validate_required_sections(&loaded.run, path)?;
    Ok(loaded)
}

/// Decodes a run artifact and returns both the original candidate and its
/// permissively normalized form without enforcing command-specific
/// minimum-request policy.
///
/// # Errors
/// Returns [`ArtifactLoadError`] when the file cannot be read, the JSON is malformed,
/// the schema envelope is unsupported, or the decoded shape is incompatible.
pub(crate) fn decode_run_artifact(path: &Path) -> Result<LoadedArtifact, ArtifactLoadError> {
    let original_run = decode_run_json_path(path).map_err(|error| map_decode_error(path, error))?;

    if original_run.metadata.finalized_at_unix_ms.is_none() {
        return Err(ArtifactLoadError::Validation {
            path: path.to_path_buf(),
            message: "active/unfinalized snapshot is not a completed Run artifact and must be finalized before CLI analysis".to_string(),
        });
    }

    let normalized = normalize_run_permissive(&original_run);
    let run = normalized.run;
    let validation_report = normalized.report;

    let mut warnings = original_run.metadata.lifecycle_warnings.clone();
    if original_run.metadata.unfinished_requests.count > 0 {
        warnings.push(format!(
            "artifact recorded {} unfinished request(s) at shutdown",
            original_run.metadata.unfinished_requests.count
        ));
    }

    Ok(LoadedArtifact {
        run,
        original_run,
        validation_report,
        warnings,
    })
}

fn map_decode_error(path: &Path, error: RunJsonDecodeError) -> ArtifactLoadError {
    let path = path.to_path_buf();
    match error {
        RunJsonDecodeError::Io(source) => ArtifactLoadError::Read { path, source },
        RunJsonDecodeError::UnsupportedSchemaVersion { found, supported } => ArtifactLoadError::UnsupportedSchemaVersion { path, found, supported },
        RunJsonDecodeError::Malformed(error) => {
            let message = if error.is_eof() {
                format!("JSON ended unexpectedly ({error}). The artifact may be truncated; re-run capture and ensure the file was fully written.")
            } else if error.is_syntax() {
                format!("malformed JSON ({error}).")
            } else if error.is_data() {
                format!("JSON data is incompatible with the expected run schema ({error}).")
            } else {
                format!("I/O error while parsing JSON ({error}).")
            };
            ArtifactLoadError::Parse { path, message }
        }
        RunJsonDecodeError::Shape(error) => ArtifactLoadError::Parse { path, message: format!("JSON shape does not match the tailtriage run schema ({error}). Check for missing required fields such as metadata.run_id and requests[].") },
        _ => ArtifactLoadError::Parse { path, message: error.to_string() },
    }
}

#[cfg(test)]
fn validate_required_sections(run: &Run, path: &Path) -> Result<(), ArtifactLoadError> {
    if run.requests.is_empty() {
        return Err(ArtifactLoadError::Validation {
            path: path.to_path_buf(),
            message: "requests section is empty. Capture at least one request event before running triage.".to_string(),
        });
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::load_run_artifact;

    // TT-TEST: L02 primary
    // TT-TEST: S02 secondary
    #[test]
    fn rejects_malformed_json() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("bad.json");
        std::fs::write(&path, "{ not json").expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected parse failure");
        let message = error.to_string();

        assert!(message.contains("failed to parse run artifact"));
        assert!(message.contains("malformed JSON"));
    }

    // TT-TEST: L02 primary
    #[test]
    fn rejects_missing_required_fields() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("missing-fields.json");
        std::fs::write(&path, r#"{"schema_version":2,"metadata":{},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#)
            .expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected schema failure");
        let message = error.to_string();

        assert!(message.contains("JSON shape does not match"));
        assert!(message.contains("missing required fields"));
    }

    // TT-TEST: L02 primary
    #[test]
    fn rejects_empty_requests_section() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("empty-requests.json");
        std::fs::write(&path, valid_run_json_with_requests("[]")).expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected validation failure");
        let message = error.to_string();

        assert!(message.contains("requests section is empty"));
    }

    // TT-TEST: L02 primary
    #[test]
    fn rejects_missing_schema_version() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("missing-version.json");
        std::fs::write(&path, valid_run_json_with_prefix("")).expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected missing version failure");
        let message = error.to_string();

        assert!(message.contains("JSON shape does not match"));
        assert!(message.contains("schema_version"));
    }

    // TT-TEST: L02 primary
    #[test]
    fn rejects_non_integer_schema_versions() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("string-version.json");
        std::fs::write(
            &path,
            valid_run_json_with_prefix("\"schema_version\": \"1\","),
        )
        .expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected schema type failure");
        let message = error.to_string();

        assert!(message.contains("JSON shape does not match"));
        assert!(message.contains("invalid type"));
    }

    // TT-TEST: L02 primary
    #[test]
    fn rejects_unsupported_schema_versions() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("unsupported-version.json");
        std::fs::write(&path, valid_run_json_with_prefix("\"schema_version\": 99,"))
            .expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected version incompatibility");
        let message = error.to_string();

        assert!(message.contains("unsupported run artifact"));
        assert!(message.contains("schema_version=99"));
    }

    // TT-TEST: support
    #[test]
    fn flags_truncation_like_parse_errors() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("truncated.json");
        std::fs::write(&path, "{\"metadata\": {\"run_id\": \"x\"").expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected parse failure");
        let message = error.to_string();

        assert!(message.contains("may be truncated"));
    }

    // TT-TEST: support
    #[test]
    fn surfaces_unfinished_request_warnings() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("with-warning.json");
        std::fs::write(
            &path,
            r#"{"schema_version":2,"metadata":{"run_id":"r1","service_name":"svc","service_version":null,"started_at_unix_ms":1,"finalized_at_unix_ms":2,"mode":"light","host":null,"pid":null,"lifecycle_warnings":["x"],"unfinished_requests":{"count":1,"sample":[{"request_id":"req1","route":"/"}]}},"requests":[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#,
        )
        .expect("fixture should write");

        let artifact = load_run_artifact(&path).expect("load should succeed");
        assert!(artifact
            .warnings
            .iter()
            .any(|warning| warning.contains("unfinished request")));
    }

    // TT-TEST: L02 primary
    // TT-TEST: S02 secondary
    #[test]
    fn cli_loads_finalized_schema_v2_artifact() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("v2.json");
        std::fs::write(&path, valid_run_json_with_requests(r#"[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}]"#)).expect("fixture should write");
        let loaded = load_run_artifact(&path).expect("v2 finalized artifact should load");
        assert_eq!(loaded.original_run.schema_version, 2);
        assert_eq!(loaded.original_run.metadata.finalized_at_unix_ms, Some(2));
    }

    // TT-TEST: S02 secondary
    #[test]
    fn structurally_incompatible_old_schema_fails_as_shape_error() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("v1.json");
        std::fs::write(&path, r#"{"schema_version":1,"metadata":{"finished_at_unix_ms":2},"requests":"not decoded as v2"}"#).expect("fixture should write");
        let error = load_run_artifact(&path).expect_err("incompatible shape should fail");
        let message = error.to_string();
        assert!(message.contains("JSON shape does not match"));
    }

    // TT-TEST: L02 primary
    #[test]
    fn cli_rejects_unfinalized_schema_v2_artifact() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("active.json");
        let json = valid_run_json_with_requests(r#"[{"request_id":"req1","route":"/","kind":null,"started_at_unix_ms":1,"finished_at_unix_ms":2,"latency_us":10,"outcome":"ok"}]"#)
            .replace(r#""finalized_at_unix_ms":2,"#, r#""finalized_at_unix_ms":null,"#);
        std::fs::write(&path, json).expect("fixture should write");
        let error =
            load_run_artifact(&path).expect_err("unfinalized persisted artifact should fail");
        let message = error.to_string();
        assert!(message.contains("active/unfinalized snapshot is not a completed Run artifact"));
        assert!(message.contains("finalized before CLI analysis"));
    }

    fn valid_run_json_with_requests(requests_json: &str) -> String {
        format!(
            "{{\"schema_version\":2,\"metadata\":{{\"run_id\":\"r1\",\"service_name\":\"svc\",\"service_version\":null,\"started_at_unix_ms\":1,\"finalized_at_unix_ms\":2,\"mode\":\"light\",\"host\":null,\"pid\":null,\"lifecycle_warnings\":[],\"unfinished_requests\":{{\"count\":0,\"sample\":[]}}}},\"requests\":{requests_json},\"stages\":[],\"queues\":[],\"inflight\":[],\"runtime_snapshots\":[]}}"
        )
    }

    fn valid_run_json_with_prefix(prefix: &str) -> String {
        format!(
            "{{{prefix}\"metadata\":{{\"run_id\":\"r1\",\"service_name\":\"svc\",\"service_version\":null,\"started_at_unix_ms\":1,\"finalized_at_unix_ms\":2,\"mode\":\"light\",\"host\":null,\"pid\":null,\"lifecycle_warnings\":[],\"unfinished_requests\":{{\"count\":0,\"sample\":[]}}}},\"requests\":[{{\"request_id\":\"req1\",\"route\":\"/\",\"kind\":null,\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"latency_us\":10,\"outcome\":\"ok\"}}],\"stages\":[],\"queues\":[],\"inflight\":[],\"runtime_snapshots\":[]}}"
        )
    }
}
