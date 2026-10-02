//! Lightroom catalog XMP adapter. Core CRS translation belongs to sidecar;
//! catalog-specific LR-2 additions run after exact source retention.
use engine_api::{
    EngineError, EngineResult,
    recipe::{CrsKey, CrsValueType, ProcessVersion, Recipe},
};
use roxmltree::{Document, Node};
use serde_json::json;
use sidecar::XmpPacket;
use std::{collections::BTreeMap, ops::Range};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";
const CRS: &str = engine_api::recipe::crs::CRS_NAMESPACE;

struct Property<'a> {
    namespace: &'a str,
    name: &'a str,
    raw: &'a str,
    range: Range<usize>,
    node: Option<Node<'a, 'a>>,
}

// Preserve the previous decoder's error/empty-target behavior when geometry or
// local parameters cannot render. Legacy/native forms retain their existing path.
fn decode_with_mask_audit(text: &str, extensions: bool) -> EngineResult<sidecar::ImportedRecipe> {
    let packet = XmpPacket::parse(text)?;
    let imported = packet.to_catalog_recipe_with_foreign_mask_extensions(extensions)?;
    if extensions && !crate::mask_source::renderable(&imported.recipe.settings.locals.adjustments) {
        packet.to_catalog_recipe_with_foreign_mask_extensions(false)
    } else {
        Ok(imported)
    }
}

/// Import through the shared codec. The catalog version remains authoritative for
/// Adobe XMP; a hash-verified native companion is interpreted only by sidecar.
/// Compatibility diagnostics retain individual properties as well as the exact
/// original packet, even when a legacy spelling needs normalization for decoding.
pub fn parse(text: &str, process_version: &str) -> EngineResult<(Recipe, Vec<String>)> {
    let (mut recipe, mut warnings) = parse_inner(text, process_version, true)?;
    crate::retouch::translate(&mut recipe, &mut warnings)?;
    crate::geometry::finish(&mut recipe)?;
    recipe.validate()?;
    let doc = Document::parse(text).map_err(|e| EngineError::Decode {
        format: "xmp".into(),
        message: e.to_string(),
    })?;
    let wrapped;
    let doc = if doc.root_element().has_tag_name((RDF, "Description")) {
        wrapped = format!("<rdf:RDF xmlns:rdf=\"{RDF}\">{text}</rdf:RDF>");
        Document::parse(&wrapped).map_err(|e| EngineError::Decode {
            format: "xmp".into(),
            message: e.to_string(),
        })?
    } else {
        doc
    };
    crate::noop::silence(&crate::noop::xmp_table(&doc), &mut recipe, &mut warnings);
    crate::residual::explain(&recipe, &mut warnings);
    Ok((recipe, warnings))
}

pub(crate) fn parse_without_retouch(
    text: &str,
    process_version: &str,
) -> EngineResult<(Recipe, Vec<String>)> {
    parse_inner(text, process_version, false)
}

