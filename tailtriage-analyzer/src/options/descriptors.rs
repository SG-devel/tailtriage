use std::sync::LazyLock;

use super::registry::OPTION_ENTRIES;
use super::AnalyzeOptionDescriptor;

static DESCRIPTORS: LazyLock<Vec<AnalyzeOptionDescriptor>> = LazyLock::new(|| {
    OPTION_ENTRIES
        .iter()
        .map(super::registry::OptionEntry::descriptor)
        .collect()
});

/// Returns human-readable descriptors for every supported semantic analyzer option.
#[must_use]
pub fn analyze_option_descriptors() -> &'static [AnalyzeOptionDescriptor] {
    DESCRIPTORS.as_slice()
}
