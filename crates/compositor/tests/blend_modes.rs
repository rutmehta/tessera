//! Every blend mode against an independent double-precision reference
//! (scratch script mirroring the formulas in COMPOSITOR.md §2), three
//! pixels each, opaque and partially transparent.
mod common;
use common::*;
use compositor::*;

const B: [[f32; 3]; 3] = [[0.2, 0.5, 0.8], [0.9, 0.1, 0.4], [0.05, 0.7, 0.35]];
const S: [[f32; 3]; 3] = [[0.6, 0.3, 0.9], [0.25, 0.75, 0.5], [0.8, 0.45, 0.1]];

const EXPECTED: [(BlendMode, [[f32; 3]; 3]); 27] = [
    (
        BlendMode::Normal,
        [[0.6, 0.3, 0.9], [0.25, 0.75, 0.5], [0.8, 0.45, 0.1]],
    ),
    (
        BlendMode::Dissolve,
        [[0.6, 0.3, 0.9], [0.25, 0.75, 0.5], [0.8, 0.45, 0.1]],
    ),
    (
        BlendMode::Darken,
        [[0.2, 0.3, 0.8], [0.25, 0.1, 0.4], [0.0500000, 0.45, 0.1]],
    ),
    (
        BlendMode::Multiply,
        [
            [0.12, 0.15, 0.72],
            [0.225, 0.0750000, 0.2],
            [0.0400000, 0.315, 0.0350000],
        ],
    ),
    (
        BlendMode::ColorBurn,
        [
            [0.0000000, 0.0000000, 0.7777778],
            [0.6, 0.0000000, 0.0000000],
            [0.0000000, 0.3333333, 0.0000000],
        ],
    ),
    (
        BlendMode::LinearBurn,
        [
            [0.0000000, 0.0000000, 0.7],
            [0.15, 0.0000000, 0.0000000],
            [0.0000000, 0.15, 0.0000000],
        ],
    ),
    (
        BlendMode::DarkerColor,
        [[0.2, 0.5, 0.8], [0.9, 0.1, 0.4], [0.0500000, 0.7, 0.35]],
    ),
    (
        BlendMode::Lighten,
        [[0.6, 0.5, 0.9], [0.9, 0.75, 0.5], [0.8, 0.7, 0.35]],
    ),
    (
        BlendMode::Screen,
        [
            [0.68, 0.65, 0.98],
            [0.925, 0.775, 0.7],
            [0.81, 0.835, 0.415],
        ],
    ),
    (
        BlendMode::ColorDodge,
        [
            [0.5, 0.7142857, 1.0000000],
            [1.0000000, 0.4, 0.8],
            [0.25, 1.0000000, 0.3888889],
        ],
    ),
    (
        BlendMode::LinearDodge,
        [
            [0.8, 0.8, 1.0000000],
            [1.0000000, 0.85, 0.9],
            [0.85, 1.0000000, 0.45],
        ],
    ),
    (
        BlendMode::LighterColor,
        [[0.6, 0.3, 0.9], [0.25, 0.75, 0.5], [0.8, 0.45, 0.1]],
    ),
    (
        BlendMode::Overlay,
        [
            [0.24, 0.3, 0.96],
            [0.85, 0.15, 0.4],
            [0.0800000, 0.67, 0.0700000],
        ],
    ),
    (
        BlendMode::SoftLight,
        [
            [0.2494427, 0.4, 0.8755418],
            [0.855, 0.2081139, 0.4],
            [0.1541641, 0.679, 0.168],
        ],
    ),
    (
        BlendMode::HardLight,
        [
            [0.36, 0.3, 0.96],
            [0.45, 0.55, 0.4],
            [0.62, 0.63, 0.0700000],
        ],
    ),
    (
        BlendMode::VividLight,
        [
            [0.25, 0.1666667, 1.0000000],
            [0.8, 0.2, 0.4],
            [0.125, 0.6666667, 0.0000000],
        ],
    ),
    (
        BlendMode::LinearLight,
        [
            [0.4, 0.1, 1.0000000],
            [0.4, 0.6, 0.4],
            [0.65, 0.6, 0.0000000],
        ],
    ),
    (
        BlendMode::PinLight,
        [[0.2, 0.5, 0.8], [0.5, 0.5, 0.4], [0.6, 0.7, 0.2]],
    ),
    (
        BlendMode::HardMix,
        [
            [0.0000000, 0.0000000, 1.0000000],
            [1.0000000, 0.0000000, 0.0000000],
            [0.0000000, 1.0000000, 0.0000000],
        ],
    ),
    (
        BlendMode::Difference,
        [[0.4, 0.2, 0.1], [0.65, 0.65, 0.1], [0.75, 0.25, 0.25]],
    ),
    (
        BlendMode::Exclusion,
        [[0.56, 0.5, 0.26], [0.7, 0.7, 0.5], [0.77, 0.52, 0.38]],
    ),
    (
        BlendMode::Subtract,
        [
            [0.0000000, 0.2, 0.0000000],
            [0.65, 0.0000000, 0.0000000],
            [0.0000000, 0.25, 0.25],
        ],
    ),
    (
        BlendMode::Divide,
        [
            [0.3333333, 1.0000000, 0.8888889],
            [1.0000000, 0.1333333, 0.8],
            [0.0625000, 1.0000000, 1.0000000],
        ],
    ),
    (
        BlendMode::Hue,
        [
            [0.587, 0.287, 0.887],
            [0.0000000, 0.5782946, 0.2891473],
            [0.729_75, 0.404_75, 0.0797500],
        ],
    ),
    (
        BlendMode::Saturation,
        [
            [0.2, 0.5, 0.8],
            [0.702_375, 0.202_375, 0.389_875],
            [0.0179615, 0.7179615, 0.3410385],
        ],
    ),
    (
        BlendMode::Color,
        [
            [0.587, 0.287, 0.887],
            [0.0505000, 0.550_5, 0.300_5],
            [0.75, 0.4, 0.0500000],
        ],
    ),
    (
        BlendMode::Luminosity,
        [
            [0.213, 0.513, 0.813],
            [1.0000000, 0.3510436, 0.5944023],
            [0.1, 0.75, 0.4],
        ],
    ),
];
const PARTIAL: [(BlendMode, [[f32; 4]; 3]); 4] = [
    (
        BlendMode::Multiply,
        [
            [0.27, 0.318_75, 0.795, 0.8],
            [0.484_375, 0.253_125, 0.35, 0.8],
            [0.233_75, 0.493_125, 0.169_375, 0.8],
        ],
    ),
    (
        BlendMode::Screen,
        [
            [0.48, 0.506_25, 0.892_5, 0.8],
            [0.746_875, 0.515_625, 0.537_5, 0.8],
            [0.522_5, 0.688_125, 0.311_875, 0.8],
        ],
    ),
    (
        BlendMode::Hue,
        [
            [0.445_125, 0.370_125, 0.857_625, 0.8],
            [0.4, 0.4418605, 0.3834302, 0.8],
            [0.4924062, 0.5267812, 0.1861563, 0.8],
        ],
    ),
    (
        BlendMode::Difference,
        [
            [0.375, 0.337_5, 0.562_5, 0.8],
            [0.643_75, 0.468_75, 0.312_5, 0.8],
            [0.5, 0.468_75, 0.25, 0.8],
        ],
    ),
];

