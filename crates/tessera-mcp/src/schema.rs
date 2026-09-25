//! Every engine input schema is derived from its serde source at build time.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
#[allow(dead_code)]
mod wire {
    include!(concat!(env!("OUT_DIR"), "/wire_schemas.rs"));
}

#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct OpenImage {
    pub path: String,
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenderPreview {
    pub image: String,
    #[serde(default = "default_max")]
    pub max_px: u32,
}
fn default_max() -> u32 {
    1024
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListImages {
    #[serde(default)]
    pub query: Option<String>,
}
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DescribeImage {
    pub image: String,
}

pub fn tools() -> Vec<Value> {
    let mut schemas = wire::engine_schemas();
    schemas.extend([
        serde_json::to_value(schemars::schema_for!(OpenImage)).unwrap(),
        serde_json::to_value(schemars::schema_for!(RenderPreview)).unwrap(),
        serde_json::to_value(schemars::schema_for!(ListImages)).unwrap(),
        serde_json::to_value(schemars::schema_for!(DescribeImage)).unwrap(),
    ]);
    engine_api::tools::ToolCall::NAMES.into_iter().chain(["open_image","render_preview","list_images","describe_image"]).zip(schemas).map(|(name,schema)|json!({"name":name,"description":description(name),"inputSchema":schema})).collect()
}
fn description(name: &str) -> &str {
    match name {
        "remove_object" => {
            "Unsupported until non-generative inpainting exists; never generates pixels."
        }
        "retouch_skin" => "Unsupported until a non-generative skin retouch backend exists.",
        "apply_style" => {
            "Apply preset JSON resolved by StyleId in app-dir/styles/index.json; one Agent history entry."
        }
        "compare" => {
            "Both current renders plus mean, contrast, clipping and the requested comparison distance."
        }
        "open_image" => {
            "Resolve an image file by indexing its parent directory. Returns its stable catalog ID."
        }
        "render_preview" => {
            "Render current recipe as an image, bounded by max_px (1..4096), without upscaling."
        }
        "describe_image" => {
            "Metadata, stored faces and deterministic quality measurements; caption remains an explicit placeholder."
        }
        _ => name,
    }
}

/// Validate with the same serde declarations used by the schema derives, then
/// let the original engine deserializer validate custom IDs and invariants.
pub(crate) fn validate(name: &str, input: &Value) -> Result<(), String> {
    if let Some(index) = engine_api::tools::ToolCall::NAMES
        .iter()
        .position(|n| *n == name)
    {
        return wire::validate_engine(index, input.clone()).map_err(|e| e.to_string());
    }
    fn parse<T: serde::de::DeserializeOwned>(value: &Value) -> Result<(), String> {
        serde_json::from_value::<T>(value.clone())
            .map(|_| ())
            .map_err(|e| e.to_string())
    }
    match name {
        "open_image" => parse::<OpenImage>(input),
        "render_preview" => parse::<RenderPreview>(input),
        "list_images" => parse::<ListImages>(input),
        "describe_image" => parse::<DescribeImage>(input),
        _ => Err(format!("unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_flattened_wire_type_accepts_envelope_and_rejects_unknown_fields() {
        let args = json!({"image":"1".repeat(32),"exposure":0.5,"rationale":"lift","group":4});
        assert!(serde_json::from_value::<wire::SetTone>(args.clone()).is_ok());
        let mut bad = args;
        bad["exposuer"] = json!(1);
        assert!(serde_json::from_value::<wire::SetTone>(bad).is_err());
    }
}