pub(crate) fn parse_inner(
    text: &str,
    process_version: &str,
    apply_lr2: bool,
) -> EngineResult<(Recipe, Vec<String>)> {
    let doc = Document::parse(text).map_err(|e| EngineError::Decode {
        format: "xmp".into(),
        message: e.to_string(),
    })?;
    if doc.root_element().has_tag_name((RDF, "Description")) {
        // Catalog rows commonly store a bare Description, unlike sidecar files.
        let wrapped = format!("<rdf:RDF xmlns:rdf=\"{RDF}\">{text}</rdf:RDF>");
        let (mut recipe, warnings) = parse_inner(&wrapped, process_version, apply_lr2)?;
        recipe.unknown.insert("sidecar_xmp".into(), json!(text));
        return Ok((recipe, warnings));
    }
    let catalog_version = ProcessVersion::from_crs(process_version)?;
    let mut properties = Vec::new();
    for desc in doc.descendants().filter(|n| {
        n.has_tag_name((RDF, "Description"))
            && n.parent().is_some_and(|p| p.has_tag_name((RDF, "RDF")))
    }) {
        for a in desc.attributes() {
            if let Some(namespace) = a.namespace() {
                properties.push(Property {
                    namespace,
                    name: a.name(),
                    raw: a.value(),
                    range: a.range(),
                    node: None,
                });
            }
        }
        for n in desc.children().filter(Node::is_element) {
            properties.push(Property {
                namespace: n.tag_name().namespace().unwrap_or(""),
                name: n.tag_name().name(),
                raw: if n.children().any(|c| c.is_element()) {
                    &text[n.range()]
                } else {
                    n.text().unwrap_or("")
                },
                range: n.range(),
                node: Some(n),
            });
        }
    }
    // Upgrade foreign mask forms only when the entire parent is consumed. An
    // untranslated input must keep its prior recipe, not just its raw envelope.
    let mask_properties: Vec<_> = properties
        .iter()
        .filter(|p| p.namespace == CRS && p.name == "MaskGroupBasedCorrections")
        .collect();
    let foreign_mask_extensions = mask_properties.len() == 1
        && mask_properties[0]
            .node
            .is_some_and(crate::mask_source::audited_approximation);
    let original = decode_with_mask_audit(text, foreign_mask_extensions)?;
    let verified_native = properties.iter().any(|p| {
        p.namespace == CRS && p.name == "ProcessVersion" && ProcessVersion::from_crs(p.raw).is_ok()
    }) && original
        .recipe
        .process_version
        .crs_value()
        .is_some_and(|v| v.native_revision.is_some());
    let mut diagnostics = Vec::new();
    let mut removed = Vec::new();
    let mut seen = BTreeMap::<(&str, &str), usize>::new();
    let mut accepted = BTreeMap::<(&str, &str), usize>::new();
    for (i, p) in properties.iter().enumerate() {
        if p.namespace == CRS && p.name.starts_with("ExtendedToneCurve") {
            continue;
        }
        let key = CrsKey::from_xmp(p.namespace, p.name);
        if key.is_none() && p.namespace != CRS {
            continue;
        }
        let qualified = key.map_or_else(|| format!("crs:{}", p.name), |k| k.qualified_name());
        if key == Some(CrsKey::PointColors) && catalog_version.revision < 3 && !verified_native {
            diagnostics.push((qualified, p.raw, "requires Adobe PV3 or later".into()));
            removed.push(p.range.clone());
            continue;
        }
        if let Some(previous) = seen.insert((p.namespace, p.name), i) {
            diagnostics.push((
                qualified.clone(),
                properties[previous].raw,
                "duplicate property superseded".to_string(),
            ));
        }
        // Retain the catalog's strict numeric diagnostics. This checks the shared
        // table, not a second translation or list of recipe paths.
        let invalid = key
            .filter(|_| !verified_native)
            .and_then(|k| match k.value_type() {
                CrsValueType::Real { .. } | CrsValueType::Integer { .. }
                    if !k.is_informational() =>
                {
                    match p.raw.trim().parse::<f64>() {
                        Ok(n) if n.is_finite() && k.value_type().accepts(n) => None,
                        _ => Some("number outside CRS range".to_string()),
                    }
                }
                CrsValueType::PointList => p.node.and_then(|n| validate_curve(n).err()),
                CrsValueType::Choice(choices) if !choices.contains(&p.raw.trim()) => {
                    Some("unknown choice".into())
                }
                CrsValueType::Boolean
                    if !matches!(
                        p.raw.trim().to_ascii_lowercase().as_str(),
                        "true" | "false" | "0" | "1"
                    ) =>
                {
                    Some("invalid boolean".into())
                }
                _ => None,
            });
        if let Some(reason) = invalid {
            diagnostics.push((qualified, p.raw, reason));
            removed.push(p.range.clone());
            continue;
        }
        if let Some(previous) = accepted.insert((p.namespace, p.name), i) {
            removed.push(properties[previous].range.clone());
        }
        if key.is_none() {
            diagnostics.push((qualified, p.raw, "unsupported property".into()));
        } else if key == Some(CrsKey::ProcessVersion) {
            match ProcessVersion::from_crs(p.raw) {
                Ok(pv) if pv != catalog_version => diagnostics.push((
                    qualified,
                    p.raw,
                    "XMP/catalog process versions disagree".into(),
                )),
                Err(_) => (), // Shared codec supplies malformed-version diagnostics.
                _ => (),
            }
        }
    }
    let mut normalized = text.to_string();
    removed.sort_by_key(|r| r.start);
    for range in removed.iter().rev() {
        normalized.replace_range(range.clone(), "");
    }
    normalized = normalize_legacy_masks(&normalized)?;
    let imported = if normalized == text {
        original
    } else {
        decode_with_mask_audit(&normalized, foreign_mask_extensions)?
    };
    let mut recipe = imported.recipe;
    let mut warnings = imported.warnings;
    // Both unsupported-property filtering and approximation use the final,
    // normalized recipe, including duplicate-property resolution.
    let active_depth = !verified_native && recipe.settings.effects.lens_blur.is_some();
    if active_depth {
        diagnostics.retain(|(key, _, reason)| {
            key != "crs:DepthMapInfo" || reason != "unsupported property"
        });
    }
    warnings.retain(|w| !w.starts_with("crs:ExtendedToneCurve"));
    if properties.iter().any(|p| {
        p.namespace == CRS
            && p.name.starts_with("ExtendedToneCurve")
            && p.name != "ExtendedToneCurveName2012"
            && !identity_extended_property(p)
    }) {
        warnings.push(crate::lua_develop::EXTENDED_TONE_CURVE_NOTE.into());
    }
    // Absent/invalid XMP versions cannot smuggle the default native revision in.
    let valid_xmp_version = accepted
        .get(&(CRS, "ProcessVersion"))
        .is_some_and(|i| ProcessVersion::from_crs(properties[*i].raw).is_ok());
    if !valid_xmp_version
        || recipe
            .process_version
            .crs_value()
            .is_some_and(|v| v.native_revision.is_none())
    {
        recipe.process_version = catalog_version;
    }
    let masks_approximate = crate::mask_source::renderable(&recipe.settings.locals.adjustments)
        && !warnings
            .iter()
            .any(|w| w.starts_with("crs:MaskGroupBasedCorrections:"))
        && properties
            .iter()
            .filter(|p| p.namespace == CRS && p.name == "MaskGroupBasedCorrections")
            .count()
            == 1
        && properties.iter().any(|p| {
            p.namespace == CRS
                && p.name == "MaskGroupBasedCorrections"
                && p.node
                    .is_some_and(crate::mask_source::audited_approximation)
        });
    for p in &properties {
        let Some(key) = CrsKey::from_xmp(p.namespace, p.name) else {
            continue;
        };
        let qualified = key.qualified_name();
        if warnings
            .iter()
            .any(|w| w.starts_with(&format!("{qualified}:")))
        {
            // The codec reports the reason; add the legacy per-property payload
            // without duplicating its warning.
            retain(&mut recipe, &qualified, p.raw);
        } else if key == CrsKey::MaskGroupBasedCorrections && masks_approximate {
            if let Some(node) = p.node {
                crate::mask_source::record_approximation_diagnostics(&mut recipe, node);
            }
        } else if key == CrsKey::MaskGroupBasedCorrections {
            diagnostics.push((
                qualified,
                p.raw,
                p.node.map(crate::mask_source::unsupported_reason).unwrap_or_else(|| "mask correction must contain a structured selection".into()),
            ));
        }
    }
    for (key, raw, reason) in diagnostics {
        retain(&mut recipe, &key, raw);
        warnings.push(format!("{key}: {reason}; source preserved"));
    }
    let mut source = serde_json::Map::new();
    let translated_points = translated_point_colors(&recipe)
        && properties
            .iter()
            .filter(|p| p.namespace == CRS && p.name == "PointColors")
            .count()
            == 1
        && !warnings.iter().any(|w| w.starts_with("crs:PointColors:"));
    if translated_points {
        crate::diagnostics::push_approximate(
            &mut recipe,
            "PointColors",
            "/settings/color/point_colors",
            "LR-1",
            "approximate: gamma-encoded CPU HSL operator; Adobe pixel parity is not established",
        );
    }
    for p in &properties {
        if p.namespace == CRS
            && (crate::lua_develop::retain_source(p.name)
                || (crate::lr2::is_legacy(&recipe) && crate::lr2::stale_modern_control(p.name)))
        {
            source.insert(p.name.to_string(), json!(&text[p.range.clone()]));
        }
    }
    if !source.is_empty() {
        recipe.unknown.insert(
            "lrcat_develop_source".into(),
            json!({"shape": "xmp-fragments", "properties": source}),
        );
    }
    if !verified_native {
        approximate_depth(&mut recipe, &properties, &mut warnings, active_depth);
    }
    recipe.unknown.insert("sidecar_xmp".into(), json!(text));
    if apply_lr2 {
        crate::geometry::apply(
            &mut recipe,
            &mut warnings,
            properties
                .iter()
                .filter(|p| p.namespace == CRS)
                .map(|p| (p.name, p.raw)),
        )?;
        crate::lr2::xmp(&doc, &mut recipe, &mut warnings)?;
    }
    Ok((recipe, warnings))
}

