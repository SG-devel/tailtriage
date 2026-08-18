use std::fmt;
use std::fs::File;
use std::io::{self, BufReader, Read, Seek, SeekFrom};
use std::path::Path;

use serde::de::{self, DeserializeSeed, IgnoredAny, MapAccess, SeqAccess, Visitor};
use serde::Deserialize;

use crate::{Run, SCHEMA_VERSION};

// Investigation defaults can retain 2.4 million events. At roughly 400 bytes
// per pretty-printed event that is about 960 MB; 2 GiB leaves more than 2x
// headroom for representative identifiers and JSON formatting while remaining
// an absolute, artifact-independent intake boundary for the 0.4 line.
/// Fixed raw Run JSON intake ceiling used by sibling artifact consumers.
pub const MAX_RUN_JSON_BYTES: u64 = 2 * 1024 * 1024 * 1024;

#[derive(Debug)]
/// Internal failure categories for canonical Run JSON decoding.
pub enum RunJsonDecodeError {
    /// The artifact could not be opened, read, or rewound.
    Io(io::Error),
    /// The raw artifact required more than the fixed byte allowance.
    TooLarge {
        /// Maximum raw bytes accepted.
        limit: u64,
    },
    /// The JSON envelope was syntactically malformed or truncated.
    Malformed(serde_json::Error),
    /// The top-level schema version field was absent.
    MissingSchemaVersion,
    /// The schema version was not a non-negative integer.
    InvalidSchemaVersionType,
    /// The schema version is not supported by this core version.
    UnsupportedSchemaVersion {
        /// Version found in the artifact.
        found: u64,
        /// Version supported by this decoder.
        supported: u64,
    },
    /// The supported envelope did not decode into a Run.
    Shape(serde_json::Error),
}

impl fmt::Display for RunJsonDecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "I/O error while reading Run JSON: {error}"),
            Self::TooLarge { limit } => write!(f, "Run JSON exceeds the {limit}-byte intake limit"),
            Self::Malformed(error) => write!(f, "invalid Run JSON: {error}"),
            Self::MissingSchemaVersion => write!(f, "missing required top-level schema_version"),
            Self::InvalidSchemaVersionType => write!(f, "schema_version must be an integer"),
            Self::UnsupportedSchemaVersion { found, supported } => write!(
                f,
                "unsupported schema_version={found}; supported schema_version={supported}"
            ),
            Self::Shape(error) => write!(f, "JSON shape does not match the Run schema: {error}"),
        }
    }
}

impl std::error::Error for RunJsonDecodeError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Malformed(error) | Self::Shape(error) => Some(error),
            _ => None,
        }
    }
}

/// Decodes one canonical Run JSON file through the fixed bounded intake path.
///
/// # Errors
///
/// Returns a categorized decode error for I/O, resource-bound, JSON envelope,
/// schema-version, or typed Run-shape failures.
pub fn decode_run_json_path(path: &Path) -> Result<Run, RunJsonDecodeError> {
    decode_run_json_file_with_limit(
        File::open(path).map_err(RunJsonDecodeError::Io)?,
        MAX_RUN_JSON_BYTES,
    )
}

fn decode_run_json_file_with_limit(mut file: File, limit: u64) -> Result<Run, RunJsonDecodeError> {
    probe_schema(&mut file, limit)?;
    file.seek(SeekFrom::Start(0))
        .map_err(RunJsonDecodeError::Io)?;
    decode_typed(&mut file, limit)
}

fn probe_schema<R: Read>(reader: R, limit: u64) -> Result<(), RunJsonDecodeError> {
    let mut bounded = BoundedReader::new(reader, limit);
    let result = {
        let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(&mut bounded));
        SchemaProbe.deserialize(&mut deserializer)
    };
    let version = map_json_result(result, &bounded, true)?;
    match version {
        None => Err(RunJsonDecodeError::MissingSchemaVersion),
        Some(None) => Err(RunJsonDecodeError::InvalidSchemaVersionType),
        Some(Some(found)) if found != SCHEMA_VERSION => {
            Err(RunJsonDecodeError::UnsupportedSchemaVersion {
                found,
                supported: SCHEMA_VERSION,
            })
        }
        Some(Some(_)) => Ok(()),
    }
}

