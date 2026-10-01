use engine_api::recipe::LocalAdjustment;
use pipeline_cpu::{Image, masks::{rasterize, MaskOptions}};
use serde_json::json;
#[test]
fn lr4c_luminance_selection_uses_display_encoded_luminance() {
    // sRGB encoded 18% gray is ~0.461, outside a linear 0.4..0.5 band.
    let image = Image::new(2, 1, vec![vec![0.18, 0.45]; 3]).unwrap();
    let g: LocalAdjustment = serde_json::from_value(json!({"components":[{
        "kind":"luminance_range","range":[0.4,0.5],"smoothness":0
    }]})).unwrap();
    assert_eq!(rasterize(&image, &g, MaskOptions::default()).unwrap(), vec![1.,0.]);
}
