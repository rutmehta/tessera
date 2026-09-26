use compositor::{
    Depth, Raster, SmartFilter,
    render::smart_filters::{FilterContext, SmartFilterEvaluator},
};
use engine_api::{EngineError, tile::Extent};
use filters::CompositorFilters;
#[test]
fn adapter_rejects_unrepresentable_new_layer_output() {
    let extent = Extent::new(8, 8);
    let input = Raster::new(extent, 4, Depth::F32, 0.5);
    let node = SmartFilter {
        name: "content_aware_fill".into(),
        params: serde_json::json!({"mask":vec![0.0;64],"fill":{"output_new_layer":true}}),
        ..Default::default()
    };
    let ctx = FilterContext {
        profile: None,
        level: 0,
        canvas: extent,
    };
    assert!(matches!(
        CompositorFilters.evaluate(&input, &node, &ctx),
        Err(EngineError::Unsupported { .. })
    ));
}

#[test]
fn neural_catalog_is_strict_and_cpu_only() {
    let extent = Extent::new(8, 8);
    let input = Raster::new(extent, 4, Depth::F32, 0.5);
    let ctx = FilterContext {
        profile: None,
        level: 0,
        canvas: extent,
    };
    assert!(CompositorFilters::load_model("unknown", None).is_err());
    assert!(
        matches!(CompositorFilters::load_model("neural/colorize", None), Err(EngineError::Unsupported {ref what}) if what.contains("weights"))
    );
    for name in [
        "neural/skin_smoothing",
        "neural/colorize",
        "neural/jpeg_artifact_removal",
    ] {
        let mut node = SmartFilter {
            name: name.into(),
            params: serde_json::json!({"typo":1}),
            ..Default::default()
        };
        assert!(matches!(
            CompositorFilters.evaluate(&input, &node, &ctx),
            Err(EngineError::InvalidArgument { .. })
        ));
        node.params = if name.ends_with("skin_smoothing") {
            serde_json::json!({"faces":[[1,1,4,4]]})
        } else {
            serde_json::json!({})
        };
        assert!(!CompositorFilters.supports(&node).unwrap());
        let result = CompositorFilters.evaluate(&input, &node, &ctx);
        if name.ends_with("skin_smoothing") {
            assert!(result.is_ok(), "{result:?}");
        } else {
            assert!(
                matches!(result, Err(EngineError::Unsupported {ref what}) if what.contains("weights")),
                "{result:?}"
            );
        }
    }
}
