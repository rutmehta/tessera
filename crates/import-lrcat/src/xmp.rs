//! Lightroom catalog XMP adapter. All recipe translation belongs to sidecar.
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

/// Import through the shared codec. The catalog version remains authoritative for
/// Adobe XMP; a hash-verified native companion is interpreted only by sidecar.
/// Compatibility diagnostics retain individual properties as well as the exact
/// original packet, even when a legacy spelling needs normalization for decoding.
pub fn parse(text: &str, process_version: &str) -> EngineResult<(Recipe, Vec<String>)> {
    let doc = Document::parse(text).map_err(|e| EngineError::Decode {
        format: "xmp".into(),
        message: e.to_string(),
    })?;
    if doc.root_element().has_tag_name((RDF, "Description")) {
        // Catalog rows commonly store a bare Description, unlike sidecar files.
        let wrapped = format!("<rdf:RDF xmlns:rdf=\"{RDF}\">{text}</rdf:RDF>");
        let (mut recipe, warnings) = parse(&wrapped, process_version)?;
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
    let original = XmpPacket::parse(text)?.to_recipe()?;
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
        let key = CrsKey::from_xmp(p.namespace, p.name);
        if key.is_none() && p.namespace != CRS {
            continue;
        }
        let qualified = key.map_or_else(|| format!("crs:{}", p.name), |k| k.qualified_name());
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
        XmpPacket::parse(normalized)?.to_recipe()?
    };
    let mut recipe = imported.recipe;
    let mut warnings = imported.warnings;
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
        } else if key == CrsKey::MaskGroupBasedCorrections {
            diagnostics.push((
                qualified,
                p.raw,
                "mask source retained; Adobe metadata/raster fidelity is not guaranteed".into(),
            ));
        }
    }
    for (key, raw, reason) in diagnostics {
        retain(&mut recipe, &key, raw);
        warnings.push(format!("{key}: {reason}; source preserved"));
    }
    if catalog_version.revision <= 2 {
        warnings.push(
            "legacy Adobe PV1/2: best-effort translation; rendering fidelity is not guaranteed"
                .into(),
        );
    }
    recipe.unknown.insert("sidecar_xmp".into(), json!(text));
    recipe.validate()?;
    Ok((recipe, warnings))
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

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shared_codec_imports_namespaced_profiles_aux_and_lens_blur() {
        let source = xml(
            r#"xmlns:a="http://ns.adobe.com/exif/1.0/aux/" crs:ProcessVersion="15.4" crs:CameraProfile="Adobe Color" crs:CameraProfileDigest="ABC" crs:LensProfileEnable="1" crs:LensProfileSetup="Custom" crs:LensProfileName="My lens" crs:LensProfileFilename="lens.lcp" crs:LensProfileDigest="DEF" a:EnhanceDenoiseAlreadyApplied="True" a:EnhanceDenoiseVersion="7" a:EnhanceDenoiseLumaAmount="50""#,
            r#"<crs:LensBlur crs:Active="True" crs:BlurAmount="27"/>"#,
        );
        let expected = sidecar::XmpPacket::parse(&source)
            .unwrap()
            .to_recipe()
            .unwrap();
        let (actual, warnings) = parse(&source, "15.4").unwrap();
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
    fn process_versions_and_legacy_warning() {
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
            assert_eq!(warnings.iter().any(|w| w.contains("legacy")), revision <= 2);
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
