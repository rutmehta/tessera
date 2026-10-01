//! Synthetic LR-6 acceptance probes. No catalog or user image is read.
use engine_api::recipe::Recipe;

const LENS_BLUR: &str = r#"s = { LensBlur = {
    Version = 1, Active = true, BlurAmount = 37,
    FocalRange = "10 20 60 80", BokehShape = 0,
    BokehShapeDetail = 0, HighlightsBoost = 25, HighlightsThreshold = 50,
    CatEyeAmount = 0, CatEyeScale = 100,
    BokehAspect = 0, BokehRotation = 0, SphericalAberration = 0
} }"#;

fn assert_translated(recipe: &Recipe, warnings: &[String]) {
    let blur = recipe.settings.effects.lens_blur.as_ref().expect(
        "active Adobe LensBlur must populate the renderable recipe, not remain opaque source",
    );
    assert_eq!(blur.amount, 37.0);
    assert!(
        recipe
            .unknown
            .get("lrcat_develop_source")
            .and_then(|s| s.get("properties"))
            .and_then(|p| p.get("LensBlur"))
            .is_some(),
        "approximate LensBlur must retain its exact source"
    );
    assert!(warnings.is_empty(), "{warnings:?}");
    assert!(
        recipe.unknown["lrcat_translation_diagnostics"]
            .as_array()
            .unwrap()
            .iter()
            .any(|d| d["level"] == "info"
                && d["key"] == "LensBlur"
                && d["message"].as_str().unwrap().starts_with("approximate: "))
    );
    assert_eq!(blur.focus_range, [0.2, 0.6]);
    assert_eq!(
        blur.adobe.as_ref().unwrap().focal_range,
        Some([0.1, 0.2, 0.6, 0.8])
    );
    recipe.validate().unwrap();
    let restored: Recipe = serde_json::from_slice(&serde_json::to_vec(recipe).unwrap()).unwrap();
    assert_eq!(restored, *recipe);
}

#[test]
fn lr6_lua_lens_blur_approximates_with_exact_source() {
    let (recipe, warnings) = import_lrcat::develop(1, LENS_BLUR, "15.4").unwrap();
    assert_translated(&recipe, &warnings);
}

#[test]
fn lr6_xmp_lens_blur_approximates_with_exact_source() {
    let source = r#"<rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"
        xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/">
        <rdf:Description><crs:LensBlur rdf:parseType="Resource"
            crs:Version="1" crs:Active="true" crs:BlurAmount="37"
            crs:FocalRange="10 20 60 80" crs:BokehShape="0"
            crs:BokehShapeDetail="0" crs:HighlightsBoost="25"
            crs:HighlightsThreshold="50" crs:CatEyeAmount="0"
            crs:CatEyeScale="100" crs:BokehAspect="0"
            crs:BokehRotation="0" crs:SphericalAberration="0"/>
        </rdf:Description></rdf:RDF>"#;
    let (recipe, warnings) = import_lrcat::develop(1, source, "15.4").unwrap();
    assert_translated(&recipe, &warnings);
}

#[test]
fn lr6_missing_adobe_depth_records_regeneration_in_recipe() {
    // No helper/resource is supplied to the pure develop decoder. This tests
    // bookkeeping only, not inference success or Adobe pixel equivalence.
    let source = r#"s = {
        LensBlur = { Active = true, BlurAmount = 37 },
        DepthMapInfo = { DepthSource = "synthetic-unavailable" }
    }"#;
    let (recipe, _) = import_lrcat::develop(1, source, "15.4").unwrap();
    assert!(
        serde_json::to_string(&recipe)
            .unwrap()
            .contains("regenerated depth"),
        "the regeneration marker must survive saving the recipe, not only an import report"
    );
}

#[test]
fn lr6_export_synthetic_recipe_for_cpu_gate() {
    // The paired image-core test consumes these exact import bytes when the
    // gate script sets LR6_RECIPE. No extra dependency or model is necessary.
    let (recipe, warnings) = import_lrcat::develop(1, LENS_BLUR, "15.4").unwrap();
    assert_translated(&recipe, &warnings);
    if let Some(path) = std::env::var_os("LR6_RECIPE") {
        std::fs::write(path, recipe.to_json().unwrap()).unwrap();
    }
}

