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
            let ns: Vec<_> = s.split_whitespace().collect();
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
        Rule::Upright(n) => {
            number(v) == Some(*n) && get(table, "PerspectiveUpright").is_none_or(off)
        }
        Rule::Legacy(n) => {
            version.family == ProcessFamily::Adobe && version.revision >= 3 && number(v) == Some(*n)
        }
        // Zero incremental WB is also neutral on rendered rows; no media-type guess is needed.
        Rule::Sdr(n) => {
            number(v).is_some()
                && (number(v) == Some(*n) || get(table, "HDREditMode").is_none_or(off))
        }
        Rule::CurveName => {
            matches!(v, LuaValue::String(_))
                && get(table, "ToneCurvePV2012").is_some_and(valid_curve)
        }
        Rule::LensBlur => {
            empty(v) || matches!(v,LuaValue::Table(t) if get(t,"Active").is_some_and(off))
        }
        Rule::PointColors => {
            empty(v)
                || matches!(v,LuaValue::Table(t) if t.fields.is_empty() && t.items.iter().all(placeholder))
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
