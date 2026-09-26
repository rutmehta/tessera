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

/// `describe_document`: layers, sizes, blend modes, history, selection,
/// a composite preview and layer thumbnails.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct DescribeDocument {
    /// Document.
    pub document: u64,
    /// Long edge of the composite preview (1..=8192, never upscaled).
    #[serde(default = "default_describe")]
    pub max_px: u32,
    /// Long edge of layer thumbnails (1..=1024); `null` = no thumbnails.
    #[serde(default = "default_thumb")]
    pub thumbnail_px: Option<u32>,
}
fn default_describe() -> u32 {
    512
}
fn default_thumb() -> Option<u32> {
    Some(96)
}
/// `render_document_preview`: the composite through the GPU-resident renderer.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RenderDocumentPreview {
    /// Document.
    pub document: u64,
    /// Long edge (1..=8192, never upscaled).
    #[serde(default = "default_max")]
    pub max_px: u32,
}
/// `actions_record`: start recording executed tool calls as an Action.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionsRecord {
    /// Action name.
    pub name: String,
}
/// `actions_stop`: stop recording; returns the action and optionally
/// writes it as a `.tessera-action` file.
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionsStop {
    /// Absolute output path ending in `.tessera-action`.
    #[serde(default)]
    pub path: Option<String>,
}
/// `actions_play`: replay an action on open documents (one per input).
#[derive(Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActionsPlay {
    /// Absolute path of a `.tessera-action` file (or give `action`).
    #[serde(default)]
    pub path: Option<String>,
    /// Inline action object (the file's JSON).
    #[serde(default)]
    pub action: Option<Value>,
    /// Documents bound to the action's inputs, in order.
    #[serde(default)]
    pub documents: Vec<u64>,
}

/// Extra (non-engine) tools, in list order.
pub(crate) const EXTRA: [&str; 9] = [
    "open_image",
    "render_preview",
    "list_images",
    "describe_image",
    "describe_document",
    "render_document_preview",
    "actions_record",
    "actions_stop",
    "actions_play",
];

pub fn tools() -> Vec<Value> {
    let mut schemas = wire::engine_schemas();
    schemas.extend(wire::document_schemas());
    schemas.extend([
        serde_json::to_value(schemars::schema_for!(OpenImage)).unwrap(),
        serde_json::to_value(schemars::schema_for!(RenderPreview)).unwrap(),
        serde_json::to_value(schemars::schema_for!(ListImages)).unwrap(),
        serde_json::to_value(schemars::schema_for!(DescribeImage)).unwrap(),
        serde_json::to_value(schemars::schema_for!(DescribeDocument)).unwrap(),
        serde_json::to_value(schemars::schema_for!(RenderDocumentPreview)).unwrap(),
        serde_json::to_value(schemars::schema_for!(ActionsRecord)).unwrap(),
        serde_json::to_value(schemars::schema_for!(ActionsStop)).unwrap(),
        serde_json::to_value(schemars::schema_for!(ActionsPlay)).unwrap(),
    ]);
    schemas.extend(crate::documents::local_tools::schemas());
    engine_api::tools::ToolCall::NAMES
        .into_iter()
        .chain(engine_api::tools::DocumentToolCall::NAMES)
        .chain(EXTRA)
        .chain(crate::documents::local_tools::NAMES)
        .zip(schemas)
        .map(|(name, schema)| {
            json!({"name":name,"description":description(name),"inputSchema":schema})
        })
        .collect()
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
        "open_document" => {
            "Open a .tessera-doc, PSD/PSB, JPEG or PNG as a layered document. Returns its session id."
        }
        "add_layer" => {
            "Add a pixel layer, group or solid-colour fill layer. One Agent history entry."
        }
        "set_layer_props" => {
            "Change name, visibility, opacity, fill, blend mode, clipping or colour tag. One Agent history entry."
        }
        "paint_stroke" => {
            "Paint or erase one brush stroke (points + brush parameters, never pixels) on a layer or its mask, limited by the selection. One Agent history entry."
        }
        "set_pixel_selection" => {
            "Replace/add/subtract/intersect the pixel selection with a marquee, ellipse, polygon, layer transparency or saved selection; optional feather and save_as. One Agent history entry."
        }
        "apply_adjustment_layer" => {
            "Add a non-destructive adjustment layer (levels, curves, hue/saturation, exposure, invert, posterize, threshold, channel mixer), masked by the selection by default. One Agent history entry."
        }
        "transform_layer" => {
            "Affine-transform a pixel layer (resampled) or a smart object (placement). One Agent history entry."
        }
        "merge_down" => "Merge a layer into the pixel layer below it. One Agent history entry.",
        "export_document" => {
            "Write the document as .tessera-doc, PSD/PSB or a flattened JPEG/PNG/TIFF. Completes before returning."
        }
        "list_layers" => "The layer tree, bottom to top, every group before its children.",
        "describe_document" => {
            "Layers (kinds, sizes, blend modes), history with rationales, selection, a composite preview and layer thumbnails."
        }
        "render_document_preview" => {
            "The document composite rendered by the GPU-resident compositor at the pyramid level fitting max_px."
        }
        "actions_record" => "Start recording executed tool calls as an Action (spec 02 §11).",
        "actions_stop" => {
            "Stop recording; returns the .tessera-action JSON and optionally writes it."
        }
        "actions_play" => {
            "Replay a recorded Action on open documents (one per input); each step is an Agent history entry."
        }
        _ => name,
    }
}

