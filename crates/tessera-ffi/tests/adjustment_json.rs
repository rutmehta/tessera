//! WP B5-06: the adjustment JSON the app's `AdjustmentModel` (apps/mac/Sources/TesseraCore/Document/
//! DocumentAdjustments.swift) mirrors. `fixtures/adjustments.json` holds one or more objects per
//! `compositor::Adjustment` variant; this test checks that serde reads each one and writes it back
//! unchanged, that the document session emits exactly that shape through `layers()`, and the Photoshop
//! layer names. The Swift suite `DocumentAdjustmentJSONTests` decodes the same file.
#![cfg(target_os = "macos")]

use compositor::Adjustment;
use serde_json::Value;
use std::sync::Arc;
use tessera_ffi::*;

fn fixture() -> Vec<Value> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/fixtures/adjustments.json"
    );
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

/// Every number as f64, so `0` (fixture) equals `0.0` (serde's f32 output).
fn normalize(v: &Value) -> Value {
    match v {
        Value::Number(n) => serde_json::json!(n.as_f64().unwrap()),
        Value::Array(a) => Value::Array(a.iter().map(normalize).collect()),
        Value::Object(o) => {
            Value::Object(o.iter().map(|(k, v)| (k.clone(), normalize(v))).collect())
        }
        other => other.clone(),
    }
}

/// Exhaustive: a new variant fails to compile until the fixture and the Swift mirror cover it.
fn tag(a: &Adjustment) -> &'static str {
    match a {
        Adjustment::Vibrance { .. } => "vibrance",
        Adjustment::ColorBalance { .. } => "color_balance",
        Adjustment::BlackWhite { .. } => "black_white",
        Adjustment::PhotoFilter { .. } => "photo_filter",
        Adjustment::GradientMap { .. } => "gradient_map",
        Adjustment::SelectiveColor { .. } => "selective_color",
        Adjustment::Desaturate => "desaturate",
        Adjustment::Equalize { .. } => "equalize",
        Adjustment::Auto { .. } => "auto",
        Adjustment::MatchColor { .. } => "match_color",
        Adjustment::ReplaceColor { .. } => "replace_color",
        Adjustment::ColorLookup { .. } => "color_lookup",
        Adjustment::ShadowsHighlights { .. } => "shadows_highlights",
        Adjustment::HdrToning { .. } => "hdr_toning",
        Adjustment::BrightnessContrast { .. } => "brightness_contrast",
        Adjustment::Levels { .. } => "levels",
        Adjustment::Curves { .. } => "curves",
        Adjustment::HueSaturation { .. } => "hue_saturation",
        Adjustment::Exposure { .. } => "exposure",
        Adjustment::Invert => "invert",
        Adjustment::Posterize { .. } => "posterize",
        Adjustment::Threshold { .. } => "threshold",
        Adjustment::ChannelMixer { .. } => "channel_mixer",
    }
}

const TITLES: [(&str, &str); 23] = [
    ("brightness_contrast", "Brightness/Contrast"),
    ("levels", "Levels"),
    ("curves", "Curves"),
    ("exposure", "Exposure"),
    ("vibrance", "Vibrance"),
    ("hue_saturation", "Hue/Saturation"),
    ("color_balance", "Color Balance"),
    ("black_white", "Black & White"),
    ("photo_filter", "Photo Filter"),
    ("channel_mixer", "Channel Mixer"),
    ("color_lookup", "Color Lookup"),
    ("invert", "Invert"),
    ("posterize", "Posterize"),
    ("threshold", "Threshold"),
    ("gradient_map", "Gradient Map"),
    ("selective_color", "Selective Color"),
    ("shadows_highlights", "Shadows/Highlights"),
    ("hdr_toning", "HDR Toning"),
    ("desaturate", "Desaturate"),
    ("match_color", "Match Color"),
    ("replace_color", "Replace Color"),
    ("equalize", "Equalize"),
    ("auto", "Auto"),
];

#[test]
fn fixture_round_trips_through_serde_and_covers_every_variant() {
    let mut seen = std::collections::BTreeSet::new();
    for v in fixture() {
        let a: Adjustment =
            serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{v}: {e}"));
        a.validate().unwrap_or_else(|e| panic!("{v}: {e:?}"));
        assert_eq!(v["kind"], tag(&a));
        assert_eq!(
            normalize(&serde_json::to_value(&a).unwrap()),
            normalize(&v),
            "serde output differs"
        );
        seen.insert(tag(&a));
    }
    let all: std::collections::BTreeSet<_> = TITLES.iter().map(|t| t.0).collect();
    assert_eq!(seen, all, "fixture covers every variant");
}

#[test]
fn session_emits_the_fixture_shape_and_photoshop_names() {
    let dir = tempfile::tempdir().unwrap();
    let engine: Arc<Engine> =
        Engine::open(dir.path().join("support").to_string_lossy().into_owned()).unwrap();
    let doc = engine.new_document(32, 16, DocDepth::U8, None).unwrap();
    // The Match Color fixture names source layer 7: any nonzero id is accepted as metadata.
    for v in fixture() {
        let up = doc
            .add_layer(
                NewLayer::Adjustment {
                    json: v.to_string(),
                },
                String::new(),
                None,
                None,
            )
            .unwrap_or_else(|e| panic!("{v}: {e:?}"));
        let id = up.created[0];
        let row = doc.layer(id).unwrap();
        let emitted: Value = serde_json::from_str(row.adjustment_json.as_deref().unwrap()).unwrap();
        assert_eq!(
            normalize(&emitted),
            normalize(&v),
            "layers() emits the serde shape"
        );
        let kind = v["kind"].as_str().unwrap();
        let title = TITLES.iter().find(|t| t.0 == kind).unwrap().1;
        assert!(
            row.name.starts_with(&format!("{title} ")),
            "{} for {kind}",
            row.name
        );
    }
}
