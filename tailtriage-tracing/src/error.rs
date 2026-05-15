use core::fmt;

/// Import failures for tracing-shaped span ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
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
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid field `{field}`: {reason}")
            }
            Self::StrictViolation(message) => write!(f, "strict import violation: {message}"),
        }
    }
}

impl std::error::Error for ImportError {}
