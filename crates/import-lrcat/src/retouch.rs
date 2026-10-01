//! Conservative Adobe retouch translation over the retained-source contract.
//! A property is consumed only when every item is representable. Unknown fields
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
    for key in ["RetouchAreas", "RetouchInfo"] {
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
        if let Some(ops) = value.and_then(operations) {
            decoded.push((key, ops));
        }
    }
    // Both names can describe the same edits. Prefer the modern nonempty list;
    // conflicting nonempty aliases remain opaque rather than applying twice.
    if decoded.len() == 2
        && !decoded[0].1.is_empty()
        && !decoded[1].1.is_empty()
        && decoded[0].1 != decoded[1].1
    {
        return Ok(());
    }
    let Some(ops) = decoded
        .iter()
        .find(|(_, ops)| !ops.is_empty())
        .map(|(_, ops)| ops.clone())
    else {
        // Empty values do not justify altering previously retained recipes.
        return Ok(());
    };
    recipe.settings.locals.retouch = ops;
    for (key, ops) in decoded {
        if ops.is_empty() {
            continue;
        }
        let qualified = format!("crs:{key}");
        recipe.unknown.remove(&qualified);
        warnings.retain(|w| !w.starts_with(&format!("{qualified}:")));
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
                if fields
                    .insert(key.to_ascii_lowercase(), literal(value)?)
                    .is_some()
                {
                    return None;
                }
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

fn operations(value: Value) -> Option<Vec<RetouchOperation>> {
    value
        .as_array()?
        .iter()
        .enumerate()
        .map(|(i, item)| operation(item, i as u32 + 1))
        .collect()
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
    let strokes = if let Some(masks) = fields.get("masks") {
        masks
            .as_array()?
            .iter()
            .map(|m| stroke(m, feather))
            .collect::<Option<Vec<_>>>()?
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
    let source = point(&fields, "sourcex", "sourcey")?;
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

fn stroke(value: &Value, feather: f32) -> Option<BrushStroke> {
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
        return Some(BrushStroke {
            points: vec![[center[0], center[1], 1.0]],
            radius,
            feather,
            flow: percent(&fields, "flow", 1.0)?,
            erase: false,
        });
    }
    let mut points = Vec::new();
    for dab in fields.get("dabs")?.as_array()? {
        let tokens: Vec<_> = dab.as_str()?.split_whitespace().collect();
        if tokens.len() != 3 || tokens[0] != "d" {
            return None;
        }
        let x = bounded(tokens[1].parse::<f32>().ok()?, 0.0, 1.0)?;
        let y = bounded(tokens[2].parse::<f32>().ok()?, 0.0, 1.0)?;
        points.push([x, y, 1.0]);
    }
    if points.is_empty() {
        return None;
    }
    Some(BrushStroke {
        points,
        radius,
        feather,
        flow: percent(&fields, "flow", 1.0)?,
        erase: false,
    })
}
