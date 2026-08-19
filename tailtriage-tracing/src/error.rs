use core::fmt;

fn escape_human_text(input: &str) -> String {
    let mut output = String::with_capacity(input.len());
    for ch in input.chars() {
        if ch.is_control() {
            output.extend(ch.escape_default());
        } else {
            output.push(ch);
        }
    }
    output
}

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
                escape_human_text(context),
                escape_human_text(reason)
            ),
            Self::MalformedJsonLine { line, reason } => {
                write!(
                    f,
                    "malformed JSONL at line {line}: {}",
                    escape_human_text(reason)
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
                    escape_human_text(reason)
                )
            }
            Self::MissingField(field) => write!(f, "missing required field: {field}"),
            Self::InvalidField { field, reason } => {
                write!(f, "invalid field `{field}`: {}", escape_human_text(reason))
            }
            Self::InvalidConfiguration { option, reason } => {
                write!(
                    f,
                    "invalid configuration `{option}`: {}",
                    escape_human_text(reason)
                )
            }
            Self::StrictViolation(message) => {
                write!(f, "strict import violation: {}", escape_human_text(message))
            }
            Self::EmptyServiceName => write!(f, "service name must not be empty"),
            Self::InvalidRunEvent(message) => {
                write!(f, "invalid run event: {}", escape_human_text(message))
            }
            Self::ZeroRequestArtifact { guidance } => write!(f, "{}", escape_human_text(guidance)),
            Self::ZeroRequestArtifactWithWarnings { guidance, warnings } => {
                writeln!(f, "{}", escape_human_text(guidance))?;
                writeln!(f, "warnings observed during tracing intake:")?;
                for warning in warnings.iter().take(8) {
                    writeln!(f, "- {}", escape_human_text(warning))?;
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
                    escape_human_text(path),
                    escape_human_text(reason)
                )
            }
        }
    }
}

impl std::error::Error for ImportError {}

#[cfg(test)]
mod tests {
    use super::{escape_human_text, ImportError};

    #[test]
    fn human_text_escaping_is_visible_idempotent_and_unicode_preserving() {
        let input = "plain\\slash café 東京\n\r\t\u{1b}\u{7}\u{8}\u{7f}\u{85}";
        let escaped = escape_human_text(input);
        assert_eq!(
            escaped,
            "plain\\slash café 東京\\n\\r\\t\\u{1b}\\u{7}\\u{8}\\u{7f}\\u{85}"
        );
        assert!(!escaped.chars().any(char::is_control));
        assert_eq!(escape_human_text(&escaped), escaped);
    }

    #[test]
    fn jsonl_resource_errors_have_deterministic_context() {
        assert_eq!(
            ImportError::JsonlRecordTooLarge { line: 7, limit: 9 }.to_string(),
            "JSONL record at line 7 exceeds the raw-byte limit of 9 bytes"
        );
    }
}
