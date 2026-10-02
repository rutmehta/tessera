//! Conservative Adobe retouch translation over the retained-source contract.
//! A property is consumed only when every item translates or has an explicit cloud note. Unknown fields
//! and unresolved source coordinates keep the original source and diagnostics.
use crate::lua_develop::{LuaKey, LuaValue};
use engine_api::{
    EngineResult,
    id::RetouchId,
    recipe::{
        MaskComponent, MaskKind, Recipe,
        mask::{BrushStroke, RetouchKind, RetouchOperation, RetouchTarget},
    },
};
use serde_json::{Map, Value};

const CRS: &str = engine_api::recipe::crs::CRS_NAMESPACE;
const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

pub(crate) fn translate(recipe: &mut Recipe, warnings: &mut Vec<String>) -> EngineResult<()> {
    let Some(source) = recipe.unknown.get("lrcat_develop_source") else {
        return Ok(());
    };
    let Some(properties) = source.get("properties").and_then(Value::as_object) else {
        return Ok(());
    };
    let lua = source["shape"] == "lua-values";
    let mut decoded = Vec::new();
    let mut cloud_keys = Vec::new();
    let mut cloud_present = false;
    let mut modern_present = false;
    let legacy_present = properties.contains_key("RetouchInfo");
    for key in ["RetouchAreas", "RetouchInfo", "RemoveAreas"] {
        let Some(raw) = properties.get(key).and_then(Value::as_str) else {
            continue;
        };
        let value = if lua {
            crate::lua_develop::read(&format!("s = {raw}"))
                .ok()
                .and_then(literal)
        } else {
            xml(
                raw,
                recipe.unknown.get("sidecar_xmp").and_then(Value::as_str),
            )
        };
        cloud_present |= value.as_ref().is_some_and(has_cloud);
        modern_present |= key == "RetouchAreas"
            && !value
                .as_ref()
                .and_then(Value::as_array)
                .is_some_and(Vec::is_empty);
        if let Some((ops, cloud)) = value.and_then(operations) {
            if cloud {
                cloud_keys.push(key);
            }
            decoded.push((key, ops));
        }
    }
    if !cloud_keys.is_empty() {
        warnings.retain(|w| {
            !cloud_keys.iter().any(|key| {
                w.starts_with(&format!("{key}:")) || w.starts_with(&format!("crs:{key}:"))
            }) && !w.starts_with("EnableDistractionRemoval:")
                && !w.starts_with("crs:EnableDistractionRemoval:")
        });
    }
    if cloud_present {
        crate::diagnostics::push_ignored(
            recipe,
            "GenerativeRemove",
            "LR-9b",
            crate::residual::CLOUD_NOTE,
        );
    }
    if modern_present && legacy_present {
        warnings.retain(|w| !w.starts_with("RetouchInfo:") && !w.starts_with("crs:RetouchInfo:"));
        crate::diagnostics::push_ignored(
            recipe,
            "RetouchInfo",
            "LR-9b",
            "legacy retouch alias superseded by the modern RetouchAreas list",
        );
    }
    // RetouchInfo is the legacy alias; the complete modern list is authoritative.
    // RemoveAreas is independent and must be appended, never silently dropped.
    let modern = modern_present;
    let mut ops: Vec<_> = decoded
        .iter()
        .filter(|(key, _)| *key != "RetouchInfo" || !modern)
        .flat_map(|(_, ops)| ops.iter().cloned())
        .collect();
    if ops.is_empty() {
        return Ok(());
    }
    for (i, op) in ops.iter_mut().enumerate() {
        op.id = RetouchId(i as u32 + 1);
    }
    recipe.settings.locals.retouch = ops;
    for (key, ops) in decoded {
        if ops.is_empty() {
            continue;
        }
        let qualified = format!("crs:{key}");
        recipe.unknown.remove(&qualified);
        warnings.retain(|w| {
            !w.starts_with(&format!("{qualified}:")) && !w.starts_with(&format!("{key}:"))
        });
        if key == "RetouchInfo" && modern {
            continue;
        }
        crate::diagnostics::push_approximate(
            recipe,
            key,
            "/settings/locals/retouch",
            "LR-3",
            "approximate: Adobe healing, feather and orientation conventions are unverified; coordinates use the Develop input frame before output lens distortion and crop",
        );
    }
    Ok(())
}

