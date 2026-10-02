use engine_api::recipe::MaskComponent;
use serde_json::json;
#[test]
fn lr4b_bounds_omit_by_default_and_survive_json_roundtrip() {
    let old = json!({"kind":"luminance_range","range":[0.25,0.5],"smoothness":0.0,"combine":"add","invert":false});
    let c: MaskComponent = serde_json::from_value(old.clone()).unwrap();
    assert_eq!(serde_json::to_value(c).unwrap(), old);
    let mut new = old;
    new["luminance_bounds"] = json!([0., 0.25, 0.5, 1.]);
    let c: MaskComponent = serde_json::from_value(new.clone()).unwrap();
    assert_eq!(serde_json::to_value(c).unwrap(), new);
}
