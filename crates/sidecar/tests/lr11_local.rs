use engine_api::recipe::Recipe;
use sidecar::{MarkPreset, Metadata, XmpPacket};
#[test]
fn local_optional_fields_roundtrip_through_native_xmp() {
    let mut r = Recipe::default();
    let locals=serde_json::from_value(serde_json::json!([{
        "params":{"curves":{"red":[{"x":0,"y":0},{"x":1,"y":0.9}]},"curves_extended":{"blue":[{"x":0,"y":0},{"x":2,"y":1.5}]},"point_colors":[{"source_lch":[0.6,0.2,25],"hue_shift":30,"range":100}],"color_overlay":[120,50],"defringe":50},
        "components":[{"kind":"object","model":null,"points":[[0.2,0.4]],"adobe_ai":{"category":"Object","resource_id":null,"mask_key":null,"regenerate":true,"instance_hint":{"InstanceIDs":[{"InstanceID":2}]}}}]
    }])).unwrap();
    r.edit(Default::default(), |s| s.locals.adjustments = locals)
        .unwrap();
    let packet =
        XmpPacket::from_recipe(&r, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
    let imported = XmpPacket::parse(packet.serialize())
        .unwrap()
        .to_recipe()
        .unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.recipe.settings.locals, r.settings.locals);
}
