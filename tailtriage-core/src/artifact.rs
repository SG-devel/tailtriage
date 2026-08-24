use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read};
use std::path::Path;

use serde::Deserialize;

use crate::{Run, SCHEMA_VERSION};

/// Failures produced while decoding canonical Run JSON.
#[derive(Debug)]
#[non_exhaustive]
pub enum RunJsonDecodeError {
    /// The artifact could not be opened or read.
    Io(io::Error),
    /// The JSON document was syntactically malformed or truncated.
    Malformed(serde_json::Error),
    /// The JSON data was incompatible with the canonical [`Run`] shape.
    Shape(serde_json::Error),
    /// The decoded Run uses a schema version unsupported by this core version.
    UnsupportedSchemaVersion {
        /// Version found in the artifact.
        found: u64,
        /// Version supported by this decoder.
        supported: u64,
    },
}

impl fmt::Display for RunJsonDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error while reading Run JSON: {error}"),
            Self::Malformed(error) => write!(f, "invalid Run JSON: {error}"),
            Self::Shape(error) => write!(f, "JSON shape does not match the Run schema: {error}"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported schema_version={found}; supported schema_version={supported}"
            ),
        }
    }
}

impl std::error::Error for RunJsonDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Malformed(error) | Self::Shape(error) => Some(error),
            Self::UnsupportedSchemaVersion { .. } => None,
        }
    }
}

/// Decodes one canonical Run JSON file with a streaming typed parse.
///
/// The document is decoded directly into [`Run`] without first retaining the
/// complete input as a string or generic JSON value. Trailing non-whitespace is
/// rejected. This removes avoidable raw-input amplification; it does not impose
/// a universal allocation bound on arbitrarily large, otherwise valid Runs.
///
/// # Errors
///
/// Returns a categorized error for I/O, malformed JSON, incompatible Run data,
/// or a successfully decoded Run whose schema version is unsupported.
pub fn decode_run_json_path(path: &Path) -> Result<Run, RunJsonDecodeError> {
    let file = File::open(path).map_err(RunJsonDecodeError::Io)?;
    decode_run_json_reader(file)
}