fn decode_typed<R: Read>(reader: R, limit: u64) -> Result<Run, RunJsonDecodeError> {
    let mut bounded = BoundedReader::new(reader, limit);
    let result = {
        let mut deserializer = serde_json::Deserializer::from_reader(BufReader::new(&mut bounded));
        match Run::deserialize(&mut deserializer) {
            Ok(run) => deserializer.end().map(|()| run),
            Err(error) => Err(error),
        }
    };
    let run = map_json_result(result, &bounded, false)?;
    if run.schema_version != SCHEMA_VERSION {
        return Err(RunJsonDecodeError::UnsupportedSchemaVersion {
            found: run.schema_version,
            supported: SCHEMA_VERSION,
        });
    }
    Ok(run)
}

fn map_json_result<T>(
    result: Result<T, serde_json::Error>,
    reader: &BoundedReader<impl Read>,
    probe: bool,
) -> Result<T, RunJsonDecodeError> {
    result.map_err(|error| {
        if reader.exceeded {
            RunJsonDecodeError::TooLarge {
                limit: reader.limit,
            }
        } else if error.classify() == serde_json::error::Category::Io {
            RunJsonDecodeError::Io(io::Error::other(error))
        } else if probe {
            RunJsonDecodeError::Malformed(error)
        } else {
            RunJsonDecodeError::Shape(error)
        }
    })
}

struct BoundedReader<R> {
    inner: R,
    limit: u64,
    consumed: u64,
    exceeded: bool,
}
impl<R> BoundedReader<R> {
    fn new(inner: R, limit: u64) -> Self {
        Self {
            inner,
            limit,
            consumed: 0,
            exceeded: false,
        }
    }
}
impl<R: Read> Read for BoundedReader<R> {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if buf.is_empty() {
            return Ok(0);
        }
        let remaining = self.limit.saturating_sub(self.consumed);
        if remaining == 0 {
            let mut byte = [0];
            if self.inner.read(&mut byte)? == 0 {
                return Ok(0);
            }
            self.exceeded = true;
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "Run JSON intake limit exceeded",
            ));
        }
        let allowed = usize::try_from(remaining)
            .unwrap_or(usize::MAX)
            .min(buf.len());
        let read = self.inner.read(&mut buf[..allowed])?;
        self.consumed = self.consumed.saturating_add(read as u64);
        Ok(read)
    }
}

