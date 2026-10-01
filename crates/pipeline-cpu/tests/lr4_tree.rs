use engine_api::recipe::mask::LocalAdjustment;
use pipeline_cpu::{
    Image,
    masks::{MaskOptions, rasterize},
};
use serde_json::json;

#[test]
fn lr4_disabled_seed_is_skipped_before_intersection() {
    let g: LocalAdjustment = serde_json::from_value(json!({"components":[
        {"kind":"linear","start":[0.,0.],"end":[1.,0.],"enabled":false},
        {"kind":"linear","start":[1.,0.],"end":[0.,0.],"combine":"intersect"}
    ]}))
    .unwrap();
    let i = Image::new(2, 1, vec![vec![0.25; 2]; 3]).unwrap();
    assert_eq!(
        rasterize(&i, &g, MaskOptions::default()).unwrap(),
        vec![0.25, 0.75]
    );
}
#[test]
fn lr4_nested_intersection_then_subtraction_and_inversion() {
    let g: LocalAdjustment = serde_json::from_value(json!({"components":[
        {"kind":"linear","start":[0.,0.],"end":[1.,0.]},
        {"kind":"brush","strokes":[],"combine":"subtract","group":[
            {"kind":"linear","start":[0.,0.],"end":[1.,0.]},
            {"kind":"linear","start":[1.,0.],"end":[0.,0.],"combine":"intersect"}
        ]}
    ]}))
    .unwrap();
    let i = Image::new(2, 1, vec![vec![0.25; 2]; 3]).unwrap();
    assert_eq!(
        rasterize(&i, &g, MaskOptions::default()).unwrap(),
        vec![0.609375, 0.203125]
    );
    let bytes = serde_json::to_vec(&g).unwrap();
    let copy: LocalAdjustment = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(copy, g);
}