pub(crate) fn translated_point_colors(recipe: &Recipe) -> bool {
    !recipe.settings.color.point_colors.is_empty()
        && recipe
            .settings
            .color
            .point_colors
            .iter()
            .all(|p| p.selection.is_some())
}

fn identity_extended_property(p: &Property<'_>) -> bool {
    let Some(node) = p.node else {
        return false;
    };
    let numbers: Option<Vec<f64>> = node
        .descendants()
        .filter(|n| n.has_tag_name((RDF, "li")))
        .flat_map(|n| n.text().unwrap_or("").split(','))
        .map(|s| s.trim().parse().ok())
        .collect();
    numbers.is_some_and(|n| {
        if n.len() < 4 || !n.len().is_multiple_of(2) {
            return false;
        }
        let points = n.as_chunks::<2>().0;
        points[0][0] == 0.0
            && points[points.len() - 1][0] == 255.0
            && points.iter().all(|p| p[0] == p[1])
            && points.windows(2).all(|p| p[0][0] < p[1][0])
    })
}

// Catalog curve validation is stricter than the shared codec's permissive
// point reader. Validate source shape only; sidecar still performs all decoding.
fn validate_curve(node: Node<'_, '_>) -> Result<(), String> {
    let seq = node
        .children()
        .find(|n| n.has_tag_name((RDF, "Seq")))
        .ok_or("missing RDF sequence")?;
    let mut previous = -1.0;
    let mut count = 0;
    for item in seq.children().filter(Node::is_element) {
        if !item.has_tag_name((RDF, "li")) || item.children().any(|n| n.is_element()) {
            return Err("invalid curve item".into());
        }
        let values = item
            .text()
            .unwrap_or("")
            .split(',')
            .map(|s| s.trim().parse::<f64>())
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| "invalid curve point")?;
        if values.len() != 2
            || values
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=255.0).contains(v))
            || values[0] <= previous
        {
            return Err("invalid or unordered curve points".into());
        }
        previous = values[0];
        count += 1;
    }
    // Empty lists are the shared writer's representation of a default curve.
    if count == 1 {
        return Err("curve needs at least two points".into());
    }
    Ok(())
}

