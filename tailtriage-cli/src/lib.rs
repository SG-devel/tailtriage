#![doc = include_str!("../README.md")]
#![warn(missing_docs)]

//! Contract note: CLI artifact loading requires non-empty `requests`, while
//! [`analyze::analyze_run`] can analyze an in-memory [`tailtriage_core::Run`]
//! that has zero requests.
//!
/// Heuristic triage analyzer and text-report rendering.
pub mod analyze;
/// Artifact loading and validation helpers for CLI workflows.
pub mod artifact;
