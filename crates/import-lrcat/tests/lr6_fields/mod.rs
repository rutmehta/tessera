use engine_api::recipe::Recipe;
use import_lrcat::diagnostics;
pub fn check_lr6_fields(recipe: &Recipe, key: &str, path: &str) -> Result<(), String> {
    let source = recipe.unknown["lrcat_develop_source"]["properties"][key]
        .as_str()
        .unwrap();
    let json = serde_json::to_value(recipe).unwrap();
    for (field, target) in [
        ("FocalRange", "/focus_falloff"),
        ("BlurAmount", "/amount"),
        ("FocalRange", "/focus_range"),
        ("Version", "/adobe/version"),
        ("BokehShape", "/adobe/bokeh_shape"),
        ("BokehShapeDetail", "/adobe/bokeh_shape_detail"),
        ("HighlightsBoost", "/adobe/highlights_boost"),
        ("HighlightsThreshold", "/adobe/highlights_threshold"),
        ("CatEyeAmount", "/adobe/cat_eye_amount"),
        ("CatEyeScale", "/adobe/cat_eye_scale"),
        ("BokehAspect", "/adobe/bokeh_aspect"),
        ("BokehRotation", "/adobe/bokeh_rotation"),
        ("SphericalAberration", "/adobe/spherical_aberration"),
        ("FocalRangeSource", "/adobe/focal_range_source"),
        ("SampledArea", "/adobe/sampled_area"),
        ("SampledRange", "/adobe/sampled_range"),
        ("SubjectRange", "/adobe/subject_range"),
        ("DepthSource", "/depth_source"),
        ("BaseRawDepthTable", "/base_raw_depth_table"),
        ("BaseRawDepthInputDigest", "/base_raw_depth_input_digest"),
        ("BaseRawDepthVersion", "/base_raw_depth_version"),
        ("BaseLayeredDepthTable", "/base_layered_depth_table"),
        (
            "BaseLayeredDepthInputDigest",
            "/base_layered_depth_input_digest",
        ),
        ("BaseLayeredDepthVersion", "/base_layered_depth_version"),
        ("BaseHighlightGuideTable", "/base_highlight_guide_table"),
        (
            "BaseHighlightGuideInputDigest",
            "/base_highlight_guide_input_digest",
        ),
        ("BaseHighlightGuideVersion", "/base_highlight_guide_version"),
    ] {
        if !source
            .split(|c: char| !c.is_alphanumeric())
            .any(|token| token == field)
        {
            continue;
        }
        if json
            .pointer(&format!("{path}{target}"))
            .is_none_or(|v| v.is_null())
        {
            return Err(format!("{key}.{field}: missing translated fields"));
        }
        let count = diagnostics::entries(recipe)
            .get(key)
            .into_iter()
            .flatten()
            .filter(|d| {
                d.field.as_deref() == Some(path)
                    && d.level == "info"
                    && d.status == "approximate"
                    && d.reason.starts_with(&format!("approximate: {field}:"))
            })
            .count();
        if count != 1 {
            return Err(format!("{key}.{field}: requires one field info reason"));
        }
    }
    Ok(())
}