fn retain(recipe: &mut Recipe, key: &str, raw: &str) {
    match recipe.unknown.entry(key.to_string()) {
        std::collections::btree_map::Entry::Vacant(e) => {
            e.insert(json!(raw));
        }
        std::collections::btree_map::Entry::Occupied(mut e) => {
            if !e.get().is_array() {
                let first = e.get().clone();
                e.insert(json!([first]));
            }
            e.get_mut().as_array_mut().unwrap().push(json!(raw));
        }
    }
}

// Older catalog exports used Start/End instead of Adobe's Full/Zero names.
// Rename only namespace-resolved mask geometry, never text or foreign fields.
fn normalize_legacy_masks(text: &str) -> EngineResult<String> {
    let doc = Document::parse(text).map_err(|e| EngineError::Decode {
        format: "xmp".into(),
        message: e.to_string(),
    })?;
    let mut edits = Vec::new();
    for n in doc.descendants().filter(|n| {
        n.ancestors()
            .any(|a| a.has_tag_name((CRS, "MaskGroupBasedCorrections")))
    }) {
        for (old, new) in [
            ("StartX", "FullX"),
            ("StartY", "FullY"),
            ("EndX", "ZeroX"),
            ("EndY", "ZeroY"),
        ] {
            if n.attribute((CRS, new)).is_none()
                && !n.children().any(|c| c.has_tag_name((CRS, new)))
            {
                if let Some(a) = n
                    .attributes()
                    .find(|a| a.namespace() == Some(CRS) && a.name() == old)
                {
                    let range = a.range();
                    let raw = &text[range.clone()];
                    let end = raw.find('=').unwrap_or(raw.len());
                    let name = raw[..end].trim_end();
                    let start = range.start + name.len() - old.len();
                    edits.push((start..start + old.len(), new));
                }
                for c in n.children().filter(|c| c.has_tag_name((CRS, old))) {
                    let range = c.range();
                    let raw = &text[range.clone()];
                    let end = raw.find([' ', '>', '/']).unwrap_or(raw.len());
                    edits.push((range.start + end - old.len()..range.start + end, new));
                    if let Some(close) = raw.rfind("</") {
                        let end = raw[close..].find('>').unwrap() + close;
                        edits.push((range.start + end - old.len()..range.start + end, new));
                    }
                }
            }
        }
    }
    edits.sort_by_key(|(r, _)| r.start);
    let mut result = text.to_string();
    for (range, value) in edits.into_iter().rev() {
        result.replace_range(range, value);
    }
    Ok(result)
}

