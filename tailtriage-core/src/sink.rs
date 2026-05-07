use std::fs::{self, OpenOptions};
use std::io::{BufWriter, Error as IoError, Write};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::Run;

/// A sink that persists the final run artifact produced at shutdown.
///
/// Implement this trait to plug in custom persistence backends.
///
/// # Example
///
/// ```no_run
/// use tailtriage_core::{Run, RunSink, SinkError, Tailtriage};
///
/// struct StdoutSink;
///
/// impl RunSink for StdoutSink {
///     fn write(&self, run: &Run) -> Result<(), SinkError> {
///         let bytes = serde_json::to_vec(run).map_err(SinkError::Serialize)?;
///         println!("{}", String::from_utf8_lossy(&bytes));
///         Ok(())
///     }
/// }
///
/// let run = Tailtriage::builder("checkout-service")
///     .sink(StdoutSink)
///     .build()?;
/// # let _ = run;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub trait RunSink {
    /// Persists a run.
    ///
    /// # Errors
    ///
    /// Returns [`SinkError`] if the sink cannot write the run output, such as
    /// when file I/O fails or serialization cannot complete.
    fn write(&self, run: &Run) -> Result<(), SinkError>;
}

/// Sink that finalizes capture lifecycle without writing a run artifact.
///
/// [`DiscardSink`] intentionally drops the finalized [`Run`] after shutdown and
/// does not persist any JSON file artifact.
///
/// Use [`MemorySink`] instead when you want to keep the finalized [`Run`] for
/// in-process analysis.
#[derive(Debug, Clone, Copy, Default)]
pub struct DiscardSink;

impl RunSink for DiscardSink {
    fn write(&self, _run: &Run) -> Result<(), SinkError> {
        Ok(())
    }
}

/// In-memory sink that stores only the last finalized run.
///
/// [`MemorySink`] writes no file artifact and keeps the most recent finalized
/// [`Run`] in memory. Later writes replace earlier stored runs.
///
/// Storing finalized runs clones captured data and can increase memory use for
/// large captures.
#[derive(Debug, Clone, Default)]
pub struct MemorySink {
    run: Arc<Mutex<Option<Run>>>,
}

impl MemorySink {
    /// Creates a new in-memory sink.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns a cloned copy of the last finalized run, if present.
    #[must_use]
    pub fn last_run(&self) -> Option<Run> {
        lock_recover(&self.run).clone()
    }

    /// Takes the last finalized run and clears the stored value.
    pub fn take_run(&self) -> Option<Run> {
        lock_recover(&self.run).take()
    }

    /// Clears any stored finalized run.
    pub fn clear(&self) {
        *lock_recover(&self.run) = None;
    }
}

impl RunSink for MemorySink {
    fn write(&self, run: &Run) -> Result<(), SinkError> {
        *lock_recover(&self.run) = Some(run.clone());
        Ok(())
    }
}

fn lock_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

/// Local file sink that writes one JSON document per run at shutdown.
///
/// This is the default sink used by [`crate::TailtriageBuilder`].
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
            finalize_temp_file(&temp_path, &self.path).map_err(SinkError::Io)
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

fn finalize_temp_file(temp_path: &Path, final_path: &Path) -> Result<(), IoError> {
    fs::rename(temp_path, final_path)
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
    use super::{
        finalize_temp_file, lock_recover, DiscardSink, LocalJsonSink, MemorySink, RunSink,
        SinkError,
    };
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
            finalized_at_unix_ms: Some(2),
            mode: CaptureMode::Light,
            effective_core_config: Some(crate::EffectiveCoreConfig {
                mode: CaptureMode::Light,
                capture_limits: CaptureMode::Light.core_defaults(),
                strict_lifecycle: false,
            }),
            effective_tokio_sampler_config: None,
            host: None,
            pid: Some(123),
            lifecycle_warnings: Vec::new(),
            unfinished_requests: UnfinishedRequests::default(),
            run_end_reason: None,
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
    fn discard_sink_write_succeeds() {
        let sink = DiscardSink;
        sink.write(&sample_run()).expect("discard should succeed");
    }

    #[test]
    fn memory_sink_replaces_previous_run() {
        let sink = MemorySink::new();
        let mut first = sample_run();
        first.metadata.run_id = "run-first".to_string();
        sink.write(&first).expect("first write should succeed");
        assert_eq!(
            sink.last_run()
                .expect("run should be present")
                .metadata
                .run_id,
            "run-first"
        );

        let mut second = sample_run();
        second.metadata.run_id = "run-second".to_string();
        sink.write(&second).expect("second write should succeed");
        assert_eq!(
            sink.last_run()
                .expect("run should be present")
                .metadata
                .run_id,
            "run-second"
        );
    }

    #[test]
    fn memory_sink_recovers_from_poisoned_mutex_operations() {
        let sink = MemorySink::new();
        {
            let sink_clone = sink.clone();
            let _ = std::thread::spawn(move || {
                let _guard = lock_recover(&sink_clone.run);
                panic!("poison mutex");
            })
            .join();
        }

        assert!(sink.last_run().is_none(), "last_run should recover");
        assert!(sink.take_run().is_none(), "take_run should recover");
        sink.clear();
        assert!(sink.last_run().is_none(), "clear should recover");
        sink.write(&sample_run()).expect("write should recover");
        assert!(sink.last_run().is_some(), "write should store run");
    }

    #[test]
    fn local_sink_write_replaces_existing_destination_with_new_run() {
        let output = unique_path("replace-existing");
        let sink = LocalJsonSink::new(&output);

        let mut first_run = sample_run();
        first_run.metadata.run_id = "run-first".to_string();
        sink.write(&first_run).expect("first write should succeed");

        let mut second_run = sample_run();
        second_run.metadata.run_id = "run-second".to_string();
        second_run.requests.push(crate::RequestEvent {
            request_id: "req-2".to_string(),
            route: "/checkout".to_string(),
            kind: Some("http".to_string()),
            started_at_unix_ms: 10,
            finished_at_unix_ms: 20,
            latency_us: 10_000,
            outcome: "ok".to_string(),
        });
        sink.write(&second_run)
            .expect("second write should replace existing artifact");

        let bytes = std::fs::read(&output).expect("artifact should be written");
        let restored: Run = serde_json::from_slice(&bytes).expect("artifact should deserialize");
        assert_eq!(restored, second_run, "existing file should be replaced");

        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn failed_finalization_keeps_existing_destination_unchanged() {
        let output = unique_path("finalization-failure");
        let original_payload = b"{\"run_id\":\"existing\"}";
        std::fs::write(&output, original_payload).expect("initial artifact should be writable");
        let missing_temp = unique_path("missing-temp");

        let err = finalize_temp_file(&missing_temp, &output)
            .expect_err("finalization should fail when temp is missing");
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);

        let final_payload = std::fs::read(&output).expect("existing final artifact should remain");
        assert_eq!(final_payload, original_payload);

        let _ = std::fs::remove_file(output);
    }

    #[test]
    fn local_sink_failed_finalization_cleans_up_temp_file_and_preserves_final_path() {
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
