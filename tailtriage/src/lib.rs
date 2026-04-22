#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

/// Re-export of `tailtriage-core`, always available at the crate root.
///
/// This crate is the recommended default entry point: start with core APIs here,
/// then enable optional integration namespaces via feature flags as needed.
pub use tailtriage_core::*;

#[cfg(feature = "axum")]
#[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
/// Optional Axum integration namespace (`tailtriage::axum`).
///
/// Enable with the `axum` feature.
pub use tailtriage_axum as axum;
#[cfg(feature = "controller")]
#[cfg_attr(docsrs, doc(cfg(feature = "controller")))]
/// Controller integration namespace (`tailtriage::controller`).
///
/// Enabled by default via the `controller` feature.
pub use tailtriage_controller as controller;
#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
/// Optional Tokio runtime sampler namespace (`tailtriage::tokio`).
///
/// Enable with the `tokio` feature.
pub use tailtriage_tokio as tokio;

#[cfg(test)]
mod tests {
    #[test]
    fn core_reexport_exposes_tailtriage() {
        let _builder = crate::Tailtriage::builder("default-smoke");
    }

    #[cfg(feature = "tokio")]
    #[test]
    fn tokio_namespace_reexport_compiles() {
        let _name = crate::tokio::crate_name();
    }

    #[cfg(feature = "controller")]
    #[test]
    fn controller_namespace_reexport_compiles() {
        let _builder = crate::controller::TailtriageController::builder("default-controller");
    }

    #[cfg(feature = "axum")]
    #[test]
    fn axum_namespace_reexport_compiles() {
        let _name = crate::axum::crate_name();
    }
}
