use engine_api::recipe::LocalAdjustment;
use pipeline_cpu::{
    Image,
    masks::{MaskOptions, rasterize},
};
use serde_json::json;
#[test]
fn lr4c_luminance_selection_uses_display_encoded_luminance() {
    // sRGB encoded 18% gray is ~0.461, outside a linear 0.4..0.5 band.
    let image = Image::new(2, 1, vec![vec![0.18, 0.45]; 3]).unwrap();
    let g: LocalAdjustment = serde_json::from_value(json!({"components":[{
        "kind":"luminance_range","luminance_domain":"display","range":[0.4,0.5],"smoothness":0
    }]}))
    .unwrap();
    assert_eq!(
        rasterize(&image, &g, MaskOptions::default()).unwrap(),
        vec![1., 0.]
    );
}

#[test]
fn lr4c_masks_are_anchored_before_guided_upright() {
    use engine_api::recipe::{
        DevelopSettings,
        settings::{GuideLine, LensProfileSource, UprightMode},
    };
    use pipeline_cpu::{
        RenderSource, geometry, render_linear_before_geometry, render_linear_scaled,
    };
    let image = Image::new(32, 24, vec![vec![0.2; 32 * 24]; 3]).unwrap();
    let mut s = DevelopSettings::default();
    s.lens.profile = LensProfileSource::None;
    s.lens.remove_chromatic_aberration = false;
    s.geometry.upright.mode = UprightMode::Guided;
    s.geometry.upright.guides = vec![
        GuideLine {
            start: [0.2, 0.],
            end: [0.1, 1.],
        },
        GuideLine {
            start: [0.8, 0.],
            end: [0.9, 1.],
        },
    ];
    s.geometry.transform.scale = 110.;
    s.locals.adjustments = serde_json::from_value(json!([{"params":{"exposure":1},"components":[{
        "kind":"brush","strokes":[],"group":[{"kind":"linear","start":[0,0],"end":[1,0]}]
    }]}]))
    .unwrap();
    let full = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    let pre = render_linear_before_geometry(&s, &RenderSource::Rgb(&image), None).unwrap();
    let expected = geometry(&pre, &s.geometry).unwrap();
    assert_eq!(full.planes(), expected.planes());
    // Re-evaluating the mask in the already-upright frame gives different pixels.
    let groups = std::mem::take(&mut s.locals.adjustments);
    let base = render_linear_scaled(&s, &RenderSource::Rgb(&image), 1).unwrap();
    let wrong_frame = pipeline_cpu::locals_image(&base, &groups, Default::default()).unwrap();
    assert!(
        full.planes()[0]
            .iter()
            .zip(&wrong_frame.planes()[0])
            .any(|(a, b)| (a - b).abs() > 1e-3)
    );
}

#[test]
fn lr4e_legacy_linear_and_explicit_display_select_different_bands() {
    let image = Image::new(2, 1, vec![vec![0.18, 0.45]; 3]).unwrap();
    for (domain, expected) in [
        (None, vec![0., 1.]),
        (Some("linear"), vec![0., 1.]),
        (Some("display"), vec![1., 0.]),
    ] {
        let mut c = json!({"kind":"luminance_range","range":[0.4,0.5]});
        if let Some(d) = domain {
            c["luminance_domain"] = json!(d);
        }
        let g = serde_json::from_value(json!({"components":[c]})).unwrap();
        assert_eq!(
            rasterize(&image, &g, MaskOptions::default()).unwrap(),
            expected
        );
    }
}
