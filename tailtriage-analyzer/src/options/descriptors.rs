use std::sync::OnceLock;

use super::registry::OPTION_SPECS;
use super::AnalyzeOptionDescriptor;

/// Returns semantic analyzer option descriptors for every supported v1 path.
#[must_use]
pub fn analyze_option_descriptors() -> &'static [AnalyzeOptionDescriptor] {
    static DESCRIPTORS: OnceLock<Box<[AnalyzeOptionDescriptor]>> = OnceLock::new();
    DESCRIPTORS
        .get_or_init(|| {
            OPTION_SPECS
                .iter()
                .map(super::registry::OptionSpec::descriptor)
                .collect::<Vec<_>>()
                .into_boxed_slice()
        })
        .as_ref()
}
