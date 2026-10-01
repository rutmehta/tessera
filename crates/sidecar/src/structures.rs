//! Structured settings with explicit, granular native RDF extensions.
//!
//! Interoperability boundary: ExifTool documents PointColors and RetouchInfo as
//! string sequences. PointColors decoding additionally supports the 19-number
//! grammar and SDK resource fields (see point_colors.rs). OkLCh samples and native
//! retouch targets have no established Adobe equivalent. Their resource items
//! below are Tessera extensions, NOT Adobe-compatible point colours/healing.
//! LensBlur uses the documented Active/BlurAmount fields; normalized depth,
//! arbitrary bokeh names and model identities remain in ts fields. We deliberately
//! interpret Adobe controls approximately; import-lrcat retains exact source and diagnostics.
//! No opaque recipe JSON, invented CRS subfields, or cached pixels are emitted.
#[path = "point_colors.rs"]
mod point_colors;
use super::resource;
use crate::xml::*;
use engine_api::{error::EngineResult, recipe::CrsKey};
use serde_json::{Map, Value, json};

fn node<'a>(tree: &'a Tree, n: &'a Node, ns: &str, name: &str) -> Option<&'a Node> {
    resource(tree, n)
        .children
        .iter()
        .map(|i| &tree.nodes[*i])
        .find(|c| c.ns == ns && c.local == name)
}
fn scalar(tree: &Tree, n: &Node, ns: &str, name: &str) -> Option<String> {
    let n = resource(tree, n);
    n.attrs
        .iter()
        .find(|a| a.ns == ns && a.local == name)
        .map(|a| a.value.clone())
        .or_else(|| node(tree, n, ns, name).map(|c| c.text.clone()))
}
fn wrap(name: &str, body: &str) -> String {
    format!("<{name} rdf:parseType=\"Resource\">{body}</{name}>")
}

// Each native field is independently addressable XML. Type tags distinguish
// null/empty lists/empty objects and preserve strings without whitespace loss.
fn native(name: &str, v: &Value) -> String {
    let (kind, body) = match v {
        Value::Null => ("null", String::new()),
        Value::Bool(b) => ("boolean", text("ts:Value", &b.to_string())),
        Value::Number(n) => ("number", text("ts:Value", &n.to_string())),
        Value::String(s) => ("string", text("ts:Value", s)),
        Value::Array(a) => (
            "array",
            format!(
                "<ts:Items><rdf:Seq>{}</rdf:Seq></ts:Items>",
                a.iter().map(|v| native("rdf:li", v)).collect::<String>()
            ),
        ),
        Value::Object(o) => (
            "object",
            o.iter()
                .map(|(k, v)| native(&format!("ts:{k}"), v))
                .collect(),
        ),
    };
    wrap(name, &(text("ts:Type", kind) + &body))
}
fn read_native(tree: &Tree, n: &Node) -> EngineResult<Value> {
    let n = resource(tree, n);
    let kind =
        scalar(tree, n, PRIVATE, "Type").ok_or_else(|| error("missing native field type"))?;
    let value =
        || scalar(tree, n, PRIVATE, "Value").ok_or_else(|| error("missing native field value"));
    Ok(match kind.as_str() {
        "null" => Value::Null,
        "boolean" => json!(value()?.parse::<bool>().map_err(error)?),
        "number" => {
            let s = value()?;
            if s.contains(['.', 'e', 'E']) {
                let n = s.parse::<f64>().map_err(error)?;
                if !n.is_finite() {
                    return Err(error("nonfinite native number"));
                }
                json!(n)
            } else {
                Value::Number(s.parse::<serde_json::Number>().map_err(error)?)
            }
        }
        "string" => json!(value()?),
        "array" => {
            let items =
                node(tree, n, PRIVATE, "Items").ok_or_else(|| error("missing native array"))?;
            Value::Array(
                tree.items(items)
                    .iter()
                    .map(|i| read_native(tree, i))
                    .collect::<EngineResult<_>>()?,
            )
        }
        "object" => {
            let mut o = Map::new();
            for c in n
                .children
                .iter()
                .map(|i| &tree.nodes[*i])
                .filter(|c| c.ns == PRIVATE && c.local != "Type")
            {
                if o.insert(c.local.clone(), read_native(tree, c)?).is_some() {
                    return Err(error("duplicate native field"));
                }
            }
            Value::Object(o)
        }
        _ => return Err(error("unknown native field type")),
    })
}
fn native_field(tree: &Tree, n: &Node, name: &str) -> EngineResult<Value> {
    read_native(
        tree,
        node(tree, n, PRIVATE, name).ok_or_else(|| error(format!("missing ts:{name}")))?,
    )
}
fn real(tree: &Tree, n: &Node, name: &str, default: f64) -> EngineResult<Value> {
    let v = scalar(tree, n, CRS, name)
        .map(|s| s.trim().parse::<f64>().map_err(error))
        .transpose()?
        .unwrap_or(default);
    if !v.is_finite() {
        return Err(error("nonfinite structure number"));
    }
    Ok(json!(v))
}