fn literal(value: LuaValue) -> Option<Value> {
    Some(match value {
        LuaValue::Nil => Value::Array(vec![]),
        LuaValue::String(s) | LuaValue::Number(s) => Value::String(s),
        LuaValue::Bool(b) => Value::String(b.to_string()),
        LuaValue::Table(t) if t.fields.is_empty() => {
            Value::Array(t.items.into_iter().map(literal).collect::<Option<_>>()?)
        }
        LuaValue::Table(t) if t.items.is_empty() => {
            let mut fields = Map::new();
            for (key, value) in t.fields {
                let LuaKey::Str(key) = key else { return None };
                let key = key.to_ascii_lowercase();
                let value = literal(value)?;
                if fields.get(&key).is_some_and(|previous| previous != &value) {
                    return None;
                }
                fields.insert(key, value);
            }
            Value::Object(fields)
        }
        _ => return None,
    })
}

fn xml(raw: &str, packet: Option<&str>) -> Option<Value> {
    if let Some(packet) = packet {
        let doc = roxmltree::Document::parse(packet).ok()?;
        let node = doc
            .descendants()
            .find(|n| n.is_element() && &packet[n.range()] == raw)?;
        return xml_value(node);
    }
    // The retained fragment normally inherits these namespaces from its packet.
    let wrapped = format!("<root xmlns:rdf='{RDF}' xmlns:crs='{CRS}'>{raw}</root>");
    let doc = roxmltree::Document::parse(&wrapped).ok()?;
    xml_value(doc.root_element().children().find(|n| n.is_element())?)
}

fn xml_value(node: roxmltree::Node<'_, '_>) -> Option<Value> {
    if let Some(seq) = node.children().find(|n| n.has_tag_name((RDF, "Seq"))) {
        return Some(Value::Array(
            seq.children()
                .filter(|n| n.is_element())
                .map(|n| {
                    if n.has_tag_name((RDF, "li")) {
                        xml_value(n)
                    } else {
                        None
                    }
                })
                .collect::<Option<_>>()?,
        ));
    }
    if let Some(desc) = node
        .children()
        .find(|n| n.has_tag_name((RDF, "Description")))
    {
        return xml_value(desc);
    }
    let mut fields = Map::new();
    for attr in node.attributes() {
        if attr.namespace() == Some(RDF) && attr.name() == "parseType" {
            continue;
        }
        if attr.namespace() != Some(CRS) {
            return None;
        }
        if fields
            .insert(
                attr.name().to_ascii_lowercase(),
                Value::String(attr.value().into()),
            )
            .is_some()
        {
            return None;
        }
    }
    for child in node.children().filter(|n| n.is_element()) {
        if child.tag_name().namespace() != Some(CRS) {
            return None;
        }
        if fields
            .insert(
                child.tag_name().name().to_ascii_lowercase(),
                xml_value(child)?,
            )
            .is_some()
        {
            return None;
        }
    }
    if fields.is_empty() {
        Some(Value::String(node.text().unwrap_or("").trim().into()))
    } else {
        Some(Value::Object(fields))
    }
}

fn has_cloud(value: &Value) -> bool {
    match value {
        Value::Array(items) => items.iter().any(has_cloud),
        Value::Object(fields) => {
            fields.contains_key("pm_clio_model_version")
                || fields.get("spottype").and_then(Value::as_str) == Some("generative")
        }
        _ => false,
    }
}

fn operations(value: Value) -> Option<(Vec<RetouchOperation>, bool)> {
    let mut operations = Vec::new();
    let mut cloud = false;
    for (i, item) in value.as_array()?.iter().enumerate() {
        let f = fields(item)?;
        if f.contains_key("pm_clio_model_version")
            || f.get("spottype").and_then(Value::as_str) == Some("generative")
        {
            cloud = true;
        } else {
            operations.push(operation(item, i as u32 + 1)?);
        }
    }
    Some((operations, cloud))
}

fn fields(value: &Value) -> Option<Map<String, Value>> {
    if let Some(fields) = value.as_object() {
        return Some(fields.clone());
    }
    let mut fields = Map::new();
    for entry in value.as_str()?.split(',') {
        let (key, value) = entry.split_once('=')?;
        if fields
            .insert(
                key.trim().to_ascii_lowercase(),
                Value::String(value.trim().into()),
            )
            .is_some()
        {
            return None;
        }
    }
    Some(fields)
}

