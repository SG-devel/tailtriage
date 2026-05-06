#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! `tailtriage-cli` is the command-line artifact loader and report emitter.
//! For in-process Rust analysis/report APIs, use `tailtriage-analyzer`.

/// Artifact loading and validation helpers for CLI workflows.
pub mod artifact;
