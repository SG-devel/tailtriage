#![doc = include_str!("../README.md")]
#![warn(missing_docs)]
#![cfg_attr(docsrs, feature(doc_cfg))]

// Official default crate for the tailtriage toolkit.
//
// - [`tailtriage_core`] is always re-exported as the foundational API surface.
// - [`controller`] is the default convenience layer when the `controller` feature is enabled.
// - [`tokio`] and [`axum`] are opt-in integration namespaces.

pub use tailtriage_core::*;

#[cfg(feature = "axum")]
#[cfg_attr(docsrs, doc(cfg(feature = "axum")))]
pub use tailtriage_axum as axum;
#[cfg(feature = "controller")]
#[cfg_attr(docsrs, doc(cfg(feature = "controller")))]
pub use tailtriage_controller as controller;
#[cfg(feature = "tokio")]
#[cfg_attr(docsrs, doc(cfg(feature = "tokio")))]
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