/// Info records are persisted independently of the user-facing warnings vector.
fn approximate_depth(
    recipe: &mut Recipe,
    properties: &[Property<'_>],
    warnings: &mut Vec<String>,
    active: bool,
) {
    use engine_api::recipe::settings::LensBlurDepth;
    for p in properties.iter().filter(|p| p.namespace == CRS) {
        if !matches!(p.name, "LensBlur" | "DepthMapInfo") {
            continue;
        }
        let Some(n) = p.node else { continue };
        let n = n
            .children()
            .find(|c| c.has_tag_name((RDF, "Description")))
            .unwrap_or(n);
        let value = |name| {
            n.attribute((CRS, name)).map(str::to_string).or_else(|| {
                n.children()
                    .find(|c| c.has_tag_name((CRS, name)))
                    .and_then(|c| c.text())
                    .map(str::to_string)
            })
        };
        if p.name == "LensBlur" {
            if !active {
                continue;
            }
            for (field, reason) in LENS_BLUR_FIELDS {
                if value(field).is_some() {
                    crate::diagnostics::push_approximate(
                        recipe,
                        "LensBlur",
                        "/settings/effects/lens_blur",
                        "LR-6",
                        &format!(
                            "approximate: {field}: {reason}; Adobe convention unverified; exact source retained"
                        ),
                    );
                }
            }
            continue;
        }
        if !active {
            continue;
        }
        warnings.retain(|w| !w.starts_with("crs:DepthMapInfo:"));
        recipe.unknown.remove("crs:DepthMapInfo");
        let mut depth = LensBlurDepth {
            regenerate: true,
            ..Default::default()
        };
        depth.depth_source = value("DepthSource");
        depth.base_raw_depth_table = value("BaseRawDepthTable");
        depth.base_raw_depth_input_digest = value("BaseRawDepthInputDigest");
        depth.base_raw_depth_version = value("BaseRawDepthVersion");
        depth.base_layered_depth_table = value("BaseLayeredDepthTable");
        depth.base_layered_depth_input_digest = value("BaseLayeredDepthInputDigest");
        depth.base_layered_depth_version = value("BaseLayeredDepthVersion");
        depth.base_highlight_guide_table = value("BaseHighlightGuideTable");
        depth.base_highlight_guide_input_digest = value("BaseHighlightGuideInputDigest");
        depth.base_highlight_guide_version = value("BaseHighlightGuideVersion");
        for (field, reason) in DEPTH_FIELDS {
            if value(field).is_some() {
                crate::diagnostics::push_approximate(
                    recipe,
                    "DepthMapInfo",
                    "/settings/effects/lens_blur/depth",
                    "LR-6",
                    &format!(
                        "approximate: {field}: {reason}; Adobe encoding/calibration unverified; exact source retained; regenerated depth pending if resource unavailable"
                    ),
                );
            }
        }
        recipe
            .settings
            .effects
            .lens_blur
            .as_mut()
            .expect("active")
            .depth = Some(depth);
    }
    if active
        && recipe
            .settings
            .effects
            .lens_blur
            .as_ref()
            .is_some_and(|b| b.depth.is_none())
    {
        recipe
            .settings
            .effects
            .lens_blur
            .as_mut()
            .expect("active")
            .depth = Some(LensBlurDepth {
            regenerate: true,
            ..Default::default()
        });
    }
    if active {
        let key = if properties
            .iter()
            .any(|p| p.namespace == CRS && p.name == "DepthMapInfo")
        {
            "DepthMapInfo"
        } else {
            "LensBlur"
        };
        crate::diagnostics::push_approximate(
            recipe,
            key,
            "/settings/effects/lens_blur/depth",
            "LR-6",
            "regenerated depth: no Adobe depth resource; Tessera estimates depth at render",
        );
    }
}

const LENS_BLUR_FIELDS: &[(&str, &str)] = &[
    (
        "BlurAmount",
        "percentage of native maximum blur radius; Adobe image-size scaling unknown",
    ),
    (
        "FocalRange",
        "percent near-to-far endpoints populate native focus range and unclamped shoulder widths; linear ramps assumed",
    ),
    (
        "BokehShape",
        "0/1/2/3/4 approximated as circle/bubble/5-blade/ring/cat-eye; other values use circle",
    ),
    ("BokehShapeDetail", "percent radial pupil weighting"),
    ("BokehAspect", "signed percent log2 axis stretch"),
    (
        "BokehRotation",
        "degrees in the image plane; rotation origin and sign assumed",
    ),
    (
        "HighlightsBoost",
        "percent highlight gain; Adobe tone curve unknown",
    ),
    (
        "HighlightsThreshold",
        "percent scene-linear luminance threshold; Adobe transfer function unknown",
    ),
    ("CatEyeAmount", "percent radial pupil clipping"),
    ("CatEyeScale", "CatEyeAmount multiplier divided by 100"),
    (
        "SphericalAberration",
        "signed percent radial pupil weighting, not a wave-optics model",
    ),
    (
        "Version",
        "string provenance only; does not select a renderer",
    ),
    (
        "FocalRangeSource",
        "numeric selection provenance only; enum unknown",
    ),
    (
        "SampledArea",
        "string selection provenance only; coordinate encoding unknown",
    ),
    (
        "SampledRange",
        "string selection provenance only; native focus range is authoritative",
    ),
    (
        "SubjectRange",
        "string selection provenance only; native focus range is authoritative",
    ),
];
const DEPTH_FIELDS: &[(&str, &str)] = &[
    (
        "DepthSource",
        "opaque source provenance; no model or enum inferred",
    ),
    ("BaseRawDepthTable", "opaque resolver ID, never a path"),
    (
        "BaseRawDepthInputDigest",
        "association metadata; digest algorithm unknown",
    ),
    (
        "BaseRawDepthVersion",
        "opaque version provenance; does not select a decoder",
    ),
    (
        "BaseLayeredDepthTable",
        "opaque resolver ID preferred over raw; never a path",
    ),
    (
        "BaseLayeredDepthInputDigest",
        "layered association metadata; digest algorithm unknown",
    ),
    (
        "BaseLayeredDepthVersion",
        "opaque layered version provenance",
    ),
    (
        "BaseHighlightGuideTable",
        "opaque guide resource ID; never interpreted as depth",
    ),
    (
        "BaseHighlightGuideInputDigest",
        "guide association metadata; digest algorithm unknown",
    ),
    (
        "BaseHighlightGuideVersion",
        "opaque guide version provenance",
    ),
];

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_codec_imports_namespaced_profiles_aux_and_lens_blur() {
        let source = xml(
            r#"xmlns:a="http://ns.adobe.com/exif/1.0/aux/" crs:ProcessVersion="15.4" crs:CameraProfile="Adobe Color" crs:CameraProfileDigest="ABC" crs:LensProfileEnable="1" crs:LensProfileSetup="Custom" crs:LensProfileName="My lens" crs:LensProfileFilename="lens.lcp" crs:LensProfileDigest="DEF" a:EnhanceDenoiseAlreadyApplied="True" a:EnhanceDenoiseVersion="7" a:EnhanceDenoiseLumaAmount="50""#,
            r#"<crs:LensBlur crs:Active="True" crs:BlurAmount="27"/>"#,
        );
        let mut expected = sidecar::XmpPacket::parse(&source)
            .unwrap()
            .to_recipe()
            .unwrap();
        let (actual, warnings) = parse(&source, "15.4").unwrap();
        expected
            .recipe
            .settings
            .effects
            .lens_blur
            .as_mut()
            .unwrap()
            .depth = Some(engine_api::recipe::settings::LensBlurDepth {
            regenerate: true,
            ..Default::default()
        });
        assert_eq!(actual.settings, expected.recipe.settings);
        assert_eq!(actual.provenance, expected.recipe.provenance);
        assert_eq!(warnings, expected.warnings);
        assert_eq!(actual.unknown["sidecar_xmp"], source);
    }

    #[test]
    fn catalog_version_fallback_disagreement_and_native_hash() {
        for value in ["11.0", "99.0"] {
            let source = xml(&format!("crs:ProcessVersion=\"{value}\""), "");
            let (r, w) = parse(&source, "15.4").unwrap();
            assert_eq!(r.process_version, ProcessVersion::adobe(6));
            assert_eq!(r.unknown["crs:ProcessVersion"], value);
            assert!(!w.is_empty());
            if value == "11.0" {
                assert!(
                    w.iter()
                        .any(|w| w.contains("XMP/catalog process versions disagree"))
                );
            }
        }
        let original = Recipe::default();
        let packet = XmpPacket::from_recipe(
            &original,
            &sidecar::Metadata::default(),
            &sidecar::MarkPreset::lightroom(),
        )
        .unwrap();
        let changed = packet
            .serialize()
            .replace(
                "<crs:Exposure2012>0.0</crs:Exposure2012>",
                "<crs:Exposure2012>1</crs:Exposure2012>",
            )
            .replace(
                "<crs:Exposure2012>0</crs:Exposure2012>",
                "<crs:Exposure2012>1</crs:Exposure2012>",
            );
        assert_ne!(changed, packet.serialize());
        assert_eq!(
            XmpPacket::parse(&changed)
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .process_version,
            ProcessVersion::adobe(6)
        );
        for source in [packet.serialize().to_string(), changed] {
            let expected = XmpPacket::parse(&source).unwrap().to_recipe().unwrap();
            let (actual, warnings) = parse(&source, "15.4").unwrap();
            assert_eq!(
                actual.process_version, expected.recipe.process_version,
                "{warnings:?}"
            );
            assert_eq!(actual.settings, expected.recipe.settings);
            assert_eq!(actual.unknown["sidecar_xmp"], source);
        }
    }

    #[test]
    fn native_structures_and_renamed_namespaces_use_shared_codec() {
        let mut recipe = Recipe::default();
        recipe.edit(Default::default(), |s| {
            s.color.point_colors = serde_json::from_value(json!([{"source_lch":[0.65,0.21,312.3],"hue_shift":-23.4,"saturation_shift":17.2,"luminance_shift":-9.3,"range":43.2}])).unwrap();
            s.effects.lens_blur = serde_json::from_value(json!({"amount":63.2,"focus_range":[0.13,0.72],"bokeh":"custom <&>","depth_model":null})).unwrap();
            s.locals.retouch = serde_json::from_value(json!([{"id":4,"kind":{"kind":"heal","source_offset":[-0.23,0.17]},"target":{"kind":"implicit"},"opacity":72.3,"feather":23.4,"enabled":true}])).unwrap();
            s.locals.adjustments = serde_json::from_value(json!([{"id":7,"name":"brush", "enabled":true,"amount":87.0,"invert":true,"params":{"exposure":0.25},"components":[{"kind":"brush","combine":"subtract","invert":true,"strokes":[{"points":[[0.1,0.2,0.7]],"radius":0.023,"feather":34.2,"flow":65.7,"erase":true}]}]}])).unwrap();
        }).unwrap();
        let packet = XmpPacket::from_recipe(
            &recipe,
            &sidecar::Metadata::default(),
            &sidecar::MarkPreset::lightroom(),
        )
        .unwrap();
        let renamed = packet
            .serialize()
            .replace("crs:", "camera:")
            .replace("xmlns:crs=", "xmlns:camera=");
        for source in [packet.serialize(), renamed.as_str()] {
            let expected = XmpPacket::parse(source).unwrap().to_recipe().unwrap();
            let (actual, _) = parse(source, "15.4").unwrap();
            assert_eq!(actual.settings, recipe.settings);
            assert_eq!(actual.settings, expected.recipe.settings);
            assert_eq!(actual.process_version, recipe.process_version);
            assert_eq!(actual.ids, expected.recipe.ids);
            assert_eq!(actual.unknown["sidecar_xmp"], source);
            actual.validate().unwrap();
        }
    }

    #[test]
    fn foreign_namespace_and_opaque_structures_are_retained_not_applied() {
        let source = xml(
            r#"xmlns:fake="urn:not-camera-raw" fake:Exposure2012="4" crs:EnhanceDenoiseVersion="7""#,
            r#"<crs:PointColors><rdf:Seq><rdf:li>opaque Adobe data</rdf:li></rdf:Seq></crs:PointColors><crs:RetouchInfo><rdf:Seq><rdf:li>opaque retouch</rdf:li></rdf:Seq></crs:RetouchInfo>"#,
        );
        let (actual, warnings) = parse(&source, "15.4").unwrap();
        assert_eq!(
            actual.settings,
            XmpPacket::parse(&source)
                .unwrap()
                .to_recipe()
                .unwrap()
                .recipe
                .settings
        );
        assert!(actual.provenance.properties.is_empty());
        for key in [
            "crs:PointColors",
            "crs:RetouchInfo",
            "crs:EnhanceDenoiseVersion",
        ] {
            assert!(actual.unknown.contains_key(key));
            assert!(warnings.iter().any(|w| w.starts_with(key)));
        }
        assert_eq!(actual.unknown["sidecar_xmp"], source);
    }

    #[test]
    fn process_versions_without_blanket_legacy_warning() {
        for (source, revision) in [
            ("5.0", 1),
            ("5.7", 2),
            ("6.7", 3),
            ("10.0", 4),
            ("11.0", 5),
            ("15.4", 6),
        ] {
            let (recipe, warnings) = parse(&xml("", ""), source).unwrap();
            assert_eq!(recipe.process_version, ProcessVersion::adobe(revision));
            assert!(warnings.is_empty(), "{warnings:?}");
        }
        assert!(parse(&xml("", ""), "99.0").is_err());
        assert!(parse("<broken", "15.4").is_err());
    }
    fn xml(attrs: &str, body: &str) -> String {
        format!(
            r#"<x:xmpmeta xmlns:x="adobe:ns:meta/" xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#" xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"><rdf:RDF><rdf:Description {attrs}>{body}</rdf:Description></rdf:RDF></x:xmpmeta>"#
        )
    }
    #[test]
    fn invalid_scalars_and_curves_are_preserved_without_poisoning_settings() {
        for (key, value) in [
            ("Exposure2012", "inf"),
            ("Contrast2012", "1.5"),
            ("Temperature", "1999"),
            ("WhiteBalance", "FutureMode"),
            ("AutoLateralCA", "banana"),
        ] {
            let (r, w) = parse(&xml(&format!("crs:{key}=\"{value}\""), ""), "15.4").unwrap();
            assert_eq!(r.unknown[&format!("crs:{key}")], value);
            assert_eq!(w.len(), 1);
            r.validate().unwrap();
        }
        let (r, w) = parse(&xml("", "<crs:ToneCurvePV2012><rdf:Seq><rdf:li>255, 255</rdf:li><rdf:li>0, 0</rdf:li></rdf:Seq></crs:ToneCurvePV2012>"), "15.4").unwrap();
        assert!(r.settings.tone.curves.rgb.0.is_empty());
        assert_eq!(w.len(), 1);
        assert!(parse("<broken", "15.4").is_err());
        assert!(parse(&xml("", ""), "99.0").is_err());
    }
    #[test]
    fn unsupported_mask_prevents_partial_group_application() {
        let (r, w) = parse(&xml("", r#"<crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li><crs:CorrectionMasks><rdf:Seq><rdf:li crs:What="Mask/Sky"/><rdf:li crs:What="Mask/Future"/></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>"#), "15.4").unwrap();
        assert!(r.settings.locals.adjustments.is_empty());
        assert!(r.unknown.contains_key("crs:MaskGroupBasedCorrections"));
        assert!(w.iter().any(|s| s.contains("Mask/Future")));
    }
    #[test]
    fn malformed_duplicate_does_not_erase_previous_valid_choice() {
        let source = xml(
            r#"crs:WhiteBalance="Daylight""#,
            "<crs:WhiteBalance>FutureMode</crs:WhiteBalance>",
        );
        let (recipe, warnings) = parse(&source, "15.4").unwrap();
        let (expected, _) = parse(&xml(r#"crs:WhiteBalance="Daylight""#, ""), "15.4").unwrap();
        assert_eq!(
            recipe.settings.white_balance,
            expected.settings.white_balance
        );
        assert!(warnings.iter().any(|w| w.contains("duplicate")));
        assert!(
            recipe.unknown["crs:WhiteBalance"]
                .as_array()
                .unwrap()
                .contains(&json!("FutureMode"))
        );
    }

    #[test]
    fn duplicate_properties_preserve_the_superseded_value() {
        let (r, w) = parse(
            &xml(
                r#"crs:Exposure2012="1""#,
                "<crs:Exposure2012>2</crs:Exposure2012>",
            ),
            "15.4",
        )
        .unwrap();
        assert_eq!(r.settings.tone.exposure, 2.0);
        assert!(r.unknown.contains_key("crs:Exposure2012"));
        assert!(!w.is_empty());
    }
    #[test]
    fn curves_and_unsupported_payloads_are_not_dropped() {
        let (r, w) = parse(&xml(r#"crs:Exposure2012="NaN" crs:Future="keep""#, r#"<crs:ToneCurvePV2012><rdf:Seq><rdf:li>0, 0</rdf:li><rdf:li>128, 160</rdf:li><rdf:li>255, 255</rdf:li></rdf:Seq></crs:ToneCurvePV2012><crs:Look><rdf:Description crs:Name="retain me"/></crs:Look>"#), "15.4").unwrap();
        assert_eq!(r.settings.tone.curves.rgb.0.len(), 3);
        assert_eq!(r.settings.tone.curves.rgb.0[2].x, 1.0);
        assert_eq!(w.len(), 2);
        assert_eq!(r.unknown["crs:Exposure2012"], "NaN");
        assert_eq!(
            r.settings.camera_profile.look.as_ref().unwrap().style,
            "retain me".into()
        );
        assert!(
            r.unknown["sidecar_xmp"]
                .as_str()
                .unwrap()
                .contains("retain me")
        );
        r.validate().unwrap();
    }
    #[test]
    fn mask_groups_translate_supported_geometry_and_retain_source() {
        let body = r#"<crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li crs:CorrectionName="Gradient" crs:LocalExposure2012="0.5"><crs:CorrectionMasks><rdf:Seq><rdf:li crs:What="Mask/Gradient" crs:StartX="0.1" crs:StartY="0.2" crs:EndX="0.8" crs:EndY="0.9" crs:MaskInverted="true"/></rdf:Seq></crs:CorrectionMasks></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections>"#;
        let (r, w) = parse(&xml("", body), "15.4").unwrap();
        assert_eq!(r.settings.locals.adjustments.len(), 1);
        let a = &r.settings.locals.adjustments[0];
        assert_eq!(a.name, "Gradient");
        assert_eq!(a.params.exposure, 0.5);
        assert!(a.components[0].invert);
        assert_eq!(
            a.components[0].kind,
            engine_api::recipe::MaskKind::Linear {
                start: [0.1, 0.2],
                end: [0.8, 0.9]
            }
        );
        assert_eq!(r.unknown["sidecar_xmp"], xml("", body));
        assert!(r.unknown.contains_key("crs:MaskGroupBasedCorrections"));
        assert!(!w.is_empty());
        r.validate().unwrap();
    }
    #[test]
    fn scalar_attributes_use_mappings_and_history() {
        let (r, w) = parse(&xml(r#"crs:Exposure2012="1.25" crs:WhiteBalance="As Shot" crs:AutoLateralCA="0" crs:PostCropVignetteStyle="2""#, ""), "15.4").unwrap();
        assert_eq!(r.settings.tone.exposure, 1.25);
        assert!(!r.settings.lens.remove_chromatic_aberration);
        assert_eq!(
            r.process_version,
            engine_api::recipe::ProcessVersion::adobe(6)
        );
        assert!(w.is_empty(), "{w:?}");
        r.validate().unwrap();
    }
}
