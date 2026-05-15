use core::fmt;

/// Import failures for tracing-shaped span ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Could not read input bytes from reader or path.
    Io(String),
    /// A JSONL line could not be parsed as JSON.
    MalformedJsonLine {
        /// Parse failure details.
        message: String,
    },
    /// Required field or option is missing.
    MissingField(&'static str),
    /// Field value had an invalid type or invalid content.
    InvalidField {
        /// Field key.
        field: &'static str,
        /// Human-readable reason.
        reason: String,
    },
    /// Import strictness rejected records that would otherwise be warnings.
    StrictViolation(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(message) => write!(f, "io error: {message}"),
            Self::MalformedJsonLine { message } => write!(f, "malformed JSONL line: {message}"),
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid field `{field}`: {reason}")
            }
            Self::StrictViolation(message) => write!(f, "strict import violation: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}
