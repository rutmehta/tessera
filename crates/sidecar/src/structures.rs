//! Structured settings with explicit, granular native RDF extensions.
//!
//! Interoperability boundary: ExifTool documents PointColors and RetouchInfo as
//! string sequences, but not their string grammar. OkLCh samples and native
//! retouch targets have no established Adobe equivalent. Their resource items
//! below are Tessera extensions, NOT Adobe-compatible point colours/healing.
//! LensBlur uses the documented Active/BlurAmount fields; normalized depth,
//! arbitrary bokeh names and model identities remain in ts fields. We deliberately
//! do not guess Adobe's four-value FocalRange or numeric BokehShape semantics.
//! No opaque recipe JSON, invented CRS subfields, or cached pixels are emitted.
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
            for k in ["focus_range", "bokeh", "depth_model"] {
                if node(tree, n, PRIVATE, k).is_some() {
                    v[k] = native_field(tree, n, k)?;
                }
            }
            // Adobe depth/shape semantics are not established; do not silently
            // replace an externally specified focus or shape with native defaults.
            if scalar(tree, n, CRS, "FocalRange").is_some()
                || scalar(tree, n, CRS, "BokehShape").is_some()
            {
                return Err(error(
                    "Adobe LensBlur focal range/bokeh shape cannot be translated losslessly",
                ));
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
            let mut values = Vec::new();
            for item in tree.items(n) {
                if key == CrsKey::PointColors {
                    values.push(read_native(tree, item)?);
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
