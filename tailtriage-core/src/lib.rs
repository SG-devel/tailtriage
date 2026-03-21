//! Core run schema and local JSON sink for tailtriage.

mod collector;
mod config;
mod events;
mod sink;
mod time;
mod timers;

pub use collector::{RequestBuilder, RequestContext, Tailtriage};
pub use config::{CaptureLimits, CaptureMode, InitError, TailtriageBuilder};
pub use events::{
    InFlightSnapshot, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot, StageEvent,
    TruncationSummary,
};
pub use sink::{LocalJsonSink, RunSink, SinkError};
pub use time::{system_time_to_unix_ms, unix_time_ms};
pub use timers::{InflightGuard, QueueTimer, StageTimer};

#[cfg(test)]
mod tests;