#[test]
fn every_documented_adobe_field_is_present_and_source_is_exact() {
    let lens_fields = [
        ("Version", "version", "'1'"),
        ("BokehShape", "bokeh_shape", "2"),
        ("BokehShapeDetail", "bokeh_shape_detail", "20"),
        ("HighlightsBoost", "highlights_boost", "25"),
        ("HighlightsThreshold", "highlights_threshold", "50"),
        ("CatEyeAmount", "cat_eye_amount", "30"),
        ("CatEyeScale", "cat_eye_scale", "100"),
        ("BokehAspect", "bokeh_aspect", "10"),
        ("BokehRotation", "bokeh_rotation", "15"),
        ("SphericalAberration", "spherical_aberration", "-20"),
        ("FocalRangeSource", "focal_range_source", "1"),
        ("SampledArea", "sampled_area", "'1 2 3 4'"),
        ("SampledRange", "sampled_range", "'10 20'"),
        ("SubjectRange", "subject_range", "'20 30'"),
    ];
    for (source_key, target, value) in lens_fields {
        let raw = format!("{{ Active = true, BlurAmount = 37, {source_key} = {value} }}");
        let (recipe, warnings) =
            import_lrcat::develop(1, &format!("s = {{ LensBlur = {raw} }}"), "15.4").unwrap();
        assert!(warnings.is_empty(), "{source_key}: {warnings:?}");
        let json = serde_json::to_value(&recipe).unwrap();
        assert!(
            !json["settings"]["effects"]["lens_blur"]["adobe"][target].is_null(),
            "{source_key}"
        );
        assert_eq!(
            recipe.unknown["lrcat_develop_source"]["properties"]["LensBlur"],
            raw
        );
        assert_eq!(
            Recipe::from_json(&recipe.to_json().unwrap()).unwrap(),
            recipe
        );
    }
    for (source_key, target) in [
        ("DepthSource", "depth_source"),
        ("BaseRawDepthTable", "base_raw_depth_table"),
        ("BaseRawDepthInputDigest", "base_raw_depth_input_digest"),
        ("BaseRawDepthVersion", "base_raw_depth_version"),
        ("BaseLayeredDepthTable", "base_layered_depth_table"),
        (
            "BaseLayeredDepthInputDigest",
            "base_layered_depth_input_digest",
        ),
        ("BaseLayeredDepthVersion", "base_layered_depth_version"),
        ("BaseHighlightGuideTable", "base_highlight_guide_table"),
        (
            "BaseHighlightGuideInputDigest",
            "base_highlight_guide_input_digest",
        ),
        ("BaseHighlightGuideVersion", "base_highlight_guide_version"),
    ] {
        let raw = format!("{{ {source_key} = 'opaque-id' }}");
        let (recipe, warnings) = import_lrcat::develop(
            1,
            &format!("s = {{ LensBlur = {{ Active = true }}, DepthMapInfo = {raw} }}"),
            "15.4",
        )
        .unwrap();
        assert!(warnings.is_empty(), "{source_key}: {warnings:?}");
        let json = serde_json::to_value(&recipe).unwrap();
        assert_eq!(
            json["settings"]["effects"]["lens_blur"]["depth"][target],
            "opaque-id"
        );
        assert_eq!(
            recipe.unknown["lrcat_develop_source"]["properties"]["DepthMapInfo"],
            raw
        );
        assert_eq!(
            recipe.settings.effects.lens_blur.as_ref().unwrap().amount,
            50.
        );
        assert_eq!(
            Recipe::from_json(&recipe.to_json().unwrap()).unwrap(),
            recipe
        );
    }
}

#[test]
fn focal_range_preserves_outer_endpoints_and_shapes_have_renderable_interpretations() {
    for (shape, name) in [
        (0, "circle"),
        (1, "bubble"),
        (2, "5-blade"),
        (3, "ring"),
        (4, "cat-eye"),
        (99, "circle"),
    ] {
        let (recipe, warnings) = import_lrcat::develop(1, &format!("s = {{ LensBlur = {{ Active = true, FocalRange = '-48 32 64 144', BokehShape = {shape} }} }}"), "15.4").unwrap();
        assert!(warnings.is_empty());
        let blur = recipe.settings.effects.lens_blur.unwrap();
        assert_eq!(blur.focus_range, [0.32, 0.64]);
        assert_eq!(
            blur.adobe.unwrap().focal_range,
            Some([-0.48, 0.32, 0.64, 1.44])
        );
        assert_eq!(blur.bokeh, name);
    }
    let (off, warnings) = import_lrcat::develop(
        1,
        "s = { LensBlur = { Active = false, BlurAmount = 100 } }",
        "15.4",
    )
    .unwrap();
    assert!(warnings.is_empty());
    assert!(off.settings.effects.lens_blur.is_none());
}

