//! Mask-group translation. CRS geometry/adjustments are interoperable; native-only
//! semantics live in individually named `ts:` fields, never a recipe JSON shadow.
//!
//! Adobe's dab payloads, colour sample colour space and AI raster/model formats
//! are not specified by ExifTool's tag table. We deliberately do not invent those
//! encodings: native strokes, OkLab samples and AI prompts/models use typed RDF
//! extensions. Other consumers cannot render those placeholders. Foreign opaque
//! masks fail atomically so the caller can retain their original XMP untouched.
use crate::xml::*;
use engine_api::error::EngineResult;
use engine_api::recipe::{LocalAdjustment, MaskComponent};
use serde_json::{Value, json};

const PARAMS: &[(&str, &str)] = &[
    ("LocalExposure2012", "exposure"),
    ("LocalContrast2012", "contrast"),
    ("LocalHighlights2012", "highlights"),
    ("LocalShadows2012", "shadows"),
    ("LocalWhites2012", "whites"),
    ("LocalBlacks2012", "blacks"),
    ("LocalTemperature", "temperature"),
    ("LocalTint", "tint"),
    ("LocalHue", "hue"),
    ("LocalSaturation", "saturation"),
    ("LocalTexture", "texture"),
    ("LocalClarity2012", "clarity"),
    ("LocalDehaze", "dehaze"),
    ("LocalSharpness", "sharpness"),
    ("LocalLuminanceNoise", "noise"),
    ("LocalMoire", "moire"),
    ("LocalDefringe", "defringe"),
];
fn resource<'a>(t: &'a Tree, n: &'a Node) -> &'a Node {
    n.children
        .iter()
        .map(|i| &t.nodes[*i])
        .find(|n| n.ns == RDF && n.local == "Description")
        .unwrap_or(n)
}
fn child<'a>(t: &'a Tree, n: &'a Node, ns: &str, name: &str) -> Option<&'a Node> {
    resource(t, n)
        .children
        .iter()
        .map(|i| &t.nodes[*i])
        .find(|c| c.ns == ns && c.local == name)
}
fn get(t: &Tree, n: &Node, ns: &str, name: &str) -> Option<String> {
    resource(t, n)
        .attrs
        .iter()
        .find(|a| a.ns == ns && a.local == name)
        .map(|a| a.value.clone())
        .or_else(|| child(t, n, ns, name).map(|c| c.text.clone()))
}
fn number(s: &str) -> EngineResult<f64> {
    let f: f64 = s.trim().parse().map_err(error)?;
    if !f.is_finite() {
        return Err(error("nonfinite mask number"));
    }
    Ok(f)
}
fn num(t: &Tree, n: &Node, name: &str, default: f64) -> EngineResult<f64> {
    get(t, n, CRS, name).map_or(Ok(default), |s| number(&s))
}
fn flag(s: Option<String>, default: bool) -> EngineResult<bool> {
    match s.as_deref().map(str::trim) {
        None => Ok(default),
        Some("true" | "True" | "1") => Ok(true),
        Some("false" | "False" | "0") => Ok(false),
        _ => Err(error("invalid mask boolean")),
    }
}
fn seq(name: &str, body: &str) -> String {
    format!("<{name}><rdf:Seq>{body}</rdf:Seq></{name}>")
}
fn structure(name: &str, body: &str) -> String {
    format!("<{name} rdf:parseType=\"Resource\">{body}</{name}>")
}
fn scalar(name: &str, v: &Value) -> String {
    text(
        name,
        &v.as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| v.to_string()),
    )
}
// Typed, field-level RDF for native data only. Array order and null versus empty
// remain explicit; property names come from the statically defined recipe types.
fn native(name: &str, v: &Value) -> String {
    let (ty, body) = match v {
        Value::Null => ("null", String::new()),
        Value::Bool(_) => ("bool", v.to_string()),
        Value::Number(_) => ("number", v.to_string()),
        Value::String(s) => ("string", escape(s)),
        Value::Array(a) => (
            "array",
            format!(
                "<rdf:Seq>{}</rdf:Seq>",
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
    let resource = if v.is_object() {
        " rdf:parseType=\"Resource\""
    } else {
        ""
    };
    format!("<{name} ts:type=\"{ty}\"{resource}>{body}</{name}>")
}
fn native_value(t: &Tree, n: &Node) -> EngineResult<Value> {
    match get(t, n, PRIVATE, "type").as_deref() {
        Some("null") => Ok(Value::Null),
        Some("bool") => Ok(json!(flag(Some(n.text.clone()), false)?)),
        Some("number") => {
            // Preserve integers (notably stable IDs) without routing through f64.
            let v: Value = serde_json::from_str(n.text.trim()).map_err(error)?;
            if !v.is_number() {
                return Err(error("invalid native number"));
            }
            Ok(v)
        }
        Some("string") => Ok(json!(n.text)),
        Some("array") => Ok(Value::Array(
            t.items(n)
                .into_iter()
                .map(|n| native_value(t, n))
                .collect::<EngineResult<_>>()?,
        )),
        Some("object") => {
            let mut map = serde_json::Map::new();
            for c in n
                .children
                .iter()
                .map(|i| &t.nodes[*i])
                .filter(|c| c.ns == PRIVATE)
            {
                if map.insert(c.local.clone(), native_value(t, c)?).is_some() {
                    return Err(error("duplicate native mask field"));
                }
            }
            Ok(Value::Object(map))
        }
        _ => Err(error("invalid native mask field type")),
    }
}
fn extension(t: &Tree, n: &Node, name: &str) -> EngineResult<Option<Value>> {
    child(t, n, PRIVATE, name)
        .map(|n| native_value(t, n))
        .transpose()
}
fn required_extension(t: &Tree, n: &Node, name: &str) -> EngineResult<Value> {
    extension(t, n, name)?.ok_or_else(|| error(format!("opaque mask requires ts:{name}")))
}

pub(super) fn export_masks(v: &Value) -> EngineResult<String> {
    // Validate the shape before indexing; no null/default coercion of invalid input.
    let locals: Vec<LocalAdjustment> = serde_json::from_value(v.clone())?;
    let mut groups = String::new();
    for local in locals {
        let v = serde_json::to_value(local)?;
        let mut b = text("crs:What", "Correction")
            + &scalar("crs:CorrectionName", &v["name"])
            + &scalar("crs:CorrectionActive", &v["enabled"])
            + &text(
                "crs:CorrectionAmount",
                &(v["amount"].as_f64().ok_or_else(|| error("amount"))? / 100.0).to_string(),
            )
            + &native("ts:LocalId", &v["id"])
            + &native("ts:CompositeInverted", &v["invert"]);
        for (crs, key) in PARAMS {
            b += &scalar(&format!("crs:{crs}"), &v["params"][*key]);
        }
        if let Some(a) = v["params"]["color_overlay"].as_array() {
            b += &scalar("crs:LocalToningHue", &a[0]);
            b += &scalar("crs:LocalToningSaturation", &a[1]);
        }
        let mut components = String::new();
        for c in v["components"]
            .as_array()
            .ok_or_else(|| error("components"))?
        {
            components += &structure("rdf:li", &export_component(c)?);
        }
        b += &seq("crs:CorrectionMasks", &components);
        groups += &structure("rdf:li", &b);
    }
    Ok(seq("crs:MaskGroupBasedCorrections", &groups))
}
fn export_component(c: &Value) -> EngineResult<String> {
    let kind = c["kind"].as_str().ok_or_else(|| error("mask kind"))?;
    let blend = match c["combine"].as_str() {
        Some("add") => "0",
        Some("subtract") => "1",
        Some("intersect") => "2",
        _ => return Err(error("unknown mask combination")),
    };
    let mut b = text("crs:MaskActive", "true")
        + &scalar("crs:MaskInverted", &c["invert"])
        + &text("crs:MaskBlendMode", blend);
    match kind {
        "linear" => {
            b += &text("crs:What", "Mask/Gradient");
            for (name, key, i) in [
                ("FullX", "start", 0),
                ("FullY", "start", 1),
                ("ZeroX", "end", 0),
                ("ZeroY", "end", 1),
            ] {
                b += &scalar(&format!("crs:{name}"), &c[key][i]);
            }
        }
        "radial" => {
            b += &text("crs:What", "Mask/CircularGradient");
            // Calculate bounds in f64 from exact f32 values. Retain native center
            // and radii too: cancellation can erase a tiny radius at a large center.
            for (name, i, sign) in [
                ("Left", 0, -1.0),
                ("Right", 0, 1.0),
                ("Top", 1, -1.0),
                ("Bottom", 1, 1.0),
            ] {
                let x = c["center"][i].as_f64().ok_or_else(|| error("center"))?;
                let r = c["radii"][i].as_f64().ok_or_else(|| error("radii"))?;
                b += &text(&format!("crs:{name}"), &(x + sign * r).to_string());
            }
            b += &scalar("crs:Angle", &c["angle"]);
            b += &scalar("crs:Feather", &c["feather"]);
            b += &native("ts:center", &c["center"]);
            b += &native("ts:radii", &c["radii"]);
        }
        "luminance_range" | "color_range" | "depth" => {
            // Type numeric codes / opaque LumRange sample payloads aren't defined
            // in our reference. Identify the native kind privately instead.
            b += &native("ts:kind", &c["kind"]);
            let mut r = String::new();
            if kind == "color_range" {
                r += &scalar("crs:ColorAmount", &c["amount"]);
                b += &native("ts:samples", &c["samples"]);
            } else {
                let (lo, hi, feather, key) = if kind == "depth" {
                    ("DepthMin", "DepthMax", "DepthFeather", "feather")
                } else {
                    ("LumMin", "LumMax", "LumFeather", "smoothness")
                };
                r += &scalar(&format!("crs:{lo}"), &c["range"][0]);
                r += &scalar(&format!("crs:{hi}"), &c["range"][1]);
                r += &scalar(&format!("crs:{feather}"), &c[key]);
            }
            b += &structure("crs:CorrectionRangeMask", &r);
            if kind == "depth" {
                b += &native("ts:model", &c["model"]);
            }
        }
        "brush" => {
            b += &native("ts:kind", &c["kind"]);
            let mut strokes = String::new();
            for s in c["strokes"].as_array().ok_or_else(|| error("strokes"))? {
                let body = scalar("crs:Radius", &s["radius"])
                    + &scalar("crs:Flow", &s["flow"])
                    + &native("ts:feather", &s["feather"])
                    + &native("ts:erase", &s["erase"])
                    + &native("ts:points", &s["points"]);
                strokes += &structure("rdf:li", &body);
            }
            b += &seq("ts:strokes", &strokes);
        }
        "subject" | "sky" | "background" | "person" | "object" | "landscape" => {
            b += &native("ts:kind", &c["kind"]);
            // A selection request, not a serialized Adobe segmentation raster.
            b += &native("ts:model", &c["model"]);
            let keys: &[&str] = match kind {
                "person" => &["person", "parts"],
                "object" => &["prompt", "region", "points"],
                "landscape" => &["class"],
                _ => &[],
            };
            for key in keys {
                b += &native(&format!("ts:{key}"), &c[*key]);
            }
        }
        _ => return Err(error(format!("unsupported mask kind {kind}"))),
    }
    Ok(b)
}

pub(super) fn import_masks(t: &Tree) -> EngineResult<Value> {
    let Some(Property::Node(root)) = t.property(CRS, "MaskGroupBasedCorrections") else {
        return Err(error("masks require a sequence"));
    };
    let mut locals = Vec::new();
    let mut used = std::collections::BTreeSet::new();
    // Reserve native IDs before assigning IDs to foreign corrections.
    for n in t.items(root) {
        if let Some(id) = extension(t, n, "LocalId")? {
            let id = id.as_u64().ok_or_else(|| error("invalid local id"))?;
            if !used.insert(id) {
                return Err(error("duplicate local id"));
            }
        }
    }
    let mut next = 0u64;
    for n in t.items(root) {
        let mut v = serde_json::to_value(LocalAdjustment::default())?;
        v["id"] = if let Some(id) = extension(t, n, "LocalId")? {
            id
        } else {
            while used.contains(&next) {
                next = next
                    .checked_add(1)
                    .ok_or_else(|| error("mask id overflow"))?;
            }
            used.insert(next);
            json!(next)
        };
        v["name"] = json!(get(t, n, CRS, "CorrectionName").unwrap_or_default());
        v["enabled"] = json!(flag(get(t, n, CRS, "CorrectionActive"), true)?);
        v["amount"] = json!(num(t, n, "CorrectionAmount", 1.0)? * 100.0);
        if let Some(invert) = extension(t, n, "CompositeInverted")? {
            v["invert"] = invert;
        }
        for (crs, key) in PARAMS {
            if let Some(s) = get(t, n, CRS, crs) {
                v["params"][*key] = json!(number(&s)?);
            }
        }
        let hue = get(t, n, CRS, "LocalToningHue");
        let sat = get(t, n, CRS, "LocalToningSaturation");
        if hue.is_some() || sat.is_some() {
            v["params"]["color_overlay"] = json!([
                number(hue.as_deref().unwrap_or("0"))?,
                number(sat.as_deref().unwrap_or("0"))?
            ]);
        }
        let mut components = Vec::new();
        if let Some(masks) = child(t, n, CRS, "CorrectionMasks") {
            for c in t.items(masks) {
                // There is no per-component enabled bit in the recipe. Do not
                // silently drop it and later overwrite a foreign disabled mask.
                if !flag(get(t, c, CRS, "MaskActive"), true)? {
                    return Err(error("disabled mask component retained in XMP"));
                }
                components.push(import_component(t, c)?);
            }
        }
        if child(t, n, CRS, "CorrectionRangeMask").is_some() {
            return Err(error("group-level range constraint retained in XMP"));
        }
        v["components"] = json!(components);
        let local: LocalAdjustment = serde_json::from_value(v)?;
        locals.push(local);
    }
    Ok(serde_json::to_value(locals)?)
}
fn import_component(t: &Tree, n: &Node) -> EngineResult<Value> {
    let what = get(t, n, CRS, "What").unwrap_or_default();
    let native_kind = extension(t, n, "kind")?;
    let kind = native_kind
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or(match what.as_str() {
            "Mask/Gradient" => "linear",
            "Mask/CircularGradient" => "radial",
            "Mask/Sky" => "sky",
            "Mask/Subject" => "subject",
            "Mask/Background" => "background",
            // The geometry of Adobe paint dabs is intentionally not guessed.
            _ => "unsupported",
        });
    let mut c = json!({"kind":kind});
    match kind {
        "linear" => {
            c["start"] = json!([num(t, n, "FullX", 0.0)?, num(t, n, "FullY", 0.0)?]);
            c["end"] = json!([num(t, n, "ZeroX", 1.0)?, num(t, n, "ZeroY", 1.0)?]);
        }
        "radial" => {
            let (l, r, top, bottom) = (
                num(t, n, "Left", 0.0)?,
                num(t, n, "Right", 1.0)?,
                num(t, n, "Top", 0.0)?,
                num(t, n, "Bottom", 1.0)?,
            );
            c["center"] = json!([(l + r) / 2.0, (top + bottom) / 2.0]);
            c["radii"] = json!([(r - l) / 2.0, (bottom - top) / 2.0]);
            if let (Some(center), Some(radii)) =
                (extension(t, n, "center")?, extension(t, n, "radii")?)
            {
                let cv: [f32; 2] = serde_json::from_value(center.clone())?;
                let rv: [f32; 2] = serde_json::from_value(radii.clone())?;
                // Trust cancellation-preserving coordinates only while the CRS
                // bounds still agree. An external geometry edit always wins.
                if [l, r, top, bottom]
                    == [
                        f64::from(cv[0]) - f64::from(rv[0]),
                        f64::from(cv[0]) + f64::from(rv[0]),
                        f64::from(cv[1]) - f64::from(rv[1]),
                        f64::from(cv[1]) + f64::from(rv[1]),
                    ]
                {
                    c["center"] = center;
                    c["radii"] = radii;
                }
            }
            c["angle"] = json!(num(t, n, "Angle", 0.0)?);
            c["feather"] = json!(num(t, n, "Feather", 0.0)?);
            if get(t, n, CRS, "Flipped").is_some() {
                return Err(error("radial Flipped semantics retained in XMP"));
            }
        }
        "brush" => {
            let root =
                child(t, n, PRIVATE, "strokes").ok_or_else(|| error("missing native strokes"))?;
            let mut strokes = Vec::new();
            for s in t.items(root) {
                strokes.push(json!({
                    "radius":num(t,s,"Radius",0.02)?, "flow":num(t,s,"Flow",100.0)?,
                    "feather":required_extension(t,s,"feather")?,
                    "erase":required_extension(t,s,"erase")?,
                    "points":required_extension(t,s,"points")?
                }));
            }
            c["strokes"] = json!(strokes);
        }
        "luminance_range" | "color_range" | "depth" => {
            let r = child(t, n, CRS, "CorrectionRangeMask")
                .ok_or_else(|| error("missing range mask"))?;
            if kind == "color_range" {
                c["amount"] = json!(num(t, r, "ColorAmount", 0.0)?);
                c["samples"] = required_extension(t, n, "samples")?;
            } else {
                let (lo, hi, feather, key) = if kind == "depth" {
                    ("DepthMin", "DepthMax", "DepthFeather", "feather")
                } else {
                    ("LumMin", "LumMax", "LumFeather", "smoothness")
                };
                c["range"] = json!([num(t, r, lo, 0.0)?, num(t, r, hi, 1.0)?]);
                c[key] = json!(num(t, r, feather, 0.0)?);
            }
            if kind == "depth" {
                c["model"] = extension(t, n, "model")?.unwrap_or(Value::Null);
            }
        }
        "subject" | "sky" | "background" | "person" | "object" | "landscape" => {
            c["model"] = extension(t, n, "model")?.unwrap_or(Value::Null);
            let keys: &[&str] = match kind {
                "person" => &["person", "parts"],
                "object" => &["prompt", "region", "points"],
                "landscape" => &["class"],
                _ => &[],
            };
            for key in keys {
                c[*key] = required_extension(t, n, key)?;
            }
        }
        _ => return Err(error(format!("unsupported mask kind {what}"))),
    }
    if !matches!(kind, "luminance_range" | "color_range" | "depth")
        && child(t, n, CRS, "CorrectionRangeMask").is_some()
    {
        return Err(error("nested range constraint retained in XMP"));
    }
    if child(t, n, CRS, "Masks").is_some() {
        return Err(error("nested mask group retained in XMP"));
    }
    c["combine"] = json!(
        match get(t, n, CRS, "MaskBlendMode").as_deref().map(str::trim) {
            None | Some("0" | "Add") => "add",
            Some("1" | "Subtract") => "subtract",
            Some("2" | "Intersect") => "intersect",
            _ => return Err(error("unsupported mask blend mode")),
        }
    );
    c["invert"] = json!(flag(get(t, n, CRS, "MaskInverted"), false)?);
    let c: MaskComponent = serde_json::from_value(c)?;
    Ok(serde_json::to_value(c)?)
}