/// Rejects argument keys the derived schema does not name (including keys of
/// flattened members, which serde cannot deny).
fn known_keys(schema: &Value, input: &Value) -> Result<(), String> {
    fn collect<'a>(node: &'a Value, root: &'a Value, out: &mut Vec<&'a str>, depth: usize) {
        if depth > 8 {
            return;
        }
        if let Some(r) = node.get("$ref").and_then(Value::as_str)
            && let Some(name) = r.strip_prefix("#/$defs/")
            && let Some(def) = root.get("$defs").and_then(|d| d.get(name))
        {
            collect(def, root, out, depth + 1);
        }
        if let Some(props) = node.get("properties").and_then(Value::as_object) {
            out.extend(props.keys().map(String::as_str));
        }
        for key in ["allOf", "anyOf", "oneOf"] {
            for sub in node
                .get(key)
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                collect(sub, root, out, depth + 1);
            }
        }
    }
    let mut keys = Vec::new();
    collect(schema, schema, &mut keys, 0);
    for key in input.as_object().into_iter().flat_map(|o| o.keys()) {
        if !keys.contains(&key.as_str()) {
            return Err(format!("unknown field `{key}`"));
        }
    }
    Ok(())
}

/// Validate with the same serde declarations used by the schema derives, then
/// let the original engine deserializer validate custom IDs and invariants.
pub(crate) fn validate(name: &str, input: &Value) -> Result<(), String> {
    if crate::documents::local_tools::NAMES.contains(&name) {
        return crate::documents::local_tools::validate(name, input);
    }
    if let Some(index) = engine_api::tools::ToolCall::NAMES
        .iter()
        .position(|n| *n == name)
    {
        return wire::validate_engine(index, input.clone()).map_err(|e| e.to_string());
    }
    if let Some(index) = engine_api::tools::DocumentToolCall::NAMES
        .iter()
        .position(|n| *n == name)
    {
        wire::validate_document(index, input.clone()).map_err(|e| e.to_string())?;
        return known_keys(&wire::document_schemas()[index], input);
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
        "describe_document" => parse::<DescribeDocument>(input),
        "render_document_preview" => parse::<RenderDocumentPreview>(input),
        "actions_record" => parse::<ActionsRecord>(input),
        "actions_stop" => parse::<ActionsStop>(input),
        "actions_play" => parse::<ActionsPlay>(input),
        _ => Err(format!("unknown tool: {name}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn document_wire_types_accept_envelope_flatten_and_reject_typos() {
        let ok = [
            (
                "set_layer_props",
                json!({"document":1,"layer":2,"opacity":0.5,"rationale":"r","expect_head":null}),
            ),
            (
                "set_pixel_selection",
                json!({"document":1,"shape":"rect","rect":{"x0":0,"y0":0,"x1":4,"y1":4},"mode":"add","expect_head":3}),
            ),
            (
                "paint_stroke",
                json!({"document":1,"layer":2,"points":[{"x":1.0,"y":2.0}],"brush":{"size":5.0}}),
            ),
            (
                "apply_adjustment_layer",
                json!({"document":1,"adjustment":{"kind":"invert"}}),
            ),
            (
                "transform_layer",
                json!({"document":1,"layer":2,"transform":[1.0,0.0,3.0,0.0,1.0,4.0]}),
            ),
            (
                "export_document",
                json!({"document":1,"settings":{"path":"/tmp/x.png","format":{"container":"image","encoding":{"format":"png","bit_depth":8}}}}),
            ),
        ];
        for (name, args) in ok {
            validate(name, &args).unwrap_or_else(|e| panic!("{name}: {e}"));
            let mut tagged = args.clone();
            tagged["tool"] = json!(name);
            serde_json::from_value::<engine_api::tools::DocumentToolRequest>(tagged).unwrap();
        }
        let bad = [
            (
                "set_layer_props",
                json!({"document":1,"layer":2,"opacty":0.5}),
            ),
            (
                "set_pixel_selection",
                json!({"document":1,"shape":"rect","rect":{"x0":0,"y0":0,"x1":4,"y1":4},"moed":"add"}),
            ),
            (
                "add_layer",
                json!({"document":1,"layer":{"kind":"pixel"},"typo":1}),
            ),
            ("paint_stroke", json!({"document":1,"layer":2})),
            ("merge_down", json!({"document":"one","layer":2})),
        ];
        for (name, args) in bad {
            assert!(validate(name, &args).is_err(), "{name} accepted {args}");
        }
    }

    #[test]
    fn derived_flattened_wire_type_accepts_envelope_and_rejects_unknown_fields() {
        let args = json!({"image":"1".repeat(32),"exposure":0.5,"rationale":"lift","group":4});
        assert!(serde_json::from_value::<wire::SetTone>(args.clone()).is_ok());
        let mut bad = args;
        bad["exposuer"] = json!(1);
        assert!(serde_json::from_value::<wire::SetTone>(bad).is_err());
    }
}
