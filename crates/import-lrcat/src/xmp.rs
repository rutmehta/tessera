//! Camera Raw XMP translation, retaining unsupported source properties.
use engine_api::recipe::crs::CRS_NAMESPACE;
use engine_api::{
    EngineError, EngineResult,
    recipe::{CrsKey, CrsValueType, DevelopSettings, EditMeta, ProcessVersion, Recipe},
};
use roxmltree::{Document, Node};
use serde_json::{Value, json};

const RDF: &str = "http://www.w3.org/1999/02/22-rdf-syntax-ns#";

/// Imports CRS properties. Unsupported or invalid properties are retained under
/// their `crs:` names in `unknown`, with a diagnostic for each occurrence.
pub fn parse(text: &str, process_version: &str) -> EngineResult<(Recipe, Vec<String>)> {
    let doc = Document::parse(text).map_err(|e| EngineError::Decode {
        format: "xmp".into(),
        message: e.to_string(),
    })?;
    let mut recipe = Recipe {
        process_version: ProcessVersion::from_crs(process_version)?,
        ..Recipe::default()
    };
    let mut state = serde_json::to_value(&recipe)?;
    let mut warnings = Vec::new();
    if recipe.process_version.revision <= 2 {
        warnings.push(
            "legacy Adobe PV1/2: best-effort translation; rendering fidelity is not guaranteed"
                .into(),
        );
    }
    let mut seen = std::collections::BTreeMap::<String, String>::new();
    for node in doc.descendants().filter(|n| {
        n.has_tag_name((RDF, "Description"))
            && !n
                .ancestors()
                .any(|a| a.tag_name().namespace() == Some(CRS_NAMESPACE))
    }) {
        for attr in node
            .attributes()
            .filter(|a| a.namespace() == Some(CRS_NAMESPACE))
        {
            if let Some(previous) = seen.insert(attr.name().into(), attr.value().into()) {
                preserve(
                    &mut recipe,
                    &mut warnings,
                    attr.name(),
                    &previous,
                    "duplicate property superseded",
                );
            }
            apply(
                attr.name(),
                attr.value(),
                None,
                &mut state,
                &mut recipe,
                &mut warnings,
            );
        }
        for child in node
            .children()
            .filter(|n| n.tag_name().namespace() == Some(CRS_NAMESPACE))
        {
            let raw = if child.children().any(|n| n.is_element()) {
                &text[child.range()]
            } else {
                child.text().unwrap_or("")
            };
            if let Some(previous) = seen.insert(child.tag_name().name().into(), raw.into()) {
                preserve(
                    &mut recipe,
                    &mut warnings,
                    child.tag_name().name(),
                    &previous,
                    "duplicate property superseded",
                );
            }
            apply(
                child.tag_name().name(),
                raw,
                Some(child),
                &mut state,
                &mut recipe,
                &mut warnings,
            );
        }
    }
    let settings: DevelopSettings = serde_json::from_value(state["settings"].clone())?;
    recipe.edit(EditMeta::user("Import Lightroom XMP", 0), |s| *s = settings)?;
    recipe.validate()?;
    Ok((recipe, warnings))
}