pub(super) fn encode(key: CrsKey, v: &Value) -> EngineResult<String> {
    let name = format!("crs:{}", key.xmp_name());
    match key {
        CrsKey::LensBlur => {
            if v.is_null() {
                return Ok(wrap(&name, &text("crs:Active", "False")));
            }
            let mut body =
                text("crs:Active", "True") + &text("crs:BlurAmount", &v["amount"].to_string());
            for k in ["focus_range", "bokeh", "depth_model"] {
                body += &native(&format!("ts:{k}"), &v[k]);
            }
            for k in ["focus_falloff", "adobe", "depth"] {
                if !v[k].is_null() {
                    body += &native(&format!("ts:{k}"), &v[k]);
                }
            }
            Ok(wrap(&name, &body))
        }
        CrsKey::PointColors | CrsKey::RetouchAreas | CrsKey::RetouchInfo => {
            let mut items = String::new();
            for v in v
                .as_array()
                .ok_or_else(|| error("expected structure list"))?
            {
                if key == CrsKey::PointColors {
                    items += &native("rdf:li", v);
                } else {
                    let mut body = String::new();
                    for k in ["id", "kind", "target", "enabled"] {
                        body += &native(&format!("ts:{k}"), &v[k]);
                    }
                    // These field names are documented on RetouchAreas only.
                    if key == CrsKey::RetouchAreas {
                        body += &text("crs:Opacity", &v["opacity"].to_string());
                        body += &text("crs:Feather", &v["feather"].to_string());
                    } else {
                        for k in ["opacity", "feather"] {
                            body += &native(&format!("ts:{k}"), &v[k]);
                        }
                    }
                    items += &wrap("rdf:li", &body);
                }
            }
            Ok(format!("<{name}><rdf:Seq>{items}</rdf:Seq></{name}>"))
        }
        _ => Err(error("unsupported structure key")),
    }
}

