use crate::{ParamSchema, colorize, jpeg, skin};

#[derive(Clone, Debug)]
pub struct FilterInfo {
    pub name: &'static str,
    pub params: &'static [ParamSchema],
    pub requires_weights: bool,
    pub limitation: Option<&'static str>,
}
/// Metadata only. Never initializes ORT, reads weights, or accesses the network.
pub fn catalog() -> Vec<FilterInfo> {
    vec![
        FilterInfo {
            name: "Skin Smoothing",
            params: skin::SCHEMA,
            requires_weights: false,
            limitation: None,
        },
        FilterInfo {
            name: "Colorize",
            params: colorize::SCHEMA,
            requires_weights: true,
            limitation: Some(
                "CPU-only DDColor; this graph fails the strict CoreML partition guard",
            ),
        },
        FilterInfo {
            name: "JPEG Artifact Removal",
            params: jpeg::SCHEMA,
            requires_weights: true,
            limitation: None,
        },
    ]
}