struct SchemaProbe;
impl<'de> DeserializeSeed<'de> for SchemaProbe {
    type Value = Option<Option<u64>>;
    fn deserialize<D: de::Deserializer<'de>>(
        self,
        deserializer: D,
    ) -> Result<Self::Value, D::Error> {
        deserializer.deserialize_map(SchemaVisitor)
    }
}
struct SchemaVisitor;
impl<'de> Visitor<'de> for SchemaVisitor {
    type Value = Option<Option<u64>>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("a Run JSON object")
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        let mut version = None;
        while let Some(key) = map.next_key::<String>()? {
            if key == "schema_version" {
                version = Some(map.next_value_seed(IntegerProbe)?);
            } else {
                map.next_value::<IgnoredAny>()?;
            }
        }
        Ok(version)
    }
}
struct IntegerProbe;
impl<'de> DeserializeSeed<'de> for IntegerProbe {
    type Value = Option<u64>;
    fn deserialize<D: de::Deserializer<'de>>(self, d: D) -> Result<Self::Value, D::Error> {
        d.deserialize_any(IntegerVisitor)
    }
}
struct IntegerVisitor;
impl<'de> Visitor<'de> for IntegerVisitor {
    type Value = Option<u64>;
    fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("any JSON value")
    }
    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E> {
        Ok(Some(value))
    }
    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E> {
        Ok(u64::try_from(value).ok())
    }
    fn visit_bool<E>(self, _: bool) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_f64<E>(self, _: f64) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_str<E>(self, _: &str) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_string<E>(self, _: String) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_none<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_unit<E>(self) -> Result<Self::Value, E> {
        Ok(None)
    }
    fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
        while seq.next_element::<IgnoredAny>()?.is_some() {}
        Ok(None)
    }
    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
        while map.next_entry::<IgnoredAny, IgnoredAny>()?.is_some() {}
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;
    fn valid() -> Vec<u8> {
        br#"{"schema_version":2,"metadata":{"run_id":"r","service_name":"s","service_version":null,"started_at_unix_ms":1,"finalized_at_unix_ms":2,"mode":"light","host":null,"pid":null},"requests":[],"stages":[],"queues":[],"inflight":[],"runtime_snapshots":[]}"#.to_vec()
    }
    fn decode(bytes: &[u8], limit: u64) -> Result<Run, RunJsonDecodeError> {
        probe_schema(Cursor::new(bytes), limit)?;
        decode_typed(Cursor::new(bytes), limit)
    }
    #[test]
    fn exact_limit_and_one_over() {
        let v = valid();
        assert!(decode(&v, v.len() as u64).is_ok());
        assert!(matches!(
            decode(&v, v.len() as u64 - 1),
            Err(RunJsonDecodeError::TooLarge { .. })
        ));
    }
    #[test]
    fn schema_errors() {
        assert!(matches!(
            decode(br"{}", 2),
            Err(RunJsonDecodeError::MissingSchemaVersion)
        ));
        assert!(matches!(
            decode(br#"{"schema_version":"2"}"#, 22),
            Err(RunJsonDecodeError::InvalidSchemaVersionType)
        ));
        assert!(matches!(
            decode(br#"{"schema_version":99}"#, 21),
            Err(RunJsonDecodeError::UnsupportedSchemaVersion { found: 99, .. })
        ));
    }
    #[test]
    fn malformed_and_trailing_are_not_size_errors() {
        assert!(matches!(
            decode(br#"{"schema_version":2"#, 19),
            Err(RunJsonDecodeError::Malformed(_))
        ));
        let mut v = valid();
        v.extend(b" x");
        assert!(matches!(
            decode(&v, v.len() as u64),
            Err(RunJsonDecodeError::Shape(_))
        ));
    }
    #[test]
    fn nested_unknown_and_control_string_are_inert() {
        let mut v = valid();
        let needle = b"\"requests\":[]";
        let pos = v.windows(needle.len()).position(|w| w == needle).unwrap();
        v.splice(
            pos..pos,
            b"\"unknown\":[[[{\"x\":\"\\u001b[31m\"}]]],"
                .iter()
                .copied(),
        );
        assert!(decode(&v, v.len() as u64).is_ok());
    }
    #[test]
    fn deep_json_fails_safely() {
        let data = format!(
            "{{\"schema_version\":2,\"unknown\":{} }}",
            "[".repeat(256) + &"]".repeat(256)
        );
        assert!(matches!(
            decode(data.as_bytes(), data.len() as u64),
            Err(RunJsonDecodeError::Malformed(_) | RunJsonDecodeError::Shape(_))
        ));
    }
    #[test]
    fn large_string_hits_reader_bound() {
        let data = format!(
            "{{\"schema_version\":2,\"unknown\":\"{}\"}}",
            "x".repeat(100)
        );
        assert!(matches!(
            decode(data.as_bytes(), 32),
            Err(RunJsonDecodeError::TooLarge { limit: 32 })
        ));
    }
    #[test]
    fn path_with_spaces() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("run artifact.json");
        std::fs::write(&path, valid()).unwrap();
        assert!(decode_run_json_path(&path).is_ok());
    }
}