fn preserve(recipe: &mut Recipe, warnings: &mut Vec<String>, name: &str, raw: &str, reason: &str) {
    let key = format!("crs:{name}");
    match recipe.unknown.entry(key.clone()) {
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
    warnings.push(format!("{key}: {reason}; source preserved"));
}

fn apply(
    name: &str,
    raw: &str,
    _node: Option<Node<'_, '_>>,
    state: &mut Value,
    recipe: &mut Recipe,
    warnings: &mut Vec<String>,
) {
    let result = (|| -> Result<(), String> {
        let key = CrsKey::from_xmp_name(name).ok_or("unsupported property")?;
        if key == CrsKey::ProcessVersion {
            let pv = ProcessVersion::from_crs(raw).map_err(|e| e.to_string())?;
            if pv != recipe.process_version {
                return Err("XMP/catalog process versions disagree".into());
            }
            return Ok(());
        }
        let path = key.recipe_path().ok_or("legacy property has no mapping")?;
        if key == CrsKey::MaskGroupBasedCorrections {
            let adjustments = masks(_node.ok_or("mask group requires RDF")?, recipe, warnings)?;
            state
                .pointer_mut(path)
                .ok_or("mapping target missing")?
                .as_array_mut()
                .ok_or("mask target is not an array")?
                .extend(adjustments);
            preserve(
                recipe,
                warnings,
                name,
                raw,
                "mask source retained; Adobe metadata/raster fidelity is not guaranteed",
            );
            return Ok(());
        }
        let mut value = if key.value_type() == CrsValueType::PointList {
            curve(_node.ok_or("curve requires an RDF sequence")?)?
        } else {
            scalar(key, raw)?
        };
        match key {
            CrsKey::WhiteBalance => value = json!(raw.trim().to_lowercase().replace(' ', "_")),
            CrsKey::AutoLateralCA | CrsKey::CropConstrainToWarp | CrsKey::HDREditMode => {
                value = json!(value.as_f64() == Some(1.0))
            }
            CrsKey::PostCropVignetteStyle => {
                value = json!(match value.as_f64().unwrap() as u8 {
                    1 => "highlight_priority",
                    2 => "color_priority",
                    _ => "paint_overlay",
                })
            }
            CrsKey::PerspectiveUpright => {
                value = json!(match value.as_f64().unwrap() as u8 {
                    0 => "off",
                    1 => "auto",
                    2 => "level",
                    3 => "vertical",
                    4 => "full",
                    _ => "guided",
                })
            }
            CrsKey::LensProfileEnable => {
                value = json!({"kind": if value.as_f64() == Some(0.0) { "none" } else { "auto" }})
            }
            // A digest is not a profile identifier; Adobe model/profile metadata
            // cannot be substituted for an engine resource identifier.
            CrsKey::CameraProfileDigest
            | CrsKey::LensProfileName
            | CrsKey::LensProfileFilename
            | CrsKey::LensProfileDigest
            | CrsKey::LensProfileSetup
            | CrsKey::EnhanceDenoiseAlreadyApplied
            | CrsKey::EnhanceDenoiseVersion
            | CrsKey::EnhanceDetailsAlreadyApplied
            | CrsKey::HasCrop => {
                return Err("requires unsupported resource or compound translation".into());
            }
            CrsKey::DefringePurpleHueLo
            | CrsKey::DefringePurpleHueHi
            | CrsKey::DefringeGreenHueLo
            | CrsKey::DefringeGreenHueHi => {
                let index = usize::from(name.ends_with("Hi"));
                state.pointer_mut(path).ok_or("mapping target missing")?[index] =
                    json!(value.as_f64().unwrap() * 3.6);
                return Ok(());
            }
            _ => {}
        }
        let mut candidate = state.clone();
        *candidate
            .pointer_mut(path)
            .ok_or("mapping target missing")? = value;
        serde_json::from_value::<DevelopSettings>(candidate["settings"].clone())
            .map_err(|e| e.to_string())?;
        *state = candidate;
        Ok(())
    })();
    if let Err(reason) = result {
        preserve(recipe, warnings, name, raw, &reason);
    }
}

// Resource properties may live on rdf:li or its rdf:Description wrapper.
fn resource<'a, 'i>(node: Node<'a, 'i>) -> Node<'a, 'i> {
    node.children()
        .find(|n| n.has_tag_name((RDF, "Description")))
        .unwrap_or(node)
}
fn property<'a>(node: Node<'a, '_>, key: &str) -> Option<&'a str> {
    let node = resource(node);
    node.attribute((CRS_NAMESPACE, key)).or_else(|| {
        node.children()
            .find(|n| n.has_tag_name((CRS_NAMESPACE, key)))
            .and_then(|n| n.text())
    })
}
fn number(node: Node<'_, '_>, key: &str) -> Result<f32, String> {
    let value: f32 = property(node, key)
        .ok_or_else(|| format!("missing {key}"))?
        .trim()
        .parse()
        .map_err(|_| format!("invalid {key}"))?;
    if !value.is_finite() {
        return Err(format!("non-finite {key}"));
    }
    Ok(value)
}
fn flag(node: Node<'_, '_>, key: &str, default: bool) -> Result<bool, String> {
    match property(node, key).map(|s| s.trim().to_ascii_lowercase()) {
        None => Ok(default),
        Some(s) if s == "true" || s == "1" => Ok(true),
        Some(s) if s == "false" || s == "0" => Ok(false),
        _ => Err(format!("invalid {key}")),
    }
}
fn component(node: Node<'_, '_>) -> Result<engine_api::recipe::MaskComponent, String> {
    use engine_api::recipe::mask::MaskCombine;
    use engine_api::recipe::{MaskComponent, MaskKind};
    let kind = match property(node, "What").ok_or("missing mask What")? {
        "Mask/Gradient" => MaskKind::Linear {
            start: [number(node, "StartX")?, number(node, "StartY")?],
            end: [number(node, "EndX")?, number(node, "EndY")?],
        },
        "Mask/CircularGradient" => {
            let left = number(node, "Left")?;
            let right = number(node, "Right")?;
            let top = number(node, "Top")?;
            let bottom = number(node, "Bottom")?;
            if left >= right || top >= bottom {
                return Err("invalid radial bounds".into());
            }
            MaskKind::Radial {
                center: [(left + right) / 2.0, (top + bottom) / 2.0],
                radii: [(right - left) / 2.0, (bottom - top) / 2.0],
                angle: if property(node, "Angle").is_some() {
                    number(node, "Angle")?
                } else {
                    0.0
                },
                feather: number(node, "Feather")?,
            }
        }
        "Mask/Sky" => MaskKind::Sky { model: None },
        "Mask/Subject" => MaskKind::Subject { model: None },
        "Mask/Background" => MaskKind::Background { model: None },
        other => return Err(format!("unsupported mask {other}")),
    };
    let mut c = MaskComponent::new(kind);
    c.invert = flag(node, "MaskInverted", false)?;
    c.combine = match property(node, "MaskBlendMode").unwrap_or("0") {
        "0" | "Add" => MaskCombine::Add,
        "1" | "Subtract" => MaskCombine::Subtract,
        "2" | "Intersect" => MaskCombine::Intersect,
        _ => return Err("unsupported mask blend mode".into()),
    };
    Ok(c)
}
fn masks(
    node: Node<'_, '_>,
    recipe: &mut Recipe,
    warnings: &mut Vec<String>,
) -> Result<Vec<Value>, String> {
    use engine_api::recipe::LocalAdjustment;
    let seq = node
        .children()
        .find(|n| n.has_tag_name((RDF, "Seq")))
        .ok_or("missing mask group sequence")?;
    let mut result = Vec::new();
    for item in seq.children().filter(|n| n.is_element()) {
        let translated = (|| -> Result<LocalAdjustment, String> {
            if !item.has_tag_name((RDF, "li")) {
                return Err("invalid mask group item".into());
            }
            let item = resource(item);
            let mut a = LocalAdjustment {
                name: property(item, "CorrectionName").unwrap_or("").into(),
                enabled: flag(item, "CorrectionActive", true)?,
                invert: flag(item, "CorrectionInverted", false)?,
                ..LocalAdjustment::default()
            };
            if property(item, "CorrectionAmount").is_some() {
                a.amount = number(item, "CorrectionAmount")? * 100.0;
            }
            let mut params = serde_json::to_value(&a.params).map_err(|e| e.to_string())?;
            for (source, target, scale) in [
                ("LocalExposure2012", "exposure", 1.0),
                ("LocalContrast2012", "contrast", 100.0),
                ("LocalHighlights2012", "highlights", 100.0),
                ("LocalShadows2012", "shadows", 100.0),
                ("LocalWhites2012", "whites", 100.0),
                ("LocalBlacks2012", "blacks", 100.0),
                ("LocalTemperature", "temperature", 100.0),
                ("LocalTint", "tint", 100.0),
                ("LocalSaturation", "saturation", 100.0),
                ("LocalTexture", "texture", 100.0),
                ("LocalClarity2012", "clarity", 100.0),
                ("LocalDehaze", "dehaze", 100.0),
                ("LocalSharpness", "sharpness", 100.0),
                ("LocalLuminanceNoise", "noise", 100.0),
                ("LocalMoire", "moire", 100.0),
                ("LocalDefringe", "defringe", 100.0),
                ("LocalHue", "hue", 1.0),
            ] {
                if property(item, source).is_some() {
                    let n = number(item, source)? * scale;
                    if !n.is_finite() {
                        return Err(format!("out of range {source}"));
                    }
                    params[target] = json!(n);
                }
            }
            a.params = serde_json::from_value(params).map_err(|e| e.to_string())?;
            let container = item
                .children()
                .find(|n| n.has_tag_name((CRS_NAMESPACE, "CorrectionMasks")))
                .ok_or("missing CorrectionMasks")?;
            let components = container
                .children()
                .find(|n| n.has_tag_name((RDF, "Seq")))
                .ok_or("missing component sequence")?;
            for child in components.children().filter(|n| n.is_element()) {
                a.components.push(component(resource(child))?);
            }
            if a.components.is_empty() {
                return Err("empty mask group".into());
            }
            Ok(a)
        })();
        match translated {
            Ok(mut a) => {
                a.id = recipe.allocate_mask_id();
                result.push(serde_json::to_value(a).map_err(|e| e.to_string())?);
            }
            Err(e) => warnings.push(format!(
                "crs:MaskGroupBasedCorrections: group not applied: {e}"
            )),
        }
    }
    Ok(result)
}

fn curve(node: Node<'_, '_>) -> Result<Value, String> {
    let seq = node
        .children()
        .find(|n| n.has_tag_name((RDF, "Seq")))
        .ok_or("missing RDF sequence")?;
    let mut points = Vec::new();
    let mut previous = -1.0;
    for item in seq.children().filter(|n| n.is_element()) {
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
        points.push(json!({"x": values[0] / 255.0, "y": values[1] / 255.0}));
    }
    if points.len() < 2 {
        return Err("curve needs at least two points".into());
    }
    Ok(json!(points))
}

fn scalar(key: CrsKey, raw: &str) -> Result<Value, String> {
    let text = raw.trim();
    match key.value_type() {
        CrsValueType::Real { .. } | CrsValueType::Integer { .. } => {
            let n: f64 = text.parse().map_err(|_| "invalid number")?;
            if !n.is_finite() || !key.value_type().accepts(n) {
                return Err("number outside CRS range".into());
            }
            Ok(json!(n))
        }
        CrsValueType::Boolean => match text.to_ascii_lowercase().as_str() {
            "true" | "1" => Ok(json!(true)),
            "false" | "0" => Ok(json!(false)),
            _ => Err("invalid boolean".into()),
        },
        CrsValueType::Text => Ok(json!(raw)),
        CrsValueType::Choice(choices) if choices.contains(&text) => Ok(json!(text)),
        CrsValueType::Choice(_) => Err("unknown choice".into()),
        _ => Err("unsupported structured property".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
        assert_eq!(w.len(), 3);
        assert_eq!(r.unknown["crs:Exposure2012"], "NaN");
        assert!(
            r.unknown["crs:Look"]
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
        assert!(matches!(
            a.components[0].kind,
            engine_api::recipe::MaskKind::Linear { .. }
        ));
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