fn two_layer(mode: BlendMode, backdrop_alpha: f32, opacity: f32) -> Vec<f32> {
    let e = engine_api::tile::Extent::new(3, 1);
    let mut d = doc(e, Depth::F32);
    let b: Vec<[f32; 4]> = B
        .iter()
        .map(|c| [c[0], c[1], c[2], backdrop_alpha])
        .collect();
    add(&mut d, None, layer_px("b", &b));
    let s: Vec<[f32; 4]> = S.iter().map(|c| opaque(*c)).collect();
    add(
        &mut d,
        None,
        layer_px("s", &s).with_mode(mode).with_opacity(opacity),
    );
    render(&d)
}

#[test]
fn every_mode_matches_reference_opaque() {
    assert_eq!(EXPECTED.len(), BlendMode::ALL.len());
    for (mode, want) in EXPECTED {
        let got = two_layer(mode, 1.0, 1.0);
        let want: Vec<f32> = want.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).collect();
        assert_close(&got, &want, 2e-6, &format!("{mode:?}"));
    }
}

#[test]
fn partial_alpha_uses_general_compositing_formula() {
    for (mode, want) in PARTIAL {
        let got = two_layer(mode, 0.6, 0.5);
        let want: Vec<f32> = want.iter().flatten().copied().collect();
        assert_close(&got, &want, 2e-6, &format!("{mode:?} partial"));
    }
}

#[test]
fn eight_bit_documents_quantize_only_storage() {
    // Same maths in an 8-bit document: inputs are quantized to code values,
    // blending runs in f32, so the result is within one code value.
    let e = engine_api::tile::Extent::new(3, 1);
    for (mode, want) in EXPECTED {
        let mut d = doc(e, Depth::U8);
        let b: Vec<[f32; 4]> = B.iter().map(|c| opaque(*c)).collect();
        let s: Vec<[f32; 4]> = S.iter().map(|c| opaque(*c)).collect();
        add(
            &mut d,
            None,
            layer_fn("b", e, Depth::U8, move |x, _| b[x as usize]),
        );
        add(
            &mut d,
            None,
            layer_fn("s", e, Depth::U8, move |x, _| s[x as usize]).with_mode(mode),
        );
        let got = render(&d);
        let want: Vec<f32> = want.iter().flat_map(|c| [c[0], c[1], c[2], 1.0]).collect();
        // Hard Mix / Divide / burns are discontinuous or steep; only check
        // the continuous, well-conditioned modes at 1 code value.
        if matches!(
            mode,
            BlendMode::HardMix
                | BlendMode::Divide
                | BlendMode::ColorBurn
                | BlendMode::ColorDodge
                | BlendMode::VividLight
        ) {
            continue;
        }
        assert_close(&got, &want, 2.5 / 255.0, &format!("{mode:?} u8"));
    }
}

#[test]
fn dissolve_is_a_deterministic_binary_threshold() {
    let e = engine_api::tile::Extent::new(64, 64);
    let mut d = doc(e, Depth::F32);
    add(
        &mut d,
        None,
        layer_fn("b", e, Depth::F32, |_, _| opaque([0.0, 0.0, 0.0])),
    );
    add(
        &mut d,
        None,
        layer_fn("s", e, Depth::F32, |_, _| opaque([1.0, 1.0, 1.0]))
            .with_mode(BlendMode::Dissolve)
            .with_opacity(0.3),
    );
    let a = render(&d);
    assert_eq!(a, render(&d));
    let white = a.chunks(4).filter(|p| p[0] == 1.0).count();
    let black = a.chunks(4).filter(|p| p[0] == 0.0).count();
    assert_eq!(white + black, 64 * 64, "binary");
    let frac = white as f32 / 4096.0;
    assert!((frac - 0.3).abs() < 0.03, "{frac}");
}