fn decode_run_json_reader<R: Read>(reader: R) -> Result<Run, RunJsonDecodeError> {
    let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(reader));
    let run = Run::deserialize(&mut deserializer).map_err(map_json_error)?;
    deserializer.end().map_err(map_json_error)?;

    if run.schema_version != SCHEMA_VERSION {
        return Err(RunJsonDecodeError::UnsupportedSchemaVersion {
            found: run.schema_version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(run)
}

fn map_json_error(error: serde_json::Error) -> RunJsonDecodeError {
    match error.classify() {
        serde_json::error::Category::Io => RunJsonDecodeError::Io(io::Error::other(error)),
        serde_json::error::Category::Syntax | serde_json::error::Category::Eof => {
            RunJsonDecodeError::Malformed(error)
        }
        serde_json::error::Category::Data => RunJsonDecodeError::Shape(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn valid() -> Vec<u8> {
        br#"{"schema_version":2,"metadata":{"run_id":"r","service_name":"s","service_version":null,"started_at_unix_ms":1,"finalized_at_unix_ms":2,"mode":"light","host":null,"pid":null},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#.to_vec()
    }

    fn decode(bytes: &[u8]) -> Result<Run, RunJsonDecodeError> {
        decode_run_json_reader(Cursor::new(bytes))
    }

    // TT-TEST: S02 primary
    #[test]
    fn valid_supported_run_decodes() {
        assert!(decode(&valid()).is_ok());
    }

    // TT-TEST: S02 primary
    #[test]
    fn malformed_and_truncated_json_are_rejected() {
        assert!(matches!(
            decode(br#"{"schema_version":2, nope}"#),
            Err(RunJsonDecodeError::Malformed(_))
        ));
        assert!(matches!(
            decode(br#"{"schema_version":2"#),
            Err(RunJsonDecodeError::Malformed(_))
        ));
    }

    // TT-TEST: S02 primary
    #[test]
    fn typed_shape_errors_are_rejected() {
        assert!(matches!(
            decode(br#"{"schema_version":2}"#),
            Err(RunJsonDecodeError::Shape(_))
        ));
        let wrong_version_type = String::from_utf8(valid())
            .unwrap()
            .replace("\"schema_version\":2", "\"schema_version\":\"2\"");
        assert!(matches!(
            decode(wrong_version_type.as_bytes()),
            Err(RunJsonDecodeError::Shape(_))
        ));
        let missing_version = String::from_utf8(valid())
            .unwrap()
            .replace("\"schema_version\":2,", "");
        assert!(matches!(
            decode(missing_version.as_bytes()),
            Err(RunJsonDecodeError::Shape(_))
        ));
    }

    // TT-TEST: S02 primary
    #[test]
    fn structurally_valid_unsupported_schema_has_dedicated_error() {
        let data = String::from_utf8(valid())
            .unwrap()
            .replace("\"schema_version\":2", "\"schema_version\":99");
        assert!(matches!(
            decode(data.as_bytes()),
            Err(RunJsonDecodeError::UnsupportedSchemaVersion {
                found: 99,
                supported: SCHEMA_VERSION
            })
        ));
    }

    // TT-TEST: S02 primary
    #[test]
    fn trailing_content_is_rejected() {
        let mut data = valid();
        data.extend_from_slice(b" true");
        assert!(matches!(
            decode(&data),
            Err(RunJsonDecodeError::Malformed(_))
        ));
    }

    // TT-TEST: S02 primary
    #[test]
    fn deep_json_fails_under_normal_recursion_protection() {
        let nesting_depth = 256;
        let nested = "[".repeat(nesting_depth) + &"]".repeat(nesting_depth);
        let mut data = String::from_utf8(valid()).unwrap();
        data.insert_str(data.len() - 1, &format!(",\"unknown\":{nested}"));

        // Deserialize the complete balanced document into a recursively visited
        // representation so this test exercises serde_json's depth guard even
        // though Run's derived visitor efficiently skips unknown field values.
        let mut deserializer = serde_json::Deserializer::from_slice(data.as_bytes());
        let json_error = serde_json::Value::deserialize(&mut deserializer)
            .expect_err("deep nesting must be rejected");
        assert!(
            json_error.to_string().contains("recursion limit exceeded"),
            "expected recursion-limit exhaustion, got {json_error}"
        );
    }

    // TT-TEST: support
    #[test]
    fn unknown_nested_fields_are_ignored() {
        let mut data = String::from_utf8(valid()).unwrap();
        data.insert_str(data.len() - 1, ",\"unknown\":{\"nested\":[1,2,3]}");
        assert!(decode(data.as_bytes()).is_ok());
    }

    // TT-TEST: support
    #[test]
    fn escaped_terminal_control_is_inert_string_data() {
        let data = String::from_utf8(valid()).unwrap().replace(
            "\"service_name\":\"s\"",
            "\"service_name\":\"\\u001b[31mservice\"",
        );
        let run = decode(data.as_bytes()).unwrap();
        assert_eq!(run.metadata.service_name, "\u{1b}[31mservice");
    }

    struct NonSeekReader {
        bytes: Cursor<Vec<u8>>,
    }

    impl Read for NonSeekReader {
        fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
            self.bytes.read(buffer)
        }
    }

    // TT-TEST: S02 primary
    #[test]
    fn decoder_accepts_non_seekable_reader() {
        let reader = NonSeekReader {
            bytes: Cursor::new(valid()),
        };
        assert!(decode_run_json_reader(reader).is_ok());
    }

    // TT-TEST: support
    #[test]
    fn path_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run artifact.json");
        std::fs::write(&path, valid()).unwrap();
        assert!(decode_run_json_path(&path).is_ok());
    }
}
