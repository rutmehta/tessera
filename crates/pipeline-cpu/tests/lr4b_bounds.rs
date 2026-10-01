use engine_api::recipe::LocalAdjustment;
use pipeline_cpu::{
    Image,
    masks::{MaskOptions, rasterize},
};
use serde_json::json;
#[test]
fn lr4b_asymmetric_bounds_roundtrip_and_render() {
    let value = json!({"components":[{"kind":"luminance_range","range":[0.25,0.5],"luminance_bounds":[0.0,0.25,0.5,1.0]}]});
    let g: LocalAdjustment = serde_json::from_value(value).unwrap();
    let data = vec![0., 0.0625, 0.125, 0.25, 0.5, 0.75, 0.875, 1.];
    let i = Image::new(8, 1, vec![data; 3]).unwrap();
    let mask = rasterize(&i, &g, MaskOptions::default()).unwrap();
    for (got, want) in mask
        .iter()
        .zip([0., 0.15625, 0.5, 1., 1., 0.5, 0.15625, 0.])
    {
        assert!((got - want).abs() < 1e-6, "{got} != {want}");
    }
    let v = serde_json::to_value(&g).unwrap();
    assert_eq!(
        v["components"][0]["luminance_bounds"],
        json!([0., 0.25, 0.5, 1.])
    );
    assert_eq!(g, serde_json::from_value(v).unwrap());
}
#[test]
fn lr4b_invalid_bounds_are_rejected_before_rendering() {
    for bounds in [
        [0., 0.5, 0.25, 1.],
        [-0.1, 0.25, 0.5, 1.],
        [0., 0.25, 0.5, 1.1],
    ] {
        let g:LocalAdjustment=serde_json::from_value(json!({"components":[{"kind":"luminance_range","range":[0.25,0.5],"luminance_bounds":bounds}]})).unwrap();
        let i = Image::new(1, 1, vec![vec![0.4]; 3]).unwrap();
        assert!(rasterize(&i, &g, MaskOptions::default()).is_err());
    }
}
