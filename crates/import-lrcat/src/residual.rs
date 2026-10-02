//! Feature-specific residual diagnostics. Never include source property values.
use engine_api::recipe::Recipe;

pub(crate) const CLOUD_NOTE: &str = "requires Adobe cloud; not translatable. Export rendered pixels from Lightroom to preserve generative removal.";

pub const FEATURES: &[(&str, &str)] = &[
    ("SDRBlend", "separate SDR rendition blending while HDR editing is active is not implemented"),
    ("SDRBrightness", "separate SDR rendition brightness while HDR editing is active is not implemented"),
    ("SDRClarity", "separate SDR rendition clarity while HDR editing is active is not implemented"),
    ("SDRContrast", "separate SDR rendition contrast while HDR editing is active is not implemented"),
    ("SDRHighlights", "separate SDR rendition highlights while HDR editing is active is not implemented"),
    ("SDRShadows", "separate SDR rendition shadows while HDR editing is active is not implemented"),
    ("SDRWhites", "separate SDR rendition whites while HDR editing is active is not implemented"),
    ("IncrementalTemperature", "relative white balance temperature requires a rendered-image calibration that is not implemented"),
    ("IncrementalTint", "relative white balance tint requires a rendered-image calibration that is not implemented"),
    ("OverrideLookVignette", "embedded profile vignette override is not implemented"),
    ("AutoTone", "Adobe automatic tone computation is not available; unresolved tone controls cannot be reproduced"),
    ("AutoWhiteVersion", "Adobe automatic white-balance algorithm version is not reproduced"),
    ("CropConstrainAspectRatio", "saved crop aspect-ratio editing constraint is not imported; explicit crop geometry is imported separately"),
    ("CustomTemperature", "saved custom white-balance temperature preset is not imported; active white balance is imported separately"),
    ("CustomTint", "saved custom white-balance tint preset is not imported; active white balance is imported separately"),
    ("CustomIncrementalTemperature", "saved custom relative white balance temperature is not implemented"),
    ("CustomIncrementalTint", "saved custom relative white balance tint is not implemented"),
    ("CustomLensProfileDigest", "custom lens-profile resource resolution is not implemented"),
    ("CustomLensProfileFilename", "custom lens-profile resource resolution is not implemented"),
    ("CustomLensProfileName", "custom lens-profile resource resolution is not implemented"),
    ("CustomLensProfileIsEmbedded", "custom embedded lens-profile selection is not implemented"),
    ("CustomLensProfileDistortionScale", "custom lens-profile distortion scaling is not implemented"),
    ("CustomLensProfileVignettingScale", "custom lens-profile vignetting scaling is not implemented"),
    ("DepthBasedCorrections", "depth-based local correction structure is not implemented"),
    ("RangeMaskMapInfo", "Adobe cached range-mask raster decoding is not implemented"),
    ("GrainSeed", "Adobe grain random-seed parity is not implemented"),
    ("Preset", "saved preset reference is not imported; explicit Develop controls are imported separately"),
    ("ToggleStyleAmount", "Adobe style-amount interpolation is not implemented"),
    ("ToggleStyleDigest", "Adobe style resource resolution is not implemented"),
    ("UprightDependentDigest", "Adobe cached Upright dependency metadata is not interpreted; selected transform is imported separately"),
    ("RemoveAreas", "content-aware removal requires Adobe patch pixels; patch decoding is not implemented"),
    ("RetouchAreas", "retouch selection or source coordinates cannot be decoded; Adobe patch removal and ellipse selections require additional translation"),
    ("RetouchInfo", "retouch spot method or source coordinates cannot be decoded"),
    ("PointColors", "point-color selection encoding cannot be decoded"),
];

pub(crate) fn explain(recipe: &mut Recipe, warnings: &mut Vec<String>) {
    let cloud_keys = ["EnableDistractionRemoval", "GenerativeRemove", "GenerativeFill"];
    let cloud_warning = warnings.iter().any(|w| w.trim_start_matches("crs:").split_once(':').is_some_and(|(key,detail)| cloud_keys.contains(&key) && detail.contains("requires Adobe cloud; not translatable")));
    if cloud_warning {
        crate::diagnostics::push_ignored(recipe, "GenerativeRemove", "LR-9b", CLOUD_NOTE);
        warnings.retain(|w| !w.trim_start_matches("crs:").split_once(':').is_some_and(|(key,_)|cloud_keys.contains(&key) || key == "FilterList"));
    }
    for warning in warnings.iter_mut() {
        let Some((key, detail)) = warning.trim_start_matches("crs:").split_once(':') else { continue; };
        if detail.contains("requires Adobe PV3 or later") { continue; }
        let reason = FEATURES.iter().find_map(|(k,r)| (*k == key).then_some(*r));
        if let Some(reason) = reason {
            *warning = format!("crs:{key}: {reason}");
        } else if key.starts_with("UprightTransform_") && detail.contains("unsupported property") {
            *warning = format!("crs:{key}: unselected cached Upright transform is not applied; selected transform is imported separately");
        } else if detail.contains("number outside CRS range") {
            *warning = format!("crs:{key}: tone or color control is outside its supported numeric range; unresolved Adobe auto-tone results cannot be reproduced");
        }
    }
    let mut seen = std::collections::BTreeSet::new();
    warnings.retain(|w| seen.insert(w.clone()));
}
