//! Error types for tracing intake import scaffolding.

use std::error::Error;
use std::fmt::{Display, Formatter};

/// Import errors for tracing intake conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Input data is malformed for conversion.
    InvalidInput(String),
    /// Input data is missing a required field.
    MissingField(&'static str),
}

impl Display for ImportError {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidInput(message) => write!(f, "invalid import input: {message}"),
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
        }
    }
}

impl Error for ImportError {}