#[test]
fn untranslated_inputs_are_byte_identical_to_dcf07355() {
    // Captured by compiling dcf07355 in a temporary detached checkout. Synthetic
    // source, complete recipe bytes and warning order; no normalization applied.
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("data/lr6b-untranslated-baseline.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let source = case["source"].as_str().unwrap();
        let (recipe, warnings) = import_lrcat::develop(1, source, "15.4").unwrap();
        assert_eq!(
            recipe.to_json().unwrap(),
            case["recipe_bytes"].as_str().unwrap().as_bytes(),
            "{source}"
        );
        assert_eq!(
            serde_json::to_value(warnings).unwrap(),
            case["warnings"],
            "{source}"
        );
    }
}

#[test]
fn xmp_depth_attributes_retain_exact_fragment_without_warnings() {
    let fragment = "<crs:DepthMapInfo crs:DepthSource='1' crs:BaseRawDepthTable='opaque&amp;id' crs:BaseRawDepthVersion=' v1 '/>";
    let xml = format!(
        "<rdf:RDF xmlns:rdf='http://www.w3.org/1999/02/22-rdf-syntax-ns#' xmlns:crs='http://ns.adobe.com/camera-raw-settings/1.0/'><rdf:Description><crs:LensBlur crs:Active='true'/>{fragment}</rdf:Description></rdf:RDF>"
    );
    let (recipe, warnings) = import_lrcat::develop(1, &xml, "15.4").unwrap();
    assert!(warnings.is_empty(), "{warnings:?}");
    let depth = recipe
        .settings
        .effects
        .lens_blur
        .as_ref()
        .unwrap()
        .depth
        .as_ref()
        .unwrap();
    assert_eq!(depth.base_raw_depth_table.as_deref(), Some("opaque&id"));
    assert_eq!(depth.base_raw_depth_version.as_deref(), Some(" v1 "));
    assert_eq!(
        recipe.unknown["lrcat_develop_source"]["properties"]["DepthMapInfo"],
        fragment
    );
}

#[test]
fn lr6c_inactive_and_depth_only_do_not_enable_blur() {
    for lens in ["", "LensBlur = { Active = false, BlurAmount = 100 },"] {
        let (recipe, _) = import_lrcat::develop(
            1,
            &format!("s = {{ {lens} DepthMapInfo = {{ DepthSource = 1 }} }}"),
            "15.4",
        )
        .unwrap();
        assert!(recipe.settings.effects.lens_blur.is_none());
        assert!(!recipe.unknown["lrcat_develop_source"]["properties"]["DepthMapInfo"].is_null());
        let info = recipe.unknown["lrcat_translation_diagnostics"]
            .as_array()
            .unwrap();
        assert!(!info.iter().any(|d| d["key"] == "LensBlur"));
        assert!(
            info.iter()
                .any(|d| d["key"] == "DepthMapInfo" && d["level"] == "info")
        );
    }
}

#[test]
fn lr6c_import_has_one_import_authored_history_entry() {
    let (recipe, _) = import_lrcat::develop(1, LENS_BLUR, "15.4").unwrap();
    assert_eq!(recipe.history.entries.len(), 1);
    assert!(matches!(
        recipe.history.entries[0].meta.author,
        engine_api::recipe::Author::Import { .. }
    ));
    recipe.validate().unwrap();
}

#[test]
fn lr6c_diagnostics_describe_only_translated_fields() {
    let (recipe, _) = import_lrcat::develop(
        1,
        "s = { LensBlur = { Active = true, BlurAmount = 37 } }",
        "15.4",
    )
    .unwrap();
    let info = recipe.unknown["lrcat_translation_diagnostics"]
        .as_array()
        .unwrap();
    let fields: Vec<_> = info
        .iter()
        .filter(|d| d["key"] == "LensBlur")
        .map(|d| d["field"].as_str().unwrap())
        .collect();
    assert_eq!(fields, ["Active", "BlurAmount"]);
    assert!(
        info.iter()
            .filter(|d| d["key"] == "LensBlur")
            .all(|d| d["message"].as_str().unwrap().starts_with("approximate: "))
    );
    let (off, _) =
        import_lrcat::develop(1, "s = { LensBlur = { Active = false } }", "15.4").unwrap();
    assert!(off.unknown.get("lrcat_translation_diagnostics").is_none());
}
