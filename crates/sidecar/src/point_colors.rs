//! Adobe PointColors: SDK resources (Lua adapter) and 19-number XMP swatches.
//! Numeric layout: JarvisArt xmp_converter.py::parse_point_colors; units/limits:
//! LrDevelopController.addPointColorSwatch. Rendering remains approximate.
use super::{node, resource, scalar};
use crate::xml::*;
use engine_api::{
    EngineResult,
    recipe::settings::{PointColor, PointColorSelection},
};
use std::collections::BTreeMap;

const SCALARS: [&str; 7] = [
    "SrcHue",
    "SrcSat",
    "SrcLum",
    "HueShift",
    "SatScale",
    "LumScale",
    "RangeAmount",
];
const RANGES: [&str; 3] = ["HueRange", "SatRange", "LumRange"];
const LIMITS: [&str; 4] = ["LowerNone", "LowerFull", "UpperFull", "UpperNone"];

// Strict shape checking prevents future source fields and duplicate values from
// being discarded when a successfully translated key leaves retained source.
fn fields<'a>(
    tree: &'a Tree,
    n: &'a Node,
    allowed: &[&str],
) -> EngineResult<BTreeMap<&'a str, String>> {
    let original = n;
    let n = resource(tree, n);
    if !std::ptr::eq(n, original)
        && (original.children.len() != 1
            || !original.text.trim().is_empty()
            || original
                .attrs
                .iter()
                .any(|a| a.ns != RDF || a.local != "parseType"))
    {
        return Err(error("unsupported PointColors resource wrapper"));
    }
    if !n.text.trim().is_empty() {
        return Err(error("unexpected PointColors resource text"));
    }
    for i in &n.children {
        let c = &tree.nodes[*i];
        if !RANGES.contains(&c.local.as_str()) && (!c.children.is_empty() || !c.attrs.is_empty()) {
            return Err(error("unsupported PointColors scalar shape"));
        }
    }
    let mut values = BTreeMap::new();
    for (ns, key, value) in n
        .attrs
        .iter()
        .map(|a| (a.ns.as_str(), a.local.as_str(), a.value.clone()))
        .chain(n.children.iter().map(|i| {
            let c = &tree.nodes[*i];
            (c.ns.as_str(), c.local.as_str(), c.text.clone())
        }))
    {
        if ns == RDF && matches!(key, "about" | "parseType") {
            continue;
        }
        if ns != CRS || !allowed.contains(&key) || values.insert(key, value).is_some() {
            return Err(error("unsupported or duplicate PointColors field"));
        }
    }
    Ok(values)
}
fn number(value: &str) -> EngineResult<f32> {
    value
        .trim()
        .parse::<f32>()
        .ok()
        .filter(|v| v.is_finite())
        .ok_or_else(|| error("invalid PointColors number"))
}
pub(super) fn decode(tree: &Tree, item: &Node) -> EngineResult<PointColor> {
    let values: Vec<f32> = if item.children.is_empty() && item.attrs.is_empty() {
        item.text
            .split(',')
            .map(number)
            .collect::<EngineResult<_>>()?
    } else {
        let allowed: Vec<_> = SCALARS.into_iter().chain(RANGES).collect();
        let f = fields(tree, item, &allowed)?;
        let mut values = Vec::with_capacity(19);
        for (i, key) in SCALARS.iter().enumerate() {
            values.push(match f.get(key) {
                Some(value) => number(value)?,
                None if i >= 3 => {
                    if i == 6 {
                        0.5
                    } else {
                        0.
                    }
                }
                None => return Err(error("missing PointColors source sample")),
            });
        }
        for key in RANGES {
            let range = node(tree, item, CRS, key)
                .ok_or_else(|| error("missing PointColors feather range"))?;
            let f = fields(tree, range, &LIMITS)?;
            for name in LIMITS {
                let value = f
                    .get(name)
                    .ok_or_else(|| error("incomplete PointColors feather range"))?;
                values.push(number(value)?);
            }
        }
        values
    };
    if values.len() != 19 {
        return Err(error("PointColors requires 19 numbers"));
    }
    if !(0.0..=6.0).contains(&values[0])
        || values[3..6].iter().any(|v| !(-1.0..=1.0).contains(v))
        || !(0.0..=1.0).contains(&values[6])
    {
        return Err(error("PointColors sample/shift outside SDK range"));
    }
    let point = PointColor {
        source_lch: [0.; 3], // HSL samples must never be mislabeled as OkLCh.
        hue_shift: values[3] * 60.,
        saturation_shift: values[4] * 100.,
        luminance_shift: values[5] * 100.,
        range: values[6] * 100.,
        selection: Some(PointColorSelection {
            source_hsl: [values[0] * 60., values[1], values[2]],
            hue: values[7..11].try_into().unwrap(),
            saturation: values[11..15].try_into().unwrap(),
            luminance: values[15..19].try_into().unwrap(),
        }),
    };
    point.validate()?;
    Ok(point)
}
pub(super) fn is_native(tree: &Tree, item: &Node) -> bool {
    scalar(tree, item, PRIVATE, "Type").is_some()
}

// Do not let the permissive XML index silently ignore a future list extension.
pub(super) fn validate_list(tree: &Tree, n: &Node) -> EngineResult<()> {
    let valid_container = |n: &Node| n.attrs.is_empty() && n.text.trim().is_empty();
    if !valid_container(n) || n.children.len() != 1 {
        return Err(error("unsupported PointColors list shape"));
    }
    let seq = &tree.nodes[n.children[0]];
    if seq.ns != RDF
        || seq.local != "Seq"
        || !valid_container(seq)
        || seq.children.iter().any(|i| {
            let item = &tree.nodes[*i];
            item.ns != RDF || item.local != "li"
        })
    {
        return Err(error("unsupported PointColors sequence shape"));
    }
    Ok(())
}
