use brush::{Brush, CloneSource, PaintMode, SampledTip, Symmetry, Texture, TextureMode, Tip};
use compositor::{Depth, Raster, Rect};
use engine_api::tile::Extent;
use std::sync::Arc;

#[test]
fn malformed_deserialized_settings_are_rejected_before_painting() {
    let default = serde_json::to_value(Brush::default()).unwrap();
    for (pointer, value) in [
        (
            "/tip/shape",
            serde_json::json!({"Sampled":{"name":"bad", "width":2,"height":2,"data":[1.0]}}),
        ),
        ("/tip/shape", serde_json::json!({"Round":{"hardness":-1.0}})),
        ("/dynamics/count", serde_json::json!(0)),
        ("/dynamics/scatter", serde_json::json!(-1.0)),
        ("/dynamics/size/jitter", serde_json::json!(2.0)),
        ("/smoothing/string_length", serde_json::json!(-1.0)),
        (
            "/symmetry",
            serde_json::json!({"Radial":{"cx":0.0,"cy":0.0,"count":0}}),
        ),
    ] {
        let mut json = default.clone();
        *json.pointer_mut(pointer).unwrap() = value;
        let b: Brush = serde_json::from_value(json).unwrap();
        assert!(b.validate().is_err(), "accepted malformed {pointer}");
    }
}

#[test]
fn complete_preset_roundtrips_including_sampled_tips_and_clone_pixels() {
    let mut source = Raster::new(Extent::new(3, 2), 4, Depth::F32, 0.25);
    for (x, y, tile) in source
        .render_region(Rect::of_extent(source.extent()), |x, y, p| {
            *p = [x as f32 * 0.17, y as f32 * 0.23, 0.8, 0.6];
        })
        .unwrap()
    {
        source.set_slot(x, y, Some(tile), 7).unwrap();
    }
    let sample = SampledTip::new("tip", 2, 2, vec![0.0, 0.3, 0.7, 1.0]).unwrap();
    let mut b = Brush {
        tip: Tip::sampled(sample.clone()),
        wet_edges: true,
        texture: Some(Texture {
            pattern: Arc::new(sample),
            scale: 2.5,
            depth: 0.4,
            invert: true,
            mode: TextureMode::Subtract,
        }),
        mode: PaintMode::Clone(CloneSource {
            offset: [2.0, -3.0],
            source: Some(source.clone()),
        }),
        symmetry: Symmetry::Radial {
            cx: 10.0,
            cy: 11.0,
            count: 5,
        },
        airbrush: Some(12.0),
        dissolve_seed: 33,
        ..Brush::default()
    };
    b.dynamics.size.jitter = 0.5;
    b.smoothing.string_length = 4.0;
    let json = serde_json::to_value(&b).unwrap();
    let decoded: Brush = serde_json::from_value(json.clone()).unwrap();
    assert_eq!(serde_json::to_value(&decoded).unwrap(), json);
    let PaintMode::Clone(c) = decoded.mode else {
        panic!("lost clone mode");
    };
    for y in 0..2 {
        for x in 0..3 {
            assert_eq!(c.source.as_ref().unwrap().pixel(x, y), source.pixel(x, y));
        }
    }
}
