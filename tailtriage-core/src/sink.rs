use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Error as IoError, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Run;

/// A sink that can persist a run artifact.
pub trait RunSink {
    /// Persists a run.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if the sink cannot write the run output, such as
    /// when file I/O fails or serialization cannot complete.
    fn write(&self, run: &Run) -> Result<(), SinkError>;
}

/// Local file sink that writes one JSON document per run.
#[derive(Debug, Clone)]
pub struct LocalJsonSink {
    path: PathBuf,
}

impl LocalJsonSink {
    /// Creates a local JSON sink for `path`.
    #[must_use]
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            path: path.as_ref().to_path_buf(),
        }
    }

    /// Returns the target file path used by this sink.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl RunSink for LocalJsonSink {
    fn write(&self, run: &Run) -> Result<(), SinkError> {
        let parent = self.path.parent().unwrap_or_else(|| Path::new("."));
        let temp_path = create_temp_path(parent, &self.path);
        let write_result = (|| {
            let file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temp_path)
                .map_err(SinkError::Io)?;
            let mut writer = BufWriter::new(file);
            serde_json::to_writer_pretty(&mut writer, run).map_err(SinkError::Serialize)?;
            writer.flush().map_err(SinkError::Io)?;
            let file = writer
                .into_inner()
                .map_err(|err| SinkError::Io(err.into_error()))?;
            file.sync_all().map_err(SinkError::Io)?;
            fs::rename(&temp_path, &self.path).map_err(SinkError::Io)
        })();

        if write_result.is_err() {
            let _ = fs::remove_file(&temp_path);
        }

        write_result
    }
}

fn create_temp_path(parent: &Path, final_path: &Path) -> PathBuf {
    let file_name = final_path
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .unwrap_or("tailtriage-run.json");
    let epoch_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| duration.as_nanos());
    parent.join(format!(
        ".{file_name}.tmp-{}-{epoch_nanos}",
        std::process::id()
    ))
}

/// Errors emitted while writing run artifacts.
#[derive(Debug)]
pub enum SinkError {
    /// Underlying I/O failure.
    Io(IoError),
    /// Serialization failure.
    Serialize(serde_json::Error),
    /// Strict lifecycle validation failure during shutdown.
    Lifecycle {
        /// Number of unfinished requests detected at shutdown.
        unfinished_count: usize,
    },
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error while writing run output: {err}"),
            Self::Serialize(err) => {
                write!(f, "serialization error while writing run output: {err}")
            }
            Self::Lifecycle { unfinished_count } => write!(
                f,
                "strict lifecycle validation failed: {unfinished_count} unfinished request(s) remained at shutdown"
            ),
        }
    }
}

impl std::error::Error for SinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Serialize(err) => Some(err),
            Self::Lifecycle { .. } => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{LocalJsonSink, RunSink, SinkError};
    use crate::{CaptureMode, Run, RunMetadata, UnfinishedRequests, SCHEMA_VERSION};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn unique_path(suffix: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |duration| duration.as_nanos());
        std::env::temp_dir().join(format!(
            "tailtriage-core-sink-{suffix}-{}-{nanos}.json",
            std::process::id()
        ))
    }

    fn sample_run() -> Run {
        Run::new(RunMetadata {
            run_id: "run-1".to_string(),
            service_name: "checkout".to_string(),
            service_version: Some("1.0.0".to_string()),
            started_at_unix_ms: 1,
            finished_at_unix_ms: 2,
            mode: CaptureMode::Light,
            host: None,
            pid: Some(123),
            lifecycle_warnings: Vec::new(),
            unfinished_requests: UnfinishedRequests::default(),
        })
    }

    #[test]
    fn local_sink_write_creates_deserializable_artifact() {
        let output = unique_path("success");
        let sink = LocalJsonSink::new(&output);
        let run = sample_run();

        sink.write(&run).expect("write should succeed");

        let bytes = std::fs::read(&output).expect("artifact should be written");
        let restored: Run = serde_json::from_slice(&bytes).expect("artifact should deserialize");
        assert_eq!(restored, run);
        assert_eq!(restored.schema_version, SCHEMA_VERSION);

        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn local_sink_failed_rename_cleans_up_temp_file_and_preserves_final_path() {
        let output = std::env::temp_dir().join(format!(
            "tailtriage-core-sink-dir-target-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map_or(0, |duration| duration.as_nanos())
        ));
        std::fs::create_dir_all(&output).expect("directory target should be created");
        let sink = LocalJsonSink::new(&output);

        let err = sink
            .write(&sample_run())
            .expect_err("rename to directory should fail");
        assert!(matches!(err, SinkError::Io(_)));
        assert!(
            output.is_dir(),
            "existing final directory should remain untouched"
        );

        let parent = output
            .parent()
            .expect("directory target should always have a parent");
        let final_name = output
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .expect("directory target should be valid utf-8 for this test");
        let temp_prefix = format!(".{final_name}.tmp-");
        let leftover_temp = std::fs::read_dir(parent)
            .expect("parent should be readable")
            .filter_map(Result::ok)
            .map(|entry| entry.file_name())
            .filter_map(|name| name.to_str().map(str::to_owned))
            .any(|name| name.starts_with(&temp_prefix));
        assert!(
            !leftover_temp,
            "temporary file should be cleaned up on failed finalization"
        );

        let _ = std::fs::remove_dir_all(output);
    }
}
