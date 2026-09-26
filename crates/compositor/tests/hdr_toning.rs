use compositor::adjust::Adjustment;

#[test]
fn hdr_toning_checked_interchange_accepts_native_methods() {
    for method in [
        "local_adaptation",
        "equalize_histogram",
        "exposure_gamma",
        "highlight_compression",
    ] {
        let json = format!(
            r#"{{"version":1,"adjustment":{{"kind":"hdr_toning","settings":{{"method":"{method}"}}}}}}"#
        );
        let decoded = Adjustment::from_versioned_json(&json);
        assert!(decoded.is_ok(), "native HDR method {method}: {decoded:?}");
    }
}

use compositor::adjust::{
    Curve,
    hdr::{HdrMethod, HdrToning},
};

fn close(a: f32, b: f32) {
    assert!((a - b).abs() < 1e-6, "{a} != {b}");
}

#[test]
fn hdr_methods_have_analytic_hdr_results() {
    let exposure = HdrToning {
        method: HdrMethod::ExposureGamma,
        exposure: -2.0,
        gamma: 2.0,
        ..Default::default()
    };
    let rgb = exposure.map_rgb([1.0, 0.25, 4.0], 0.0);
    for (a, b) in rgb.into_iter().zip([0.5, 0.25, 1.0]) {
        close(a, b)
    }
    let highlight = HdrToning {
        method: HdrMethod::HighlightCompression,
        ..Default::default()
    };
    for v in highlight.map_rgb([4.0; 3], 0.0) {
        close(v, 0.8)
    }
    let equalize = HdrToning::equalize_from_histogram(&[1, 2, 1], 4.0).unwrap();
    for v in equalize.map_rgb([2.0; 3], 0.0) {
        close(v, 2.0 / 3.0)
    }
    let local = HdrToning {
        strength: 0.5,
        ..Default::default()
    };
    for v in local.map_rgb([1.0; 3], 1.0) {
        close(v, 2.0 / 3.0)
    }
}

#[test]
fn hdr_local_controls_change_real_pixels_and_identity_preserves_hdr() {
    let original = [0.7, 0.3, 0.2];
    assert_eq!(
        HdrToning::default().map_rgb([4.0, 2.0, -0.1], 0.5),
        [4.0, 2.0, -0.1]
    );
    let neutral = HdrToning::default();
    for setting in [
        HdrToning {
            strength: 0.5,
            ..neutral.clone()
        },
        HdrToning {
            detail: 0.5,
            ..neutral.clone()
        },
        HdrToning {
            gamma: 2.0,
            ..neutral.clone()
        },
        HdrToning {
            exposure: 1.0,
            ..neutral.clone()
        },
        HdrToning {
            shadows: 0.8,
            ..neutral.clone()
        },
        HdrToning {
            vibrance: 0.8,
            ..neutral.clone()
        },
        HdrToning {
            saturation: -0.5,
            ..neutral.clone()
        },
        HdrToning {
            curve: Curve(vec![[0.0, 0.0], [0.5, 0.7], [1.0, 1.0]]),
            ..neutral.clone()
        },
    ] {
        assert_ne!(setting.map_rgb(original, 0.25), original, "{setting:?}");
    }
    let highlight = HdrToning {
        highlights: 0.8,
        ..neutral
    };
    assert!(highlight.map_rgb([0.8; 3], 0.8)[0] < 0.8);
}

#[test]
fn hdr_padded_local_preserves_alpha_and_rejects_missing_halo() {
    let settings = HdrToning {
        strength: 0.7,
        detail: 0.5,
        radius: 4.0,
        ..Default::default()
    };
    assert_eq!(settings.halo(0), 4);
    assert_eq!(settings.halo(2), 1);
    let mut pixels = vec![[0.2, 0.2, 0.2, 0.25]; 81];
    pixels[40] = [0.3, 0.2, 0.1, 0.3];
    let out = settings
        .apply_padded(&pixels, 9, 9, [4, 4, 5, 5], 0)
        .unwrap();
    assert_eq!(out[0][3], 0.3);
    assert_ne!(out[0][0], pixels[40][0]);
    assert!(
        settings
            .apply_padded(&pixels, 9, 9, [3, 4, 5, 5], 0)
            .is_err()
    );
    pixels[40] = [20.0, -2.0, 1.0, 0.0];
    assert_eq!(
        settings
            .apply_padded(&pixels, 9, 9, [4, 4, 5, 5], 0)
            .unwrap()[0],
        pixels[40]
    );
    let mut hidden = pixels.clone();
    hidden[39] = [1e4, 1e4, 1e4, 0.0];
    pixels[39] = [-1e4, -1e4, -1e4, 0.0];
    pixels[40] = [0.3, 0.2, 0.1, 1.0];
    hidden[40] = pixels[40];
    assert_eq!(
        settings
            .apply_padded(&pixels, 9, 9, [4, 4, 5, 5], 0)
            .unwrap(),
        settings
            .apply_padded(&hidden, 9, 9, [4, 4, 5, 5], 0)
            .unwrap()
    );
}

