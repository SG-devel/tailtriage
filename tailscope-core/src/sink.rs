use std::fs::File;
use std::io::{BufWriter, Error as IoError};
use std::path::{Path, PathBuf};

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
        let file = File::create(&self.path).map_err(SinkError::Io)?;
        let mut writer = BufWriter::new(file);
        serde_json::to_writer_pretty(&mut writer, run).map_err(SinkError::Serialize)?;
        Ok(())
    }
}

/// Errors emitted while writing run artifacts.
#[derive(Debug)]
pub enum SinkError {
    /// Underlying I/O failure.
    Io(IoError),
    /// Serialization failure.
    Serialize(serde_json::Error),
}

impl std::fmt::Display for SinkError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(err) => write!(f, "I/O error while writing run output: {err}"),
            Self::Serialize(err) => {
                write!(f, "serialization error while writing run output: {err}")
            }
        }
    }
}

impl std::error::Error for SinkError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(err) => Some(err),
            Self::Serialize(err) => Some(err),
        }
    }
}