fn number(fields: &Map<String, Value>, key: &str) -> Option<f32> {
    let n = fields.get(key)?.as_str()?.parse::<f32>().ok()?;
    n.is_finite().then_some(n)
}
fn bounded(n: f32, min: f32, max: f32) -> Option<f32> {
    (n >= min && n <= max).then_some(n)
}
fn percent(fields: &Map<String, Value>, key: &str, default: f64) -> Option<f32> {
    let value = match fields.get(key) {
        Some(v) => v.as_str()?.parse::<f64>().ok()?,
        None => default,
    };
    (value.is_finite() && (0.0..=1.0).contains(&value)).then_some((value * 100.0) as f32)
}
fn point(fields: &Map<String, Value>, x: &str, y: &str) -> Option<[f32; 2]> {
    Some([
        bounded(number(fields, x)?, 0.0, 1.0)?,
        bounded(number(fields, y)?, 0.0, 1.0)?,
    ])
}

fn operation(value: &Value, id: u32) -> Option<RetouchOperation> {
    let fields = fields(value)?;
    let allowed = [
        "centerx",
        "centery",
        "radius",
        "sourcex",
        "sourcey",
        "spottype",
        "sourcestate",
        "opacity",
        "feather",
        "masks",
        "method",
        "seed",
        "maskdigest",
        "healversion",
        "offsety",
    ];
    if fields.keys().any(|k| !allowed.contains(&k.as_str())) {
        return None;
    }
    if let Some(state) = fields.get("sourcestate")
        && !matches!(
            state.as_str()?,
            "sourceSetExplicitly" | "sourceAutoComputed"
        )
    {
        return None;
    }
    if let Some(method) = fields.get("method") {
        let method = method.as_str()?;
        if method != fields.get("spottype")?.as_str()? && method != "gaussian" {
            return None;
        }
    }
    let feather = percent(&fields, "feather", 0.0)?;
    let opacity = percent(&fields, "opacity", 1.0)?;
    let strokes = if let Some(masks) = fields.get("masks")
        && !equivalent_circle_mask(masks, &fields).unwrap_or(false)
    {
        masks
            .as_array()?
            .iter()
            .map(|m| stroke(m, feather))
            .collect::<Option<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect()
    } else {
        let center = point(&fields, "centerx", "centery")?;
        vec![BrushStroke {
            points: vec![[center[0], center[1], 1.0]],
            radius: bounded(number(&fields, "radius")?, 1e-6, 1.0)?,
            feather,
            flow: 100.0,
            erase: false,
        }]
    };
    let first = strokes.first()?.points.first()?;
    let source = if fields.contains_key("sourcey") {
        point(&fields, "sourcex", "sourcey")?
    } else {
        point(&fields, "sourcex", "offsety")?
    };
    let source_offset = [source[0] - first[0], source[1] - first[1]];
    let kind = match fields.get("spottype")?.as_str()? {
        "heal" => RetouchKind::Heal { source_offset },
        "clone" => RetouchKind::Clone { source_offset },
        _ => return None,
    };
    Some(RetouchOperation {
        id: RetouchId(id),
        kind,
        target: RetouchTarget::Area {
            components: vec![MaskComponent::new(MaskKind::Brush { strokes })],
        },
        opacity,
        feather: 0.0,
        enabled: true,
    })
}

// Some catalog versions write both the old circular spot and its equivalent
// ellipse mask. Use LR-3's existing flat geometry only when both agree exactly.
fn equivalent_circle_mask(value: &Value, flat: &Map<String, Value>) -> Option<bool> {
    let masks = value.as_array()?;
    if masks.len() != 1 {
        return Some(false);
    }
    let mask = fields(&masks[0])?;
    if mask.get("what")?.as_str()? != "Mask/Ellipse" {
        return Some(false);
    }
    let allowed = [
        "what",
        "x",
        "y",
        "sizex",
        "sizey",
        "alpha",
        "centervalue",
        "perimetervalue",
        "maskid",
        "masksyncid",
        "maskactive",
        "maskinverted",
        "maskblendmode",
        "maskvalue",
    ];
    if mask.keys().any(|k| !allowed.contains(&k.as_str())) {
        return Some(false);
    }
    for (key, expected) in [
        ("maskactive", "true"),
        ("maskinverted", "false"),
        ("maskblendmode", "0"),
        ("maskvalue", "1"),
        ("alpha", "0"),
        ("centervalue", "1"),
        ("perimetervalue", "0"),
    ] {
        if mask.get(key).is_some_and(|v| v.as_str() != Some(expected)) {
            return Some(false);
        }
    }
    Some(
        number(flat, "centerx")? == number(&mask, "x")?
            && number(flat, "centery")? == number(&mask, "y")?
            && number(flat, "radius")? == number(&mask, "sizex")?
            && number(flat, "radius")? == number(&mask, "sizey")?,
    )
}

