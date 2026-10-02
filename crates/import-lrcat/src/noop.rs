//! Catalog no-op policy. Source retention is unconditional and independent of this table.
//! Constants follow the LR-9 contract; never infer defaults from catalog frequency.
use crate::lua_develop::{LuaKey, LuaTable, LuaValue};
use engine_api::recipe::{ProcessFamily, ProcessVersion};

#[derive(Clone, Copy, Debug)]
pub enum Rule {
    Provenance,
    False,
    Empty,
    Upright(f64),
    Zero,
    Legacy(f64),
    CurveName,
    Number(f64),
    Sdr(f64),
    LensBlur,
    PointColors,
}
/// One policy table, shared by both adapters, the aggregate audit and matrix guard.
pub const RULES: &[(&str, Rule)] = &[
    ("Version", Rule::Provenance),
    ("CompatibleVersion", Rule::Provenance),
    ("UprightVersion", Rule::Provenance),
    ("UprightPreview", Rule::Provenance),
    ("UprightTransformCount", Rule::Provenance),
    ("AutoToneDigest", Rule::Provenance),
    ("AutoToneDigestNoSat", Rule::Provenance),
    ("AutoToneDigest*", Rule::Provenance),
    ("LensProfileIsEmbedded", Rule::Provenance),
    ("ConvertToGrayscale", Rule::False),
    ("OverrideLookVignette", Rule::False),
    ("RedEyeInfo", Rule::Empty),
    ("RetouchInfo", Rule::Empty),
    ("UprightFourSegmentsCount", Rule::Zero),
    ("UprightCenterMode", Rule::Upright(0.)),
    ("UprightCenterNormX", Rule::Upright(0.5)),
    ("UprightCenterNormY", Rule::Upright(0.5)),
    ("UprightFocalMode", Rule::Upright(0.)),
    ("UprightFocalLength35mm", Rule::Upright(35.)),
    ("ToneCurveName2012", Rule::CurveName),
    ("CurveRefineSaturation", Rule::Number(100.)),
    ("Brightness", Rule::Legacy(50.)),
    ("Contrast", Rule::Legacy(25.)),
    ("Exposure", Rule::Legacy(0.)),
    ("Shadows", Rule::Legacy(5.)),
    ("IncrementalTemperature", Rule::Zero),
    ("IncrementalTint", Rule::Zero),
    ("SDRBlend", Rule::Sdr(0.)),
    ("SDRBrightness", Rule::Sdr(0.)),
    ("SDRClarity", Rule::Sdr(0.)),
    ("SDRContrast", Rule::Sdr(0.)),
    ("SDRHighlights", Rule::Sdr(0.)),
    ("SDRShadows", Rule::Sdr(0.)),
    ("SDRWhites", Rule::Sdr(0.)),
    ("LensBlur", Rule::LensBlur),
    ("PointColors", Rule::PointColors),
];
fn get<'a>(t: &'a LuaTable, key: &str) -> Option<&'a LuaValue> {
    let mut found = t.fields.iter().filter_map(|(k, v)| match k {
        LuaKey::Str(k) if k == key => Some(v),
        _ => None,
    });
    let v = found.next()?;
    found.next().is_none().then_some(v)
}
fn off_or_absent(table: &LuaTable, key: &str) -> bool {
    !table
        .fields
        .iter()
        .any(|(k, _)| matches!(k,LuaKey::Str(k) if k == key))
        || get(table, key).is_some_and(off)
}
fn number(v: &LuaValue) -> Option<f64> {
    match v {
        LuaValue::Number(s) => s.parse::<f64>().ok().filter(|v| v.is_finite()),
        _ => None,
    }
}
fn zero(v: &LuaValue) -> bool {
    number(v) == Some(0.)
}
fn off(v: &LuaValue) -> bool {
    matches!(v, LuaValue::Bool(false)) || zero(v)
}
fn empty(v: &LuaValue) -> bool {
    matches!(v, LuaValue::Nil)
        || matches!(v, LuaValue::String(s) if s.trim().is_empty())
        || matches!(v, LuaValue::Table(t) if t.items.is_empty() && t.fields.is_empty())
}
fn placeholder(v: &LuaValue) -> bool {
    match v {
        LuaValue::String(s) => {
            let ns: Vec<_> = s.split(',').map(str::trim).collect();
            ns.len() == 19 && ns.iter().all(|n| n.parse::<f64>() == Ok(-1.))
        }
        _ => false,
    }
}
/// No source values (especially preset names) may be printed by callers of this audit API.
pub fn is_noop(key: &str, table: &LuaTable, version: &ProcessVersion) -> bool {
    let Some(v) = get(table, key) else {
        return false;
    };
    let Some((_, rule)) = RULES.iter().find(|(k, _)| *k == key).or_else(|| {
        RULES
            .iter()
            .find(|(k, _)| *k == "AutoToneDigest*" && key.starts_with("AutoToneDigest"))
    }) else {
        return false;
    };
    match rule {
        Rule::Provenance => true,
        Rule::False => off(v),
        Rule::Empty => empty(v),
        Rule::Zero => zero(v),
        Rule::Number(n) => number(v) == Some(*n),
        Rule::Upright(n) => number(v) == Some(*n) && off_or_absent(table, "PerspectiveUpright"),
        Rule::Legacy(n) => {
            version.family == ProcessFamily::Adobe && version.revision >= 3 && number(v) == Some(*n)
        }
        // Zero incremental WB is also neutral on rendered rows; no media-type guess is needed.
        Rule::Sdr(n) => {
            number(v).is_some() && (number(v) == Some(*n) || off_or_absent(table, "HDREditMode"))
        }
        Rule::CurveName => {
            version.family == ProcessFamily::Adobe
                && version.revision >= 3
                && matches!(v, LuaValue::String(_))
                && get(table, "ToneCurvePV2012").is_some_and(valid_curve)
        }
        Rule::LensBlur => {
            empty(v) || matches!(v,LuaValue::Table(t) if get(t,"Active").is_some_and(off))
        }
        Rule::PointColors => {
            empty(v)
                || matches!(v,LuaValue::Table(t) if crate::lua_develop::point_colors::sequence(t).is_ok_and(|t| t.items.iter().all(placeholder)))
        }
    }
}
fn valid_curve(v: &LuaValue) -> bool {
    let LuaValue::Table(t) = v else { return false };
    if !t.fields.is_empty() || t.items.len() < 4 || !t.items.len().is_multiple_of(2) {
        return false;
    }
    let Some(ns) = t.items.iter().map(number).collect::<Option<Vec<_>>>() else {
        return false;
    };
    ns.iter().all(|n| (0. ..=255.).contains(n))
        && ns
            .chunks_exact(2)
            .map(|p| p[0])
            .collect::<Vec<_>>()
            .windows(2)
            .all(|p| p[0] < p[1])
}

