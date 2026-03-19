//! Core run schema and local JSON sink for tailscope.

mod collector;
mod config;
mod events;
mod sink;
mod timers;

pub use collector::Tailscope;
pub use config::{CaptureMode, Config, InitError, RequestMeta};
pub use events::{
    InFlightSnapshot, QueueEvent, RequestEvent, Run, RunMetadata, RuntimeSnapshot, StageEvent,
};
pub use sink::{LocalJsonSink, RunSink, SinkError};
pub use timers::{InflightGuard, QueueTimer, StageTimer};

#[cfg(test)]
mod tests;
