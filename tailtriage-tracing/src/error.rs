use std::error::Error;
use std::fmt::{Display, Formatter};

/// Import failure for trace-shaped intake conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Required field was missing.
    MissingField(&'static str),
    /// Field value was invalid for conversion.
    InvalidField { field: &'static str, reason: String },
    /// Strict mode rejected input with warnings.
    StrictModeRejected,
}

impl Display for ImportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid field '{field}': {reason}")
            }
            Self::StrictModeRejected => {
                write!(f, "strict import rejected input due to conversion warnings")
            }
        }
    }
}

impl Error for ImportError {}