/// Remove key-scoped warnings/report diagnostics for proven no-ops; preserve source bytes.
pub(crate) fn silence(
    table: &LuaTable,
    recipe: &mut engine_api::recipe::Recipe,
    warnings: &mut Vec<String>,
) {
    for (key, _) in &table.fields {
        if let LuaKey::Str(key) = key
            && is_noop(key, table, &recipe.process_version)
        {
            crate::diagnostics::remove_noop(recipe, key);
        }
    }
    warnings.retain(|warning| {
        let warning = warning.strip_prefix("crs:").unwrap_or(warning);
        let Some((key, _)) = warning.split_once(':') else {
            return true;
        };
        !is_noop(key, table, &recipe.process_version)
    });
}

/// Convert CRS syntax for the policy only. Decoder and retention still read original XMP.
pub(crate) fn xmp_table(doc: &roxmltree::Document<'_>) -> LuaTable {
    const CRS: &str = engine_api::recipe::crs::CRS_NAMESPACE;
    const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
    fn scalar(s: &str) -> LuaValue {
        match s.trim() {
            "true" | "True" => LuaValue::Bool(true),
            "false" | "False" => LuaValue::Bool(false),
            s if s.parse::<f64>().is_ok() => LuaValue::Number(s.into()),
            _ => LuaValue::String(s.into()),
        }
    }
    fn value(n: roxmltree::Node<'_, '_>) -> LuaValue {
        if n.tag_name().name() == "ToneCurvePV2012" {
            return LuaValue::Table(LuaTable {
                fields: vec![],
                items: n
                    .descendants()
                    .filter(|n| n.has_tag_name((RDF, "li")))
                    .flat_map(|n| n.text().unwrap_or("").split(','))
                    .map(scalar)
                    .collect(),
            });
        }
        let children: Vec<_> = n.children().filter(|n| n.is_element()).collect();
        if children.len() == 1
            && ["Seq", "Bag", "Description"]
                .iter()
                .any(|name| children[0].has_tag_name((RDF, *name)))
        {
            return value(children[0]);
        }
        let mut t = LuaTable {
            items: vec![],
            fields: vec![],
        };
        for a in n.attributes().filter(|a| a.namespace() == Some(CRS)) {
            t.fields
                .push((LuaKey::Str(a.name().into()), scalar(a.value())));
        }
        for c in &children {
            if c.has_tag_name((RDF, "li")) {
                t.items.push(value(*c));
            } else {
                t.fields
                    .push((LuaKey::Str(c.tag_name().name().into()), value(*c)));
            }
        }
        if children.is_empty() && t.fields.is_empty() {
            scalar(n.text().unwrap_or(""))
        } else {
            LuaValue::Table(t)
        }
    }
    let mut t = LuaTable {
        items: vec![],
        fields: vec![],
    };
    for desc in doc.descendants().filter(|n| {
        n.has_tag_name((RDF, "Description"))
            && n.parent().is_some_and(|p| p.has_tag_name((RDF, "RDF")))
    }) {
        for a in desc.attributes().filter(|a| a.namespace() == Some(CRS)) {
            t.fields
                .push((LuaKey::Str(a.name().into()), scalar(a.value())));
        }
        for n in desc
            .children()
            .filter(|n| n.is_element() && n.tag_name().namespace() == Some(CRS))
        {
            t.fields
                .push((LuaKey::Str(n.tag_name().name().into()), value(n)));
        }
    }
    t
}
