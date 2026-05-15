use std::fmt;

/// Error returned by tracing import operations.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Input data was malformed for import.
    InvalidInput(String),
    /// Strict mode rejected non-conforming input.
    StrictViolation(String),
    /// Failed to construct an output run.
    RunBuild(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "invalid tracing input: {message}"),
            Self::StrictViolation(message) => write!(f, "strict import violation: {message}"),
            Self::RunBuild(message) => write!(f, "failed to build run artifact: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}