pub(super) fn decode(key: CrsKey, tree: &Tree) -> EngineResult<Value> {
    let Some(Property::Node(n)) = tree.property(CRS, key.xmp_name()) else {
        return Err(error("expected structured property"));
    };
    match key {
        CrsKey::LensBlur => {
            let active = scalar(tree, n, CRS, "Active");
            match active.as_deref().map(str::trim) {
                Some("False" | "false" | "0") => return Ok(Value::Null),
                Some("True" | "true" | "1") | None => (),
                _ => return Err(error("invalid LensBlur Active")),
            }
            let r = resource(tree, n);
            if active.is_none()
                && r.attrs.iter().all(|a| a.ns == RDF)
                && r.children.iter().all(|i| {
                    let c = &tree.nodes[*i];
                    c.ns == RDF && c.children.is_empty()
                })
            {
                return Ok(Value::Null);
            }
            let mut v = serde_json::to_value(engine_api::recipe::settings::LensBlur::default())?;
            v["amount"] = real(tree, n, "BlurAmount", 50.0)?;
            for k in ["focus_range", "focus_falloff", "bokeh", "depth_model"] {
                if node(tree, n, PRIVATE, k).is_some() {
                    v[k] = native_field(tree, n, k)?;
                }
            }
            let mut adobe = Map::new();
            for (source, target) in [
                ("BokehShape", "bokeh_shape"),
                ("BokehShapeDetail", "bokeh_shape_detail"),
                ("HighlightsBoost", "highlights_boost"),
                ("HighlightsThreshold", "highlights_threshold"),
                ("CatEyeAmount", "cat_eye_amount"),
                ("CatEyeScale", "cat_eye_scale"),
                ("BokehAspect", "bokeh_aspect"),
                ("BokehRotation", "bokeh_rotation"),
                ("SphericalAberration", "spherical_aberration"),
                ("FocalRangeSource", "focal_range_source"),
            ] {
                if scalar(tree, n, CRS, source).is_some() {
                    adobe.insert(target.into(), real(tree, n, source, 0.)?);
                }
            }
            for (source, target) in [
                ("Version", "version"),
                ("SampledArea", "sampled_area"),
                ("SampledRange", "sampled_range"),
                ("SubjectRange", "subject_range"),
            ] {
                if let Some(value) = scalar(tree, n, CRS, source) {
                    adobe.insert(target.into(), json!(value));
                }
            }
            if let Some(range) = scalar(tree, n, CRS, "FocalRange") {
                let range: Vec<f32> = range
                    .split_whitespace()
                    .map(str::parse)
                    .collect::<Result<_, _>>()
                    .map_err(error)?;
                if range.len() != 4
                    || range.iter().any(|v| !v.is_finite())
                    || range.windows(2).any(|w| w[0] > w[1])
                {
                    return Err(error("invalid LensBlur FocalRange"));
                }
                let range: Vec<_> = range.iter().map(|v| v / 100.).collect();
                v["focus_range"] = json!([range[1].clamp(0., 1.), range[2].clamp(0., 1.)]);
                v["focus_falloff"] = json!([range[1] - range[0], range[3] - range[2]]);
                adobe.insert("focal_range".into(), json!(range));
            }
            if let Some(shape) = adobe.get("bokeh_shape").and_then(Value::as_f64) {
                v["bokeh"] = json!(match shape {
                    1. => "bubble",
                    2. => "5-blade",
                    3. => "ring",
                    4. => "cat-eye",
                    _ => "circle",
                });
            }
            if !adobe.is_empty() {
                adobe.insert("active".into(), json!(true));
                v["adobe"] = Value::Object(adobe);
            }
            for k in ["adobe", "depth"] {
                if node(tree, n, PRIVATE, k).is_some() {
                    v[k] = native_field(tree, n, k)?;
                }
            }
            Ok(v)
        }
        CrsKey::PointColors | CrsKey::RetouchAreas | CrsKey::RetouchInfo => {
            // RetouchInfo is a legacy alias. An empty alias must not erase a
            // nonempty RetouchAreas value decoded earlier by the table walker.
            if key == CrsKey::RetouchInfo
                && tree.items(n).is_empty()
                && tree.property(CRS, "RetouchAreas").is_some()
            {
                return decode(CrsKey::RetouchAreas, tree);
            }
            if key == CrsKey::PointColors
                && tree
                    .items(n)
                    .iter()
                    .any(|item| !point_colors::is_native(tree, item))
            {
                point_colors::validate_list(tree, n)?;
            }
            let mut values = Vec::new();
            for item in tree.items(n) {
                if key == CrsKey::PointColors {
                    if point_colors::is_native(tree, item) {
                        values.push(read_native(tree, item)?);
                    } else if let Some(point) = point_colors::decode(tree, item)? {
                        values.push(serde_json::to_value(point)?);
                    }
                    continue;
                }
                let mut v = json!({});
                for k in ["id", "kind", "target", "enabled"] {
                    v[k] = native_field(tree, item, k)?;
                }
                if key == CrsKey::RetouchAreas {
                    v["opacity"] = real(tree, item, "Opacity", 100.0)?;
                    v["feather"] = real(tree, item, "Feather", 0.0)?;
                } else {
                    for k in ["opacity", "feather"] {
                        v[k] = native_field(tree, item, k)?;
                    }
                }
                values.push(v);
            }
            Ok(Value::Array(values))
        }
        _ => Err(error("unsupported structure key")),
    }
}
