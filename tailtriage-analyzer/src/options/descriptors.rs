use std::sync::LazyLock;

use super::registry::OPTION_ENTRIES;
use super::AnalyzeOptionDescriptor;

static DESCRIPTORS: LazyLock<Vec<AnalyzeOptionDescriptor>> = LazyLock::new(|| {
    OPTION_ENTRIES
        .iter()
        .map(super::registry::OptionEntry::descriptor)
        .collect()
});

/// Returns the shared registry view of every supported semantic analyzer option.
///
/// Rust callers can use these descriptors to discover stable paths, displayed defaults, Rust
/// value types, affected behavior, descriptions, and directional effects. The same registry
/// backs checked string/TOML configuration and non-default report summaries. Descriptors do not
/// replace semantic validation; call [`AnalyzeOptions::validate`](super::AnalyzeOptions::validate).
#[must_use]
pub fn analyze_option_descriptors() -> &'static [AnalyzeOptionDescriptor] {
    DESCRIPTORS.as_slice()
}