fn stroke(value: &Value, feather: f32) -> Option<Vec<BrushStroke>> {
    let fields = fields(value)?;
    let allowed = [
        "what",
        "radius",
        "flow",
        "dabs",
        "maskactive",
        "maskblendmode",
        "maskinverted",
        "maskvalue",
        "masksyncid",
        "maskdigest",
        "seed",
        "centerx",
        "centery",
        "maskid",
        "centerweight",
    ];
    if fields.keys().any(|k| !allowed.contains(&k.as_str()))
        || !matches!(fields.get("what")?.as_str()?, "Mask/Paint" | "Mask/Circle")
    {
        return None;
    }
    for (key, expected) in [
        ("maskactive", "true"),
        ("maskblendmode", "0"),
        ("maskinverted", "false"),
        ("maskvalue", "1"),
    ] {
        if fields
            .get(key)
            .is_some_and(|v| v.as_str() != Some(expected))
        {
            return None;
        }
    }
    let radius = bounded(number(&fields, "radius")?, 1e-6, 1.0)?;
    if fields.get("what")?.as_str()? == "Mask/Circle" {
        let center = point(&fields, "centerx", "centery")?;
        return Some(vec![BrushStroke {
            points: vec![[center[0], center[1], 1.0]],
            radius,
            feather,
            flow: percent(&fields, "flow", 1.0)?,
            erase: false,
        }]);
    }
    let dabs = fields.get("dabs")?.as_array()?;
    if dabs.is_empty() || dabs.len() > 65_536 {
        return None;
    }
    let stateful = dabs.iter().any(|dab| {
        dab.as_str()
            .and_then(|s| s.split_whitespace().next())
            .is_some_and(|s| matches!(s, "r" | "f" | "h"))
    });
    let mut radius = radius;
    let mut flow = percent(&fields, "flow", 1.0)?;
    let mut feather = if fields.contains_key("centerweight") {
        100. - percent(&fields, "centerweight", 0.5)?
    } else {
        feather
    };
    let mut points = Vec::new();
    let mut stamps = Vec::new();
    for dab in dabs {
        let tokens: Vec<_> = dab.as_str()?.split_whitespace().collect();
        let n = |s: &str, min, max| bounded(s.parse::<f32>().ok()?, min, max);
        match tokens.as_slice() {
            ["r", value] => radius = n(value, 1e-6, 1.)?,
            ["f", value] => flow = n(value, 0., 1.)? * 100.,
            ["h", value] => feather = (1. - n(value, 0., 1.)?) * 100.,
            ["d", x, y] => {
                let point = [n(x, 0., 1.)?, n(y, 0., 1.)?, 1.];
                if stateful {
                    stamps.push(BrushStroke {
                        points: vec![point],
                        radius,
                        feather,
                        flow,
                        erase: false,
                    });
                } else {
                    points.push(point);
                }
            }
            _ => return None,
        }
    }
    if stateful {
        return (!stamps.is_empty()).then_some(stamps);
    }
    if points.is_empty() {
        return None;
    }
    Some(vec![BrushStroke {
        points,
        radius,
        feather,
        flow,
        erase: false,
    }])
}

pub(crate) fn has_adobe_patch(recipe: &Recipe, key: &str) -> bool {
    let Some(source) = recipe.unknown.get("lrcat_develop_source") else {
        return false;
    };
    let Some(raw) = source["properties"][key].as_str() else {
        return false;
    };
    let value = if source["shape"] == "lua-values" {
        crate::lua_develop::read(&format!("s={raw}"))
            .ok()
            .and_then(literal)
    } else {
        xml(
            raw,
            recipe.unknown.get("sidecar_xmp").and_then(Value::as_str),
        )
    };
    value
        .as_ref()
        .and_then(Value::as_array)
        .is_some_and(|items| {
            items
                .iter()
                .any(|item| fields(item).is_some_and(|f| f.contains_key("pm_patch")))
        })
}
