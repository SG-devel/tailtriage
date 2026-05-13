#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnalyzeOptionDescriptor {
    pub path: &'static str,
    pub default_value: &'static str,
    pub value_type: &'static str,
    pub affects: &'static str,
    pub description: &'static str,
    pub increasing: Option<&'static str>,
    pub decreasing: Option<&'static str>,
}

pub const fn analyze_option_descriptors() -> &'static [AnalyzeOptionDescriptor] {
    &[]
}
