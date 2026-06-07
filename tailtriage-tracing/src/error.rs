use core::fmt;

/// Import failures for tracing-shaped span ingestion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImportError {
    /// Could not read JSONL input from a reader or filesystem path.
    Io {
        /// Operation being performed, such as "read jsonl line" or "open jsonl path".
        operation: &'static str,
        /// Human-readable context, such as a path or line number.
        context: String,
        /// Underlying I/O error text.
        reason: String,
    },
    /// A non-empty JSONL line could not be parsed as JSON.
    MalformedJsonLine {
        /// 1-based JSONL line number.
        line: usize,
        /// Underlying JSON parser error text.
        reason: String,
    },
    /// JSONL input did not match the stable tailtriage wrapper shape required by wrapper-only mode.
    ExpectedTailtriageWrapper {
        /// Human-readable reason the wrapper shape was rejected.
        reason: String,
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
    /// Service name was empty or whitespace-only.
    EmptyServiceName,
    /// Imported run event failed `tailtriage-core` run-builder validation.
    InvalidRunEvent(String),
    /// Persistable run artifact is missing required completed request spans.
    ZeroRequestArtifact {
        /// Actionable setup guidance for creating a persistable run artifact.
        guidance: String,
    },
    /// Persistable run artifact is missing completed request spans and warnings were observed.
    ZeroRequestArtifactWithWarnings {
        /// Actionable setup guidance for creating a persistable run artifact.
        guidance: String,
        /// Intake and lifecycle warning summaries observed before shutdown.
        warnings: Vec<String>,
    },
    /// Failed to write run JSON output via core sink.
    RunJsonWrite {
        /// Target output path.
        path: String,
        /// Underlying sink failure reason.
        reason: String,
    },
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io {
                operation,
                context,
                reason,
            } => write!(f, "io error while {operation} ({context}): {reason}"),
            Self::MalformedJsonLine { line, reason } => {
                write!(f, "malformed JSONL at line {line}: {reason}")
            }
            Self::ExpectedTailtriageWrapper { reason } => {
                write!(f, "expected tailtriage wrapper JSONL record: {reason}")
            }
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid field `{field}`: {reason}")
            }
            Self::StrictViolation(message) => write!(f, "strict import violation: {message}"),
            Self::EmptyServiceName => write!(f, "service name must not be empty"),
            Self::InvalidRunEvent(message) => write!(f, "invalid run event: {message}"),
            Self::ZeroRequestArtifact { guidance } => write!(f, "{guidance}"),
            Self::ZeroRequestArtifactWithWarnings { guidance, warnings } => {
                writeln!(f, "{guidance}")?;
                writeln!(f, "warnings observed during tracing intake:")?;
                for warning in warnings.iter().take(8) {
                    writeln!(f, "- {warning}")?;
                }
                let omitted = warnings.len().saturating_sub(8);
                if omitted > 0 {
                    writeln!(f, "- ... and {omitted} additional warnings omitted")?;
                }
                Ok(())
            }
            Self::RunJsonWrite { path, reason } => {
                write!(f, "failed to write run JSON at {path}: {reason}")
            }
        }
    }
}

impl std::error::Error for ImportError {}
