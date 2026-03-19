//! Tokio runtime integration for tailscope.

pub use tailscope_macros::instrument_request;

/// Returns the crate name for smoke-testing workspace wiring.
#[must_use]
pub const fn crate_name() -> &'static str {
    "tailscope-tokio"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    #[test]
    fn crate_name_is_stable() {
        assert_eq!(crate_name(), "tailscope-tokio");
    }
}