#[test]
fn hdr_validation_and_frozen_histogram_roundtrip() {
    assert!(
        HdrToning {
            gamma: 0.0,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        HdrToning {
            equalize_map: vec![0.5, 0.4],
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        HdrToning {
            radius: f32::NAN,
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(
        HdrToning {
            curve: Curve(vec![[0.5, 0.0], [0.2, 1.0]]),
            ..Default::default()
        }
        .validate()
        .is_err()
    );
    assert!(HdrToning::equalize_from_histogram(&[0], 1.0).is_err());
    let a = Adjustment::HdrToning {
        settings: HdrToning::equalize_from_histogram(&[u64::MAX, 2, u64::MAX], 8.0).unwrap(),
    };
    assert_eq!(
        a,
        Adjustment::from_versioned_json(&a.to_versioned_json().unwrap()).unwrap()
    );
    for h in [[0, 0, 0], [0, 5, 0]] {
        let s = HdrToning::equalize_from_histogram(&h, 4.0).unwrap();
        close(s.map_rgb([2.0; 3], 0.0)[0], 0.5);
    }
}

use compositor::{Compositor, Depth, DocOp, DocState, Document, Layer, Rect};
use engine_api::tile::{Extent, TileCoord};
fn whole_reference(
    s: &HdrToning,
    image: &[[f32; 4]],
    w: usize,
    h: usize,
    level: u8,
) -> Vec<[f32; 4]> {
    let halo = s.halo(level);
    let pw = w + 2 * halo;
    let ph = h + 2 * halo;
    let padded: Vec<_> = (0..ph)
        .flat_map(|y| {
            (0..pw).map(move |x| {
                let xx = (x as isize - halo as isize).clamp(0, w as isize - 1) as usize;
                let yy = (y as isize - halo as isize).clamp(0, h as isize - 1) as usize;
                image[yy * w + xx]
            })
        })
        .collect();
    s.apply_padded(&padded, pw, ph, [halo, halo, halo + w, halo + h], level)
        .unwrap()
}

#[test]
fn hdr_local_render_matches_whole_backdrop_at_l0_l2_seams() {
    let s = HdrToning {
        strength: 0.8,
        detail: 0.5,
        radius: 5.0,
        ..Default::default()
    };
    let e = Extent::new(1040, 8);
    let mut d = Document::new(DocState::new(e, Depth::F32));
    let mut layer = Layer::pixel("backdrop", e, Depth::F32);
    layer
        .raster_mut()
        .unwrap()
        .edit_region(Rect::of_extent(e), 1, |x, _, p| {
            let v = 0.1 + (x % 37) as f32 / 100.0;
            *p = [v, v, v, 1.0];
        })
        .unwrap();
    d.apply(DocOp::AddLayer {
        parent: None,
        index: usize::MAX,
        layer,
    })
    .unwrap();
    let comp = Compositor::new(1 << 24);
    let references: Vec<_> = [0, 2]
        .into_iter()
        .map(|level| {
            let (e, rgba) = comp.render_level_rgba(&d, level).unwrap();
            let image: Vec<_> = rgba
                .as_chunks::<4>()
                .0
                .iter()
                .map(|p| [p[0], p[1], p[2], p[3]])
                .collect();
            (
                level,
                e,
                whole_reference(&s, &image, e.width as usize, e.height as usize, level),
            )
        })
        .collect();
    d.apply(DocOp::AddLayer {
        parent: None,
        index: usize::MAX,
        layer: Layer::new(
            "tone",
            compositor::LayerKind::Adjustment(Adjustment::HdrToning { settings: s }),
        ),
    })
    .unwrap();
    for (level, e, expected) in references {
        for tx in 0..e.width.div_ceil(256) {
            let tile = comp.render_tile(&d, TileCoord::new(level, tx, 0)).unwrap();
            let p = tile.samples::<f32>().unwrap();
            let w = tile.layout().extent.width as usize;
            let n = tile.layout().plane_len();
            for y in 0..e.height as usize {
                for x in 0..w {
                    for c in 0..4 {
                        assert!(
                            (p[c * n + y * w + x]
                                - expected[y * e.width as usize + tx as usize * 256 + x][c])
                                .abs()
                                < 1e-6
                        );
                    }
                }
            }
        }
    }
}
