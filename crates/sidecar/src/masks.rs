//! Mask-group translation. CRS geometry/adjustments are interoperable; native-only
//! semantics live in individually named `ts:` fields, never a recipe JSON shadow.
//!
//! Foreign dabs and color models decode with explicit approximations. The catalog
//! adapter retains exact source plus info diagnostics; native typed RDF keeps
//! its precise Tessera interpretation. Unknown/malformed payloads still fail.
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
/// Adobe part IDs are unverified: a mask carrying one is never rendered as
/// its whole category.
fn reject_part_id(t: &Tree, n: &Node) -> EngineResult<()> {
    if get(t, n, CRS, "MaskSubCategoryID").is_some_and(|part| number(&part).ok() != Some(0.)) {
        return Err(error(
            "unsupported Adobe AI mask part; part identities are unverified",
        ));
    }
    Ok(())
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
        local.validate_mask_tree()?;
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
        for key in ["curves", "curves_extended", "point_colors"] {
            if !v["params"][key].is_null() {
                b += &native(&format!("ts:{key}"), &v["params"][key]);
            }
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
    let mut b = text(
        "crs:MaskActive",
        if c["enabled"] == false {
            "false"
        } else {
            "true"
        },
    ) + &scalar("crs:MaskInverted", &c["invert"])
        + &text("crs:MaskBlendMode", blend);
    if !c["adobe_ai"].is_null() {
        b += &native("ts:adobe_ai", &c["adobe_ai"]);
    }
    if let Some(children) = c["group"].as_array() {
        b += &text("crs:What", "Mask/Group");
        let component: MaskComponent = serde_json::from_value(c.clone())?;
        b += &native("ts:GroupFallback", &serde_json::to_value(component.kind)?);
        let mut body = String::new();
        for child in children {
            body += &structure("rdf:li", &export_component(child)?);
        }
        b += &seq("crs:Masks", &body);
        return Ok(b);
    }
    if !c["luminance_domain"].is_null() {
        b += &native("ts:luminance_domain", &c["luminance_domain"]);
    }
    if !c["luminance_bounds"].is_null() {
        b += &native("ts:luminance_bounds", &c["luminance_bounds"]);
    }
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

/// The stable `LocalAdjustment::id` of every mask group in one packet, in
/// source order. A native `ts:LocalId` is kept; every other group takes the
/// lowest id that no native group and no earlier group uses. `None` when a
/// native id repeats or the id space is exhausted. Callers that need to pair a
/// recipe group with its source group use this rule, never the group's index.
pub fn assign_local_ids(native: &[Option<u64>]) -> Option<Vec<u64>> {
    let mut used = std::collections::BTreeSet::new();
    for id in native.iter().flatten() {
        if !used.insert(*id) {
            return None;
        }
    }
    let mut next = 0u64;
    native
        .iter()
        .map(|id| match id {
            Some(id) => Some(*id),
            None => {
                while used.contains(&next) {
                    next = next.checked_add(1)?;
                }
                used.insert(next);
                Some(next)
            }
        })
        .collect()
}

pub(super) fn import_masks(t: &Tree, foreign_extensions: bool) -> EngineResult<Value> {
    let Some(Property::Node(root)) = t.property(CRS, "MaskGroupBasedCorrections") else {
        return Err(error("masks require a sequence"));
    };
    let mut locals = Vec::new();
    let mut native_ids = Vec::new();
    for n in t.items(root) {
        native_ids.push(match extension(t, n, "LocalId")? {
            Some(id) => Some(id.as_u64().ok_or_else(|| error("invalid local id"))?),
            None => None,
        });
    }
    let mut seen = std::collections::BTreeSet::new();
    if native_ids.iter().flatten().any(|id| !seen.insert(*id)) {
        return Err(error("duplicate local id"));
    }
    let ids = assign_local_ids(&native_ids).ok_or_else(|| error("mask id overflow"))?;
    for (n, (id, native_id)) in t.items(root).into_iter().zip(ids.iter().zip(&native_ids)) {
        let mut v = serde_json::to_value(LocalAdjustment::default())?;
        let native_local = native_id.is_some();
        v["id"] = json!(id);
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
            let overlay = [
                number(hue.as_deref().unwrap_or("0"))?,
                number(sat.as_deref().unwrap_or("0"))?,
            ];
            // Native optional presence is lossless, including Some([0, 0]).
            // Adobe's inactive zero controls still keep the prior None shape.
            if native_local || !foreign_extensions || overlay[1] != 0. {
                v["params"]["color_overlay"] = json!(overlay);
            }
        }
        if !(0.0..=100.0).contains(&v["params"]["defringe"].as_f64().unwrap_or(f64::NAN)) {
            return Err(error("invalid local defringe"));
        }
        if let Some(a) = v["params"]["color_overlay"].as_array()
            && (!(0.0..=360.0).contains(&a[0].as_f64().unwrap_or(f64::NAN))
                || !(0.0..=100.0).contains(&a[1].as_f64().unwrap_or(f64::NAN)))
        {
            return Err(error("invalid local colour overlay"));
        }
        import_local_curves(t, n, &mut v["params"])
            .map_err(|e| error(format!("local curve: {e}")))?;
        import_local_point_colors(t, n, &mut v["params"])
            .map_err(|e| error(format!("local point colours: {e}")))?;
        for field in ["curves", "curves_extended", "point_colors"] {
            if let Some(native) = extension(t, n, field)? {
                v["params"][field] = native;
            }
        }
        let mut components = Vec::new();
        if let Some(masks) = child(t, n, CRS, "CorrectionMasks") {
            for c in t.items(masks) {
                components.push(import_component(t, c, foreign_extensions)?);
            }
        }
        if let Some(range) = child(t, n, CRS, "CorrectionRangeMask") {
            components.push(import_range(t, range, foreign_extensions)?);
        }
        v["components"] = json!(components);
        let local: LocalAdjustment = serde_json::from_value(v)?;
        local.validate_mask_tree()?;
        locals.push(local);
    }
    Ok(serde_json::to_value(locals)?)
}
fn import_component(t: &Tree, n: &Node, foreign_extensions: bool) -> EngineResult<Value> {
    let mut parent = n.parent;
    let mut depth = 0;
    while let Some(i) = parent {
        if t.nodes[i].ns == CRS && t.nodes[i].local == "Masks" {
            depth += 1;
        }
        if depth >= 8 {
            return Err(error("mask tree exceeds 8 levels"));
        }
        parent = t.nodes[i].parent;
    }
    // An individual instance selection is never widened to the whole object
    // or subject: the group stays unsupported and its source is retained.
    if ["InstanceIDs", "InstanceBounds"]
        .iter()
        .any(|key| get(t, n, CRS, key).is_some())
    {
        return Err(error("individual AI instance selection is not supported"));
    }
    let what = get(t, n, CRS, "What").unwrap_or_default();
    let native_kind = extension(t, n, "kind")?;
    let kind = native_kind
        .as_ref()
        .and_then(Value::as_str)
        .unwrap_or(match what.as_str() {
            "Mask/Group" | "Mask/Aggregate" => "group",
            "Mask/Range" | "Mask/RangeMask" => "range",
            "Mask/Gradient" => "linear",
            "Mask/CircularGradient" => "radial",
            "Mask/Sky" => "sky",
            "Mask/Subject" => "subject",
            "Mask/Background" => "background",
            "Mask/Paint" if foreign_extensions => "adobe_brush",
            "Mask/Image" | "Mask/People" | "Mask/Person" | "Mask/Object" if foreign_extensions => {
                "adobe_ai"
            }
            _ => "unsupported",
        });
    let mut c = json!({"kind":kind});
    match kind {
        "adobe_ai" => {
            let category = if let Some(category) = get(t, n, CRS, "MaskType") {
                category
            } else if what != "Mask/Image" {
                what.trim_start_matches("Mask/").to_string()
            } else {
                match get(t, n, CRS, "MaskSubType").as_deref() {
                    Some("1") => "Subject".into(),
                    Some("2") => "Sky".into(),
                    Some("3") => "People".into(),
                    Some("0")
                        if get(t, n, CRS, "ReferencePoint").is_some()
                            || get(t, n, CRS, "Left").is_some() =>
                    {
                        "Object".into()
                    }
                    _ => return Err(error("unknown Adobe AI mask subtype")),
                }
            };
            // A part ID selects a sub-region of whatever category carries it.
            // The IDs are unverified, so never widen one to the whole category.
            reject_part_id(t, n)?;
            let native_kind = match category.as_str() {
                "Subject" => "subject",
                "Sky" => "sky",
                "Background" => "background",
                "Object" | "Objects" => "object",
                _ => {
                    return Err(error(
                        "unsupported Adobe person or part mask; subtype and instance identities are unverified",
                    ));
                }
            };
            c = json!({"kind":native_kind, "model":null});
            if native_kind == "object" {
                let bounds = [
                    num(t, n, "Left", 0.)?,
                    num(t, n, "Top", 0.)?,
                    num(t, n, "Right", 1.)?,
                    num(t, n, "Bottom", 1.)?,
                ];
                if !bounds.iter().all(|v| (0.0..=1.0).contains(v))
                    || bounds[0] >= bounds[2]
                    || bounds[1] >= bounds[3]
                {
                    return Err(error("invalid object bounds"));
                }
                if get(t, n, CRS, "Left").is_some()
                    && get(t, n, CRS, "Top").is_some()
                    && get(t, n, CRS, "Right").is_some()
                    && get(t, n, CRS, "Bottom").is_some()
                {
                    c["region"] = json!({"left":bounds[0],"top":bounds[1],"right":bounds[2],"bottom":bounds[3]});
                } else if let Some(point) = get(t, n, CRS, "ReferencePoint") {
                    let point: Vec<f64> = point
                        .split_whitespace()
                        .map(number)
                        .collect::<EngineResult<_>>()?;
                    if point.len() != 2 || !point.iter().all(|v| (0.0..=1.0).contains(v)) {
                        return Err(error("invalid object reference point"));
                    }
                    c["points"] = json!([point]);
                } else {
                    return Err(error(
                        "object regeneration requires a box or reference point",
                    ));
                }
            }
            c["adobe_ai"] = json!({"category":category,"resource_id":get(t,n,CRS,"MaskDigest"),"mask_key":null,"regenerate":true});
        }
        "range" => {
            let r = child(t, n, CRS, "CorrectionRangeMask")
                .ok_or_else(|| error("missing range mask"))?;
            c = import_range(t, r, foreign_extensions)?;
        }
        "group" => {
            let masks = child(t, n, CRS, "Masks").ok_or_else(|| error("missing nested masks"))?;
            c = if let Some(fallback) = extension(t, n, "GroupFallback")? {
                let kind: engine_api::recipe::MaskKind = serde_json::from_value(fallback)?;
                serde_json::to_value(kind)?
            } else {
                json!({"kind":"brush", "strokes":[]})
            };
            c["group"] = json!(
                t.items(masks)
                    .into_iter()
                    .map(|n| import_component(t, n, foreign_extensions))
                    .collect::<EngineResult<Vec<_>>>()?
            );
        }
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
            if let Some(flipped) = get(t, n, CRS, "Flipped") {
                if !foreign_extensions {
                    return Err(error("radial Flipped semantics retained in XMP"));
                }
                let inverted = !flag(Some(flipped), true)?;
                if get(t, n, CRS, "MaskInverted").is_some()
                    && flag(get(t, n, CRS, "MaskInverted"), false)? != inverted
                {
                    return Err(error("radial Flipped conflicts with MaskInverted"));
                }
            }
        }
        "adobe_brush" => {
            c = json!({"kind":"brush", "strokes":import_dabs(t, n)?});
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
            if foreign_extensions
                && native_kind.is_none()
                && matches!(kind, "subject" | "sky" | "background")
            {
                reject_part_id(t, n)?;
                c["adobe_ai"] = json!({"category":what.trim_start_matches("Mask/"),"resource_id":get(t,n,CRS,"MaskDigest"),"mask_key":null,"regenerate":true});
            }
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
    if kind != "group" && child(t, n, CRS, "Masks").is_some() {
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
    if kind == "adobe_brush"
        && get(t, n, CRS, "MaskBlendMode").is_none()
        && num(t, n, "MaskValue", 1.)? == 0.
    {
        // A zero-value Paint removes from earlier components. Erasing an
        // isolated, initially empty native brush plane would do nothing.
        c["combine"] = json!("subtract");
    }
    c["enabled"] = json!(flag(get(t, n, CRS, "MaskActive"), true)?);
    let component_invert = if kind == "radial" && get(t, n, CRS, "Flipped").is_some() {
        !flag(get(t, n, CRS, "Flipped"), true)?
    } else {
        flag(get(t, n, CRS, "MaskInverted"), false)?
    };
    c["invert"] = json!(c["invert"].as_bool().unwrap_or(false) ^ component_invert);
    if !matches!(kind, "range" | "luminance_range" | "color_range" | "depth")
        && let Some(range) = child(t, n, CRS, "CorrectionRangeMask")
    {
        // Component inversion belongs to the seed, not its range constraint:
        // (not seed) intersect range, never not (seed intersect range).
        let combine = c["combine"].clone();
        let enabled = c["enabled"].clone();
        c["combine"] = json!("add");
        c["enabled"] = json!(true);
        let seed: MaskComponent = serde_json::from_value(c)?;
        c = json!({"kind":"brush", "strokes":[], "combine":combine,
            "enabled":enabled, "invert":false,
            "group":[seed, import_range(t, range, foreign_extensions)?]});
    }
    if let Some(state) = extension(t, n, "adobe_ai")? {
        c["adobe_ai"] = state;
    }
    if let Some(bounds) = extension(t, n, "luminance_bounds")? {
        c["luminance_bounds"] = bounds;
    }
    if let Some(domain) = extension(t, n, "luminance_domain")? {
        c["luminance_domain"] = domain;
    }
    let c: MaskComponent = serde_json::from_value(c)?;
    Ok(serde_json::to_value(c)?)
}

// Explicit scalar and four-bound luminance ranges. Type codes dispatch only
// with matching geometry; color models remain opaque until their color space
// and selection kernel can be represented faithfully.
fn import_range(t: &Tree, n: &Node, foreign_extensions: bool) -> EngineResult<Value> {
    let lum = get(t, n, CRS, "LumMin").is_some() && get(t, n, CRS, "LumMax").is_some();
    let depth = get(t, n, CRS, "DepthMin").is_some() && get(t, n, CRS, "DepthMax").is_some();
    let four = get(t, n, CRS, "LumRange");
    let subtype = get(t, n, CRS, "Type");
    if !foreign_extensions && (four.is_some() || subtype.is_some()) {
        return Err(error("opaque or ambiguous range mask retained in XMP"));
    }
    let selected = match subtype.as_deref().map(str::trim) {
        None => None,
        Some("1") => Some("color_range"),
        Some("2") => Some("luminance_range"),
        Some("3") => Some("depth"),
        _ => return Err(error("opaque or ambiguous range mask retained in XMP")),
    };
    let color =
        child(t, n, CRS, "PointModels").is_some() || child(t, n, CRS, "AreaModels").is_some();
    if color || selected == Some("color_range") {
        if !foreign_extensions
            || !color
            || lum
            || depth
            || four.is_some()
            || selected.is_some_and(|s| s != "color_range")
        {
            return Err(error("mixed or incomplete color range"));
        }
        return import_color_range(t, n);
    }
    if let Some(value) = four {
        if selected == Some("depth")
            || [
                "LumMin",
                "LumMax",
                "LumFeather",
                "DepthMin",
                "DepthMax",
                "DepthFeather",
                "ColorAmount",
            ]
            .iter()
            .any(|key| get(t, n, CRS, key).is_some())
            || child(t, n, CRS, "AreaModels").is_some()
            || child(t, n, CRS, "PointModels").is_some()
        {
            return Err(error("opaque or ambiguous range mask retained in XMP"));
        }
        let bounds = value
            .split_whitespace()
            .map(number)
            .collect::<EngineResult<Vec<_>>>()?;
        if bounds.len() != 4
            || bounds.iter().any(|v| !(0. ..=1.).contains(v))
            || bounds.windows(2).any(|p| p[0] > p[1])
        {
            return Err(error("opaque or ambiguous range mask retained in XMP"));
        }
        return Ok(
            json!({"kind":"luminance_range", "luminance_domain":"display", "range":[bounds[1],bounds[2]],
            "luminance_bounds":bounds,"combine":"intersect", "smoothness":0.,
            "invert":flag(get(t,n,CRS,"Invert"),false)?}),
        );
    }
    if lum == depth
        || selected == Some("luminance_range") && !lum
        || selected == Some("depth") && !depth
        || child(t, n, CRS, "AreaModels").is_some()
    {
        return Err(error("opaque or ambiguous range mask retained in XMP"));
    }
    let (kind, lo, hi, feather, key) = if lum {
        (
            "luminance_range",
            "LumMin",
            "LumMax",
            "LumFeather",
            "smoothness",
        )
    } else {
        ("depth", "DepthMin", "DepthMax", "DepthFeather", "feather")
    };
    let low = num(t, n, lo, 0.)?;
    let high = num(t, n, hi, 1.)?;
    let feather = num(t, n, feather, 0.)?;
    if !(0. ..=1.).contains(&low) || !(low..=1.).contains(&high) || !(0. ..=100.).contains(&feather)
    {
        return Err(error("invalid explicit range bounds"));
    }
    let mut c = json!({"kind":kind,"range":[low,high],"combine":"intersect",
        "invert":flag(get(t,n,CRS,"Invert"),false)?});
    c[key] = json!(feather);
    if lum {
        c["luminance_domain"] = json!("display");
    }
    if depth {
        c["model"] = Value::Null;
    }
    Ok(c)
}

// LR-4c approximations: exact foreign source and assumptions are retained by
// import-lrcat. Each `d` is one stamp (never a connected native stroke).
fn import_dabs(t: &Tree, n: &Node) -> EngineResult<Vec<engine_api::recipe::mask::BrushStroke>> {
    use engine_api::recipe::mask::BrushStroke;
    let root = child(t, n, CRS, "Dabs").ok_or_else(|| error("missing Adobe Dabs"))?;
    let tokens = t.items(root);
    if tokens.is_empty() || tokens.len() > 65_536 {
        return Err(error("empty or oversized Adobe Dabs"));
    }
    let mut radius = num(t, n, "Radius", 0.02)?;
    let mut flow = num(t, n, "Flow", 1.)?;
    let mut hardness = num(t, n, "CenterWeight", 0.5)?;
    let value = num(t, n, "MaskValue", 1.)?;
    if !(0. ..=1.).contains(&value) {
        return Err(error("invalid Adobe MaskValue"));
    }
    let mut strokes = Vec::new();
    for token in tokens {
        if token.text.len() > 256 || !token.children.is_empty() || !token.attrs.is_empty() {
            return Err(error("invalid Adobe dab token"));
        }
        let mut parts = token.text.split_whitespace();
        let command = parts.next().ok_or_else(|| error("empty dab token"))?;
        let values = parts.map(number).collect::<EngineResult<Vec<_>>>()?;
        match (command, values.as_slice()) {
            ("r", [r]) => radius = *r,
            ("f", [f]) => flow = *f,
            ("h", [h]) => hardness = *h,
            ("d", [x, y]) if (-16. ..=16.).contains(x) && (-16. ..=16.).contains(y) => {
                strokes.push(BrushStroke {
                    points: vec![[
                        *x as f32,
                        *y as f32,
                        if value == 0. { 1. } else { value as f32 },
                    ]],
                    radius: radius as f32,
                    feather: ((1. - hardness) * 100.) as f32,
                    flow: (flow * 100.) as f32,
                    erase: false,
                });
            }
            _ => return Err(error("unknown or malformed Adobe dab token")),
        }
        if !(1e-6..=16.).contains(&radius)
            || !(0. ..=1.).contains(&flow)
            || !(0. ..=1.).contains(&hardness)
        {
            return Err(error("invalid Adobe dab radius/flow/hardness"));
        }
    }
    if strokes.is_empty() {
        return Err(error("Adobe Dabs contain no stamps"));
    }
    Ok(strokes)
}

fn import_color_range(t: &Tree, n: &Node) -> EngineResult<Value> {
    let mut samples = Vec::new();
    for name in ["PointModels", "AreaModels"] {
        if let Some(root) = child(t, n, CRS, name) {
            for sample in t.items(root) {
                if samples.len() >= 65_536
                    || sample.text.len() > 4096
                    || !sample.attrs.is_empty()
                    || !sample.children.is_empty()
                {
                    return Err(error("invalid color sample model"));
                }
                let values = sample
                    .text
                    .split_whitespace()
                    .map(number)
                    .collect::<EngineResult<Vec<_>>>()?;
                if values.len() < 3 || values[..3].iter().any(|v| !(0. ..=1.).contains(v)) {
                    return Err(error("color model needs an RGB sample triple"));
                }
                // Assumption: leading triple is display-encoded sRGB D65;
                // remaining sample/area coordinates do not limit selection.
                samples.push(srgb_to_oklab([values[0], values[1], values[2]]));
            }
        }
    }
    let amount = num(t, n, "ColorAmount", 0.5)?;
    if samples.is_empty() || !(0. ..=1.).contains(&amount) {
        return Err(error("empty color samples or invalid ColorAmount"));
    }
    Ok(
        json!({"kind":"color_range", "samples":samples, "amount":amount*100.,
        "combine":"intersect", "invert":flag(get(t,n,CRS,"Invert"),false)?}),
    )
}

fn srgb_to_oklab(rgb: [f64; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|v| {
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

// LR-11: reuse the global point-colour grammar and curve representation.
//
// Adobe extended (HDR-domain) local curves follow the global rule: they are
// translated only when the packet selects HDR output, and an identity channel
// never replaces the ordinary one. Otherwise the source is provenance only and
// the ordinary local curve renders. They are validated either way.
fn import_local_curves(t: &Tree, n: &Node, params: &mut Value) -> EngineResult<()> {
    let hdr = flag(t.value(CRS, "HDREditMode"), false).unwrap_or(false);
    for (prefix, field) in [("", "curves"), ("Extended", "curves_extended")] {
        let mut curves = if prefix == "Extended" && !params["curves"].is_null() {
            params["curves"].clone()
        } else {
            serde_json::to_value(engine_api::recipe::settings::ToneCurves::default())?
        };
        let mut present = false;
        for (name, channel) in [
            ("MainCurve", "rgb"),
            ("RedCurve", "red"),
            ("GreenCurve", "green"),
            ("BlueCurve", "blue"),
        ] {
            if let Some(curve) = child(t, n, CRS, &format!("{prefix}{name}")) {
                super::structures::point_colors::validate_list(t, curve)?;
                let mut points = Vec::new();
                for item in t.items(curve) {
                    if !item.children.is_empty() || !item.attrs.is_empty() {
                        return Err(error("unsupported local curve point shape"));
                    }
                    let pair = item
                        .text
                        .split(',')
                        .map(number)
                        .collect::<EngineResult<Vec<_>>>()?;
                    if pair.len() != 2
                        || (prefix.is_empty() && pair.iter().any(|v| !(0.0..=255.0).contains(v)))
                    {
                        return Err(error("local curve requires coordinate pairs"));
                    }
                    points.push(json!({"x":pair[0]/255., "y":pair[1]/255.}));
                }
                if points.len() < 2
                    || points.windows(2).any(|p| {
                        p[0]["x"].as_f64() >= p[1]["x"].as_f64()
                            || p[0]["y"].as_f64() > p[1]["y"].as_f64()
                    })
                {
                    return Err(error("invalid local curve points"));
                }
                if prefix == "Extended"
                    && (!hdr || points.iter().all(|p| p["x"].as_f64() == p["y"].as_f64()))
                {
                    continue;
                }
                curves[channel] = json!(points);
                present = true;
            }
        }
        if present {
            params[field] = curves;
        }
    }
    Ok(())
}

fn import_local_point_colors(t: &Tree, n: &Node, params: &mut Value) -> EngineResult<()> {
    if let Some(points) = child(t, n, CRS, "LocalPointColors")
        && !(points.children.is_empty() && points.attrs.is_empty() && points.text.trim().is_empty())
    {
        super::structures::point_colors::validate_list(t, points)?;
        let decoded = t
            .items(points)
            .into_iter()
            .map(|item| super::structures::point_colors::decode(t, item))
            .collect::<EngineResult<Vec<_>>>()?;
        let decoded: Vec<_> = decoded.into_iter().flatten().collect();
        if !decoded.is_empty() {
            params["point_colors"] = serde_json::to_value(decoded)?;
        }
    }
    Ok(())
}
