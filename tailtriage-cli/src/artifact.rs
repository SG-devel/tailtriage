use std::path::{Path, PathBuf};

use serde_json::Value;
use tailtriage_core::Run;

const SUPPORTED_SCHEMA_VERSION: u64 = 1;

#[derive(Debug)]
pub struct LoadedArtifact {
    pub run: Run,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub enum ArtifactLoadError {
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    Parse {
        path: PathBuf,
        message: String,
    },
    UnsupportedSchemaVersion {
        path: PathBuf,
        found: u64,
        supported: u64,
    },
    InvalidSchemaVersionType {
        path: PathBuf,
    },
    Validation {
        path: PathBuf,
        message: String,
    },
}

impl std::fmt::Display for ArtifactLoadError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Read { path, source } => {
                write!(f, "failed to read run artifact '{}': {source}", path.display())
            }
            Self::Parse { path, message } => {
                write!(f, "failed to parse run artifact '{}': {message}", path.display())
            }
            Self::UnsupportedSchemaVersion {
                path,
                found,
                supported,
            } => write!(
                f,
                "unsupported run artifact schema_version={found} in '{}'; supported schema_version is {supported}. Re-generate the artifact with a compatible tailtriage version.",
                path.display()
            ),
            Self::InvalidSchemaVersionType { path } => write!(
                f,
                "invalid run artifact in '{}': schema_version must be an integer when provided.",
                path.display()
            ),
            Self::Validation { path, message } => write!(
                f,
                "invalid run artifact '{}': {message}",
                path.display()
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

/// Loads and validates a tailtriage run artifact from disk.
///
/// # Errors
/// Returns [`ArtifactLoadError`] when the file cannot be read, the JSON is malformed,
/// the schema is unsupported, or required sections are missing.
pub fn load_run_artifact(path: &Path) -> Result<LoadedArtifact, ArtifactLoadError> {
    let input = std::fs::read_to_string(path).map_err(|source| ArtifactLoadError::Read {
        path: path.to_path_buf(),
        source,
    })?;

    let raw: Value = serde_json::from_str(&input).map_err(|err| ArtifactLoadError::Parse {
        path: path.to_path_buf(),
        message: parse_error_message(&err),
    })?;

    validate_schema_version(&raw, path)?;

    let run: Run = serde_json::from_value(raw).map_err(|err| ArtifactLoadError::Parse {
        path: path.to_path_buf(),
        message: format!(
            "JSON shape does not match the tailtriage run schema ({err}). Check for missing required fields such as metadata.run_id and requests[]."
        ),
    })?;

    validate_required_sections(&run, path)?;

    Ok(LoadedArtifact {
        run,
        warnings: Vec::new(),
    })
}

fn validate_schema_version(raw: &Value, path: &Path) -> Result<(), ArtifactLoadError> {
    if let Some(version) = raw.get("schema_version") {
        let Some(found) = version.as_u64() else {
            return Err(ArtifactLoadError::InvalidSchemaVersionType {
                path: path.to_path_buf(),
            });
        };

        if found != SUPPORTED_SCHEMA_VERSION {
            return Err(ArtifactLoadError::UnsupportedSchemaVersion {
                path: path.to_path_buf(),
                found,
                supported: SUPPORTED_SCHEMA_VERSION,
            });
        }
    }

    Ok(())
}

fn validate_required_sections(run: &Run, path: &Path) -> Result<(), ArtifactLoadError> {
    if run.requests.is_empty() {
        return Err(ArtifactLoadError::Validation {
            path: path.to_path_buf(),
            message: "requests section is empty. Capture at least one request event before running triage.".to_string(),
        });
    }

    Ok(())
}

fn parse_error_message(error: &serde_json::Error) -> String {
    match error.classify() {
        serde_json::error::Category::Eof => {
            format!("JSON ended unexpectedly ({error}). The artifact may be truncated; re-run capture and ensure the file was fully written.")
        }
        serde_json::error::Category::Syntax => {
            format!("malformed JSON ({error}).")
        }
        serde_json::error::Category::Data => {
            format!("JSON data is incompatible with the expected run schema ({error}).")
        }
        serde_json::error::Category::Io => {
            format!("I/O error while parsing JSON ({error}).")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::load_run_artifact;

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

    #[test]
    fn rejects_missing_required_fields() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("missing-fields.json");
        std::fs::write(&path, r#"{"metadata":{},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#)
            .expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected schema failure");
        let message = error.to_string();

        assert!(message.contains("JSON shape does not match"));
        assert!(message.contains("missing required fields"));
    }

    #[test]
    fn rejects_empty_requests_section() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("empty-requests.json");
        std::fs::write(&path, valid_run_json_with_requests("[]")).expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected validation failure");
        let message = error.to_string();

        assert!(message.contains("requests section is empty"));
    }

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

    #[test]
    fn flags_truncation_like_parse_errors() {
        let dir = tempfile::tempdir().expect("tempdir should build");
        let path = dir.path().join("truncated.json");
        std::fs::write(&path, "{\"metadata\": {\"run_id\": \"x\"").expect("fixture should write");

        let error = load_run_artifact(&path).expect_err("expected parse failure");
        let message = error.to_string();

        assert!(message.contains("may be truncated"));
    }

    fn valid_run_json_with_requests(requests_json: &str) -> String {
        format!(
            "{{\"metadata\":{{\"run_id\":\"r1\",\"service_name\":\"svc\",\"service_version\":null,\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"mode\":\"light\",\"host\":null,\"pid\":null}},\"requests\":{requests_json},\"stages\":[],\"queues\":[],\"inflight\":[],\"runtime_snapshots\":[]}}"
        )
    }

    fn valid_run_json_with_prefix(prefix: &str) -> String {
        format!(
            "{{{prefix}\"metadata\":{{\"run_id\":\"r1\",\"service_name\":\"svc\",\"service_version\":null,\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"mode\":\"light\",\"host\":null,\"pid\":null}},\"requests\":[{{\"request_id\":\"req1\",\"route\":\"/\",\"kind\":null,\"started_at_unix_ms\":1,\"finished_at_unix_ms\":2,\"latency_us\":10,\"outcome\":\"ok\"}}],\"stages\":[],\"queues\":[],\"inflight\":[],\"runtime_snapshots\":[]}}"
        )
    }
}
