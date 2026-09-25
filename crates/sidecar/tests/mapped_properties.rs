use engine_api::recipe::{CrsKey, CrsValueType, DevelopSettings, Recipe};
use proptest::prelude::*;
use serde_json::{Value, json};
use sidecar::{MarkPreset, Metadata, XmpPacket};

// Every table pointer is checked, not a hand-picked set of slider assertions.
fn settings_strategy() -> impl Strategy<Value = DevelopSettings> {
    (prop::collection::vec(0u16..1000, CrsKey::ALL.len()), any::<bool>(), "[a-zA-Z0-9 &<>\"'α花\\r\\n\\t]{0,20}")
        .prop_map(|(numbers, flag, name)| {
            let mut doc = serde_json::to_value(Recipe::default()).unwrap();
            for (key, n) in CrsKey::ALL.iter().zip(numbers.iter()) {
                let Some(path) = key.recipe_path().filter(|p| p.starts_with("/settings")) else {continue};
                let target = doc.pointer_mut(path).unwrap();
                let unit = f64::from(*n) / 1000.0;
                if target.is_boolean() { *target = json!(flag); }
                else if target.is_number() {
                    let (lo, hi) = match key.value_type() {
                        CrsValueType::Integer{min,max} => (min as f64,max as f64),
                        CrsValueType::Real{min,max} => (min,max),
                        _ => continue,
                    };
                    *target = json!((lo + unit * (hi-lo)) as f32);
                } else if key.value_type() == CrsValueType::PointList {
                    *target = json!([{"x":0.0,"y":0.0},{"x":0.5,"y":unit as f32},{"x":1.0,"y":1.0}]);
                }
            }
            doc["settings"]["white_balance"]["mode"] = json!(["as_shot","auto","daylight","cloudy","shade","tungsten","fluorescent","flash","custom"][numbers[0] as usize % 9]);
            doc["settings"]["effects"]["vignette"]["style"] = json!(["highlight_priority","color_priority","paint_overlay"][numbers[1] as usize % 3]);
            doc["settings"]["geometry"]["upright"]["mode"] = json!(["off","auto","full","level","vertical","guided"][numbers[2] as usize % 6]);
            doc["settings"]["camera_profile"]["profile"] = json!({"name":name,"digest":format!("digest-{}",numbers[3])});
            doc["settings"]["lens"]["profile"] = match numbers[4] % 5 {
                0 => json!({"kind":"none"}),
                1 => json!({"kind":"auto"}),
                2 => json!({"kind":"embedded"}),
                3 => json!({"kind":"auto_calibrated"}),
                _ => json!({"kind":"database","profile":{"name":name,"filename":"profile.lcp","digest":"digest","setup":"custom"}}),
            };
            let x = f32::from(numbers[5]) / 1000.0;
            doc["settings"]["camera_profile"]["look"] = if flag {json!({"style":name,"amount":x*200.0})} else {Value::Null};
            doc["settings"]["color"]["point_colors"] = json!([{"source_lch":[x,0.2,123.4],"hue_shift":x,"saturation_shift":-x,"luminance_shift":x,"range":x*100.0}]);
            doc["settings"]["effects"]["lens_blur"] = if flag {json!({"amount":x*100.0,"focus_range":[x,1.0],"bokeh":name,"depth_model":{"id":"depth","version":"v1"}})} else {Value::Null};
            doc["settings"]["lens"]["defringe_purple"]["hue_range"] = json!([x*180.0,180.0+x*180.0]);
            doc["settings"]["lens"]["defringe_green"]["hue_range"] = json!([x,360.0-x]);
            doc["settings"]["locals"]["adjustments"] = json!([{"id":numbers[6],"name":name,"amount":x*200.0,"enabled":flag,"invert":flag,"params":{"exposure":x,"color_overlay":[x*360.0,x*100.0]},"components":[
                {"kind":"linear","start":[x,0.0],"end":[1.0,1.0],"invert":flag,"combine":"subtract"},
                {"kind":"radial","center":[x,x],"radii":[0.13,0.27],"angle":x*360.0,"feather":x*100.0,"combine":"intersect"},
                {"kind":"subject","model":{"id":name,"version":"v2"}}
            ]}]);
            doc["settings"]["locals"]["retouch"] = json!([{"id":numbers[7],"kind":{"kind":"heal","source_offset":[x,-x]},"target":{"kind":"implicit"},"opacity":x*100.0,"feather":x*100.0,"enabled":flag}]);
            serde_json::from_value(doc["settings"].take()).unwrap()
        })
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 128, failure_persistence: None, ..ProptestConfig::default() })]
    #[test]
    fn all_mapped_fields_round_trip(settings in settings_strategy()) {
        let mut recipe = Recipe::default();
        recipe.edit(Default::default(), |s| *s = settings).unwrap();
        let packet = XmpPacket::from_recipe(&recipe, &Metadata::default(), &MarkPreset::lightroom()).unwrap();
        let imported = packet.to_recipe().unwrap();
        prop_assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
        let before = serde_json::to_value(&recipe).unwrap();
        let after = serde_json::to_value(&imported.recipe).unwrap();
        for key in CrsKey::ALL {
            if let Some(path) = key.recipe_path() {
                prop_assert_eq!(before.pointer(path), after.pointer(path), "{}", key);
            }
        }
        prop_assert_eq!(packet.with_recipe(&imported.recipe).unwrap().to_recipe().unwrap().recipe.settings, recipe.settings);
    }
}
