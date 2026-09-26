//! engine-api 1.2 layered-document contracts agree with the compositor's own
//! serialized forms, so tool-call payloads convert by a JSON round trip.

use compositor::{Adjustment, Affine, BlendMode, Depth, GroupMode, LayerId, Rect};
use engine_api::document as api;
use serde::Serialize;
use serde::de::DeserializeOwned;

fn convert<A: Serialize, B: DeserializeOwned>(a: &A) -> B {
    serde_json::from_value(serde_json::to_value(a).unwrap()).unwrap()
}

#[test]
fn blend_modes_match_in_menu_order() {
    assert_eq!(api::BlendMode::ALL.len(), BlendMode::ALL.len());
    for (a, c) in api::BlendMode::ALL.iter().zip(BlendMode::ALL) {
        assert_eq!(convert::<_, BlendMode>(a), c);
    }
}

#[test]
fn adjustments_depth_group_mode_rect_and_affine_convert() {
    let specs = [
        api::AdjustmentSpec::Levels {
            master: api::LevelsChannel {
                gamma: 1.3,
                ..Default::default()
            },
            rgb: Default::default(),
        },
        api::AdjustmentSpec::Curves {
            master: vec![[0.0, 0.1], [1.0, 0.9]],
            rgb: Default::default(),
        },
        api::AdjustmentSpec::HueSaturation {
            hue: 20.0,
            saturation: 10.0,
            lightness: 0.0,
            colorize: false,
        },
        api::AdjustmentSpec::Exposure {
            exposure: 0.5,
            offset: 0.0,
            gamma: 1.0,
        },
        api::AdjustmentSpec::Invert,
        api::AdjustmentSpec::Posterize { levels: 4 },
        api::AdjustmentSpec::Threshold { level: 0.5 },
        api::AdjustmentSpec::ChannelMixer {
            matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
            constant: [0.0; 3],
            monochrome: false,
        },
    ];
    for s in &specs {
        let a: Adjustment = convert(s);
        assert_eq!(convert::<_, api::AdjustmentSpec>(&a), *s);
    }
    for (a, c) in [
        (api::DocumentDepth::U8, Depth::U8),
        (api::DocumentDepth::U16, Depth::U16),
        (api::DocumentDepth::F32, Depth::F32),
    ] {
        assert_eq!(convert::<_, Depth>(&a), c);
    }
    assert_eq!(
        convert::<_, GroupMode>(&api::GroupMode::Isolated),
        GroupMode::Isolated
    );
    let r = api::CanvasRect {
        x0: -1,
        y0: 2,
        x1: 30,
        y1: 40,
    };
    let cr: Rect = convert(&r);
    assert_eq!((cr.x0, cr.y0, cr.x1, cr.y1), (-1, 2, 30, 40));
    // Same coefficient order as `Affine::m` (the wire form is a bare array).
    let t = api::AffineTransform([2.0, 0.5, 5.0, 0.25, 3.0, 7.0]);
    assert_eq!(
        Affine { m: t.0 }.apply(1.0, 2.0),
        (2.0 + 1.0 + 5.0, 0.25 + 6.0 + 7.0)
    );
}

#[test]
fn layer_ids_are_the_contract_type() {
    let id: engine_api::id::LayerId = LayerId(5);
    assert_eq!(serde_json::to_string(&id).unwrap(), "5");
    assert!(LayerId::ROOT.is_root());
}
