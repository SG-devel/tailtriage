use core::fmt;

use tailtriage_core::__internal::escape_control_chars;

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
    /// One logical JSONL record exceeded the shared fixed raw-byte ceiling.
    JsonlRecordTooLarge {
        /// 1-based logical line number.
        line: usize,
        /// Maximum permitted raw bytes, excluding the newline delimiter.
        limit: usize,
    },
    /// JSONL input did not match the stable tailtriage wrapper shape required by the stable tracing JSONL wrapper contract.
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
    /// Import or live-session configuration is invalid.
    InvalidConfiguration {
        /// Configuration option name.
        option: &'static str,
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
            } => write!(
                f,
                "io error while {operation} ({}): {}",
                escape_control_chars(context),
                escape_control_chars(reason)
            ),
            Self::MalformedJsonLine { line, reason } => {
                write!(
                    f,
                    "malformed JSONL at line {line}: {}",
                    escape_control_chars(reason)
                )
            }
            Self::JsonlRecordTooLarge { line, limit } => write!(
                f,
                "JSONL record at line {line} exceeds the raw-byte limit of {limit} bytes"
            ),
            Self::ExpectedTailtriageWrapper { reason } => {
                write!(
                    f,
                    "expected tailtriage wrapper JSONL record: {}",
                    escape_control_chars(reason)
                )
            }
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(
                    f,
                    "invalid field `{field}`: {}",
                    escape_control_chars(reason)
                )
            }
            Self::InvalidConfiguration { option, reason } => {
                write!(
                    f,
                    "invalid configuration `{option}`: {}",
                    escape_control_chars(reason)
                )
            }
            Self::StrictViolation(message) => {
                write!(
                    f,
                    "strict import violation: {}",
                    escape_control_chars(message)
                )
            }
            Self::EmptyServiceName => write!(f, "service name must not be empty"),
            Self::InvalidRunEvent(message) => {
                write!(f, "invalid run event: {}", escape_control_chars(message))
            }
            Self::ZeroRequestArtifact { guidance } => {
                write!(f, "{}", escape_control_chars(guidance))
            }
            Self::ZeroRequestArtifactWithWarnings { guidance, warnings } => {
                writeln!(f, "{}", escape_control_chars(guidance))?;
                writeln!(f, "warnings observed during tracing intake:")?;
                for warning in warnings.iter().take(8) {
                    writeln!(f, "- {}", escape_control_chars(warning))?;
                }
                let omitted = warnings.len().saturating_sub(8);
                if omitted > 0 {
                    writeln!(f, "- ... and {omitted} additional warnings omitted")?;
                }
                Ok(())
            }
            Self::RunJsonWrite { path, reason } => {
                write!(
                    f,
                    "failed to write run JSON at {}: {}",
                    escape_control_chars(path),
                    escape_control_chars(reason)
                )
            }
        }
    }
}

impl std::error::Error for ImportError {}

#[cfg(test)]
mod tests {
    use super::ImportError;

    // TT-TEST: S04 primary

    #[test]
    fn import_error_display_escapes_dynamic_fields_and_preserves_layout() {
        let error = ImportError::ZeroRequestArtifactWithWarnings {
            guidance: "retry\nnow".to_owned(),
            warnings: vec!["hostile\u{1b}warning".to_owned()],
        };

        assert_eq!(
            error.to_string(),
            "retry\\nnow\nwarnings observed during tracing intake:\n- hostile\\u{1b}warning\n"
        );
    }

    // TT-TEST: support

    #[test]
    fn jsonl_resource_errors_have_deterministic_context() {
        assert_eq!(
            ImportError::JsonlRecordTooLarge { line: 7, limit: 9 }.to_string(),
            "JSONL record at line 7 exceeds the raw-byte limit of 9 bytes"
        );
    }
}
