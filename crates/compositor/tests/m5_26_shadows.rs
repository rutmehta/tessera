//! Contract tests for the local, padded Shadows/Highlights CPU operator.
use compositor::adjust::shadows::ShadowsHighlights;
use compositor::{Adjustment, Compositor, Depth, DocOp, DocState, Document, Layer, Rect};
use engine_api::tile::{Extent, TileCoord};

fn document(settings: ShadowsHighlights, opacity: f32) -> Document {
    let e = Extent::new(3, 1);
    let mut d = Document::new(DocState::new(e, Depth::F32));
    let mut layer = Layer::pixel("backdrop", e, Depth::F32);
    layer
        .raster_mut()
        .unwrap()
        .edit_region(Rect::of_extent(e), 1, |_, _, p| *p = [0.2, 0.2, 0.2, 0.5])
        .unwrap();
    d.apply(DocOp::AddLayer {
        parent: None,
        index: usize::MAX,
        layer,
    })
    .unwrap();
    let mut layer = Layer::new(
        "local tone",
        compositor::LayerKind::Adjustment(Adjustment::ShadowsHighlights { settings }),
    );
    layer.props.opacity = opacity;
    d.apply(DocOp::AddLayer {
        parent: None,
        index: usize::MAX,
        layer,
    })
    .unwrap();
    d
}

fn whole_reference(
    s: &ShadowsHighlights,
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
fn live_neighbourhood_matches_whole_backdrop_at_l0_l2_seams() {
    let s = ShadowsHighlights {
        shadows_amount: 0.8,
        shadows_radius: 5.0,
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
            compositor::LayerKind::Adjustment(Adjustment::ShadowsHighlights { settings: s }),
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

#[test]
fn live_prefixes_preserve_groups_clips_and_refresh_after_adjacent_edits() {
    use compositor::{GroupMode, LayerKind, PaintTarget};
    let s = ShadowsHighlights {
        shadows_amount: 0.65,
        shadows_radius: 4.0,
        ..Default::default()
    };
    for level in [0, 2] {
        for kind in 0..4 {
            let scale = 1u32 << level;
            let e = Extent::new(260 * scale, scale);
            let mut d = Document::new(DocState::new(e, Depth::F32));
            let mut root = Layer::pixel("outside group", e, Depth::F32);
            root.raster_mut()
                .unwrap()
                .edit_region(Rect::of_extent(e), 1, |_, _, p| *p = [0.8, 0.8, 0.8, 1.0])
                .unwrap();
            d.apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: root,
            })
            .unwrap();
            let parent = if kind == 1 || kind == 2 {
                let mut group = Layer::group(
                    "frame",
                    if kind == 1 {
                        GroupMode::Isolated
                    } else {
                        GroupMode::PassThrough
                    },
                );
                group.props.opacity = 0.5;
                Some(
                    d.apply(DocOp::AddLayer {
                        parent: None,
                        index: usize::MAX,
                        layer: group,
                    })
                    .unwrap()
                    .created[0],
                )
            } else {
                None
            };
            let mut base = Layer::pixel("inside frame", e, Depth::F32);
            base.raster_mut()
                .unwrap()
                .edit_region(Rect::of_extent(e), 1, |x, _, p| {
                    let v = if x / scale < 256 { 0.2 } else { 0.3 };
                    *p = [v, v, v, 1.0];
                })
                .unwrap();
            let id = d
                .apply(DocOp::AddLayer {
                    parent,
                    index: usize::MAX,
                    layer: base,
                })
                .unwrap()
                .created[0];
            for _ in 0..2 {
                let mut adj = Layer::new(
                    "sequential tone",
                    LayerKind::Adjustment(Adjustment::ShadowsHighlights {
                        settings: s.clone(),
                    }),
                );
                adj.props.clipped = kind == 3;
                adj.props.opacity = 0.5;
                d.apply(DocOp::AddLayer {
                    parent,
                    index: usize::MAX,
                    layer: adj,
                })
                .unwrap();
            }
            let comp = Compositor::new(1 << 24);
            let cached = Compositor::new(1 << 24);
            let mut previous = None;
            for value in [0.3, 0.15, 0.35] {
                let op = compositor::edit::paint_op(
                    d.state(),
                    id,
                    PaintTarget::Content,
                    Rect::new(256 * scale as i64, 0, 260 * scale as i64, scale as i64),
                    |_, _, p| *p = [value, value, value, 1.0],
                )
                .unwrap();
                d.apply(op).unwrap();
                let mut image: Vec<_> = (0..260)
                    .map(|x| {
                        let v = if x < 256 { 0.2 } else { value };
                        [v, v, v, 1.0]
                    })
                    .collect();
                for _ in 0..2 {
                    let adjusted = whole_reference(&s, &image, 260, 1, level);
                    for (p, a) in image.iter_mut().zip(adjusted) {
                        for c in 0..3 {
                            p[c] += 0.5 * (a[c] - p[c]);
                        }
                    }
                }
                let tile = comp
                    .render_tile_with_neighbourhood(&d, TileCoord::new(level, 0, 0))
                    .unwrap();
                let p = tile.samples::<f32>().unwrap();
                let cached_tile = cached.render_tile(&d, TileCoord::new(level, 0, 0)).unwrap();
                assert_eq!(cached_tile.samples::<f32>().unwrap(), p);
                for x in 0..256 {
                    let expected = if kind == 1 || kind == 2 {
                        0.8 + 0.5 * (image[x][0] - 0.8)
                    } else {
                        image[x][0]
                    };
                    assert!(
                        (p[x] - expected).abs() < 1e-6,
                        "kind={kind}, level={level}, x={x}: {} != {expected}",
                        p[x]
                    );
                }
                if let Some(old) = previous {
                    assert_ne!(old, p[255], "adjacent edit must change warm edge");
                }
                previous = Some(p[255]);
                assert_eq!(p[768], 1.0);
                assert_eq!(
                    comp.stats().cache_entries,
                    0,
                    "fallback must not populate caller caches"
                );
            }
        }
    }
}

#[test]
fn live_neighbourhood_respects_mask_mode_opacity_and_alpha() {
    let s = ShadowsHighlights {
        shadows_amount: 0.8,
        shadows_radius: 2.0,
        ..Default::default()
    };
    let expected = whole_reference(&s, &[[0.2, 0.2, 0.2, 0.5]; 3], 3, 1, 0);
    let mut d = document(s, 0.5);
    let layer = d.state().root[1].clone();
    let mut props = layer.props.clone();
    props.blend_mode = compositor::BlendMode::Multiply;
    d.apply(DocOp::SetProps {
        id: layer.id,
        props,
    })
    .unwrap();
    let mut mask = compositor::Mask::reveal_all(Extent::new(3, 1), Depth::F32);
    mask.raster
        .edit_region(Rect::new(0, 0, 3, 1), 1, |x, _, p| p[0] = x as f32 / 2.0)
        .unwrap();
    d.apply(DocOp::SetMask {
        id: layer.id,
        mask: Some(mask),
    })
    .unwrap();
    let tile = Compositor::new(1 << 20)
        .render_tile_with_neighbourhood(&d, TileCoord::new(0, 0, 0))
        .unwrap();
    let p = tile.samples::<f32>().unwrap();
    for x in 0..3 {
        let want = 0.2 + 0.5 * (x as f32 / 2.0) * (0.2 * expected[x][0] - 0.2);
        assert!((p[x] - want).abs() < 1e-6);
        assert_eq!(p[9 + x], 0.5);
    }
}

#[test]
fn live_neighbourhood_rejects_eager_styled_sources() {
    use compositor::render::styles::{Shadow, StyleEffect};
    let mut d = document(
        ShadowsHighlights {
            shadows_amount: 0.5,
            shadows_radius: 2.0,
            ..Default::default()
        },
        1.0,
    );
    let layer = d.state().root[0].clone();
    let mut props = layer.props.clone();
    props
        .styles
        .effects
        .push(StyleEffect::DropShadow(Shadow::default()));
    d.apply(DocOp::SetProps {
        id: layer.id,
        props,
    })
    .unwrap();
    assert!(matches!(
        Compositor::new(0).render_tile_with_neighbourhood(&d, TileCoord::new(0, 0, 0)),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}

#[test]
fn cached_neighborhood_render_matches_live_reference() {
    let d = document(
        ShadowsHighlights {
            shadows_amount: 0.5,
            ..Default::default()
        },
        1.0,
    );
    let comp = Compositor::new(1 << 20);
    let coord = TileCoord::new(0, 0, 0);
    let expected = comp.render_tile_with_neighbourhood(&d, coord).unwrap();
    let actual = comp.render_tile(&d, coord).unwrap();
    assert_eq!(
        actual.samples::<f32>().unwrap(),
        expected.samples::<f32>().unwrap()
    );
}

#[test]
fn zero_radius_render_respects_opacity_and_alpha() {
    let s = ShadowsHighlights {
        shadows_amount: 0.8,
        shadows_radius: 0.0,
        ..Default::default()
    };
    let want = s
        .apply_padded(&[[0.2, 0.2, 0.2, 0.5]], 1, 1, [0, 0, 1, 1], 0)
        .unwrap()[0];
    let d = document(s, 0.5);
    let tile = Compositor::new(1 << 20)
        .render_tile(&d, TileCoord::new(0, 0, 0))
        .unwrap();
    let p = tile.samples::<f32>().unwrap();
    assert!((p[0] - (0.2 + want[0]) * 0.5).abs() < 1e-6);
    assert_eq!(p[9], 0.5);
}

#[test]
fn local_radius_changes_mapping_without_changing_alpha() {
    let pixels: Vec<_> = (0..9)
        .map(|i| {
            let v = if i == 4 { 0.2 } else { 0.35 };
            [v, v, v, 0.7]
        })
        .collect();
    let mut s = ShadowsHighlights {
        shadows_amount: 0.8,
        shadows_radius: 0.0,
        ..Default::default()
    };
    let point = s.apply_padded(&pixels, 9, 1, [4, 0, 5, 1], 0).unwrap()[0];
    assert!(point[0] > 0.2);
    s.shadows_radius = 2.0;
    assert!(s.needs_neighbourhood());
    // Explicit replicated document-edge padding in Y; X uses real neighbours.
    let padded: Vec<_> = (0..5).flat_map(|_| pixels.iter().copied()).collect();
    let local = s.apply_padded(&padded, 9, 5, [4, 2, 5, 3], 0).unwrap()[0];
    assert!((point[0] - local[0]).abs() > 1e-4);
    assert_eq!(local[3], 0.7);
}

#[test]
fn invalid_controls_and_missing_halo_are_errors() {
    let p = [[0.2, 0.3, 0.4, 1.0]; 9];
    for bad in [f32::NAN, f32::INFINITY, -1.0] {
        let s = ShadowsHighlights {
            shadows_amount: bad,
            ..Default::default()
        };
        assert!(s.apply_padded(&p, 3, 3, [0, 0, 3, 3], 0).is_err());
    }
    let s = ShadowsHighlights {
        black_clip: 0.6,
        white_clip: 0.5,
        ..Default::default()
    };
    assert!(s.apply_padded(&p, 3, 3, [0, 0, 3, 3], 0).is_err());
    let s = ShadowsHighlights {
        shadows_amount: 0.8,
        shadows_radius: 2.0,
        ..Default::default()
    };
    assert!(s.apply_padded(&p, 3, 3, [1, 1, 2, 2], 0).is_err());
    assert!(s.apply_padded(&p, usize::MAX, 2, [0, 0, 1, 1], 0).is_err());
}

#[test]
fn nonfinite_backdrop_is_rejected() {
    let s = ShadowsHighlights::default();
    assert!(
        s.apply_padded(&[[f32::NAN, 0.0, 0.0, 1.0]], 1, 1, [0, 0, 1, 1], 0)
            .is_err()
    );
}

#[test]
fn padded_tiles_match_whole_image_across_256_boundary_and_neighbour_edit() {
    let s = ShadowsHighlights {
        shadows_amount: 0.8,
        highlights_amount: 0.6,
        shadows_radius: 2.0,
        highlights_radius: 3.0,
        ..Default::default()
    };
    let (w, h) = (520, 3);
    let mut image: Vec<_> = (0..w * h)
        .map(|i| {
            let v = 0.1 + (i % 37) as f32 / 50.0;
            [v, v, v, 1.0]
        })
        .collect();
    let tile = |image: &[[f32; 4]], start: usize, end: usize| {
        let halo = s.halo(0);
        let pw = end - start + 2 * halo;
        let ph = h + 2 * halo;
        let padded: Vec<_> = (0..ph)
            .flat_map(|y| {
                (0..pw).map(move |x| {
                    let xx = (start as isize + x as isize - halo as isize).clamp(0, w as isize - 1)
                        as usize;
                    let yy = (y as isize - halo as isize).clamp(0, h as isize - 1) as usize;
                    image[yy * w + xx]
                })
            })
            .collect();
        s.apply_padded(&padded, pw, ph, [halo, halo, pw - halo, ph - halo], 0)
            .unwrap()
    };
    let before = tile(&image, 0, 256);
    image[256] = [0.2, 0.2, 0.2, 1.0];
    let after = tile(&image, 0, 256);
    assert_ne!(
        before[255], after[255],
        "neighbour edit must affect edge pixel"
    );
    let whole = tile(&image, 0, w);
    for (start, end) in [(0, 256), (256, 512), (512, w)] {
        let part = tile(&image, start, end);
        for y in 0..h {
            assert_eq!(
                &part[y * (end - start)..(y + 1) * (end - start)],
                &whole[y * w + start..y * w + end]
            );
        }
    }
}

#[test]
fn highlights_have_independent_tone_and_radius_and_mip_support() {
    let mut s = ShadowsHighlights {
        highlights_amount: 0.8,
        highlights_radius: 0.0,
        ..Default::default()
    };
    let p = [[0.8, 0.8, 0.8, 0.4]];
    let lowered = s.apply_padded(&p, 1, 1, [0, 0, 1, 1], 0).unwrap()[0];
    assert!(lowered[0] < p[0][0]);
    s.highlights_tone = 0.0;
    assert_eq!(s.apply_padded(&p, 1, 1, [0, 0, 1, 1], 0).unwrap(), p);
    s.highlights_tone = 0.5;
    s.highlights_radius = 5.0;
    assert_eq!(s.halo(0), 5);
    assert_eq!(s.halo(1), 3);
    assert_eq!(s.halo(3), 1);
    let mut padded = [[0.65, 0.65, 0.65, 0.4]; 9];
    padded[4] = p[0];
    let local = s.apply_padded(&padded, 3, 3, [1, 1, 2, 2], 3).unwrap()[0];
    assert_ne!(local[0], lowered[0]);
    assert_eq!(local[3], 0.4);
}

#[test]
fn transparent_neighbours_do_not_bias_bilateral_base() {
    let s = ShadowsHighlights {
        shadows_amount: 0.8,
        shadows_radius: 1.0,
        ..Default::default()
    };
    let mut a = [[0.0; 4]; 9];
    a[4] = [0.2, 0.2, 0.2, 0.5];
    let mut b = [[0.4, 0.4, 0.4, 0.0]; 9];
    b[4] = a[4];
    assert_eq!(
        s.apply_padded(&a, 3, 3, [1, 1, 2, 2], 0).unwrap(),
        s.apply_padded(&b, 3, 3, [1, 1, 2, 2], 0).unwrap()
    );
}

#[test]
fn color_midtone_and_endpoint_clip_controls_are_effective() {
    let p = [[0.2, 0.3, 0.4, 0.5]];
    for s in [
        ShadowsHighlights {
            color: -1.0,
            ..Default::default()
        },
        ShadowsHighlights {
            midtone: 0.5,
            ..Default::default()
        },
        ShadowsHighlights {
            black_clip: 0.1,
            ..Default::default()
        },
        ShadowsHighlights {
            white_clip: 0.1,
            ..Default::default()
        },
    ] {
        let q = s.apply_padded(&p, 1, 1, [0, 0, 1, 1], 0).unwrap()[0];
        assert_ne!(q, p[0]);
        assert_eq!(q[3], p[0][3]);
    }
}

#[test]
fn default_is_exact_identity_and_roundtrips() {
    let s = ShadowsHighlights::default();
    let pixels = [[0.2, 0.3, 0.4, 0.5], [-0.2, 1.2, 2.0, 1.0], [0.0; 4]];
    assert_eq!(
        s.apply_padded(&pixels, 3, 1, [0, 0, 3, 1], 0).unwrap(),
        pixels
    );
    assert_eq!(
        serde_json::from_str::<ShadowsHighlights>(&serde_json::to_string(&s).unwrap()).unwrap(),
        s
    );
    assert!(!s.needs_neighbourhood());
}

#[test]
fn native_separable_bilateral_reference_and_edge_preservation() {
    // Independent two-pass reference. This asymmetric fixture distinguishes
    // horizontal-then-vertical bilateral from the previous direct 2D operator.
    let mut pixels = [[0.0; 4]; 25];
    for (i, p) in pixels.iter_mut().enumerate() {
        let l = 0.05 + ((i * 7) % 13) as f32 * 0.04;
        *p = [l, l, l, if i % 7 == 0 { 0.0 } else { 0.8 }];
    }
    let tap = |values: &[f32], x: usize, y: usize, vertical: bool| {
        let center = values[y * 5 + x];
        let mut sum = 0.0;
        let mut weights = 0.0;
        for d in -1isize..=1 {
            let xx = (x as isize + if vertical { 0 } else { d }).clamp(0, 4) as usize;
            let yy = (y as isize + if vertical { d } else { 0 }).clamp(0, 4) as usize;
            let j = yy * 5 + xx;
            let w =
                (-(d * d) as f32 / 0.5 - (values[j] - center).powi(2) / 0.045).exp() * pixels[j][3];
            sum += w * values[j];
            weights += w;
        }
        if weights > 0.0 { sum / weights } else { center }
    };
    let luma: Vec<_> = pixels
        .iter()
        .map(|p| 0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2])
        .collect();
    let h: Vec<_> = (0..25).map(|i| tap(&luma, i % 5, i / 5, false)).collect();
    let base = tap(&h, 2, 2, true);
    let settings = ShadowsHighlights {
        shadows_amount: 1.0,
        shadows_tone: 1.0,
        shadows_radius: 1.0,
        ..Default::default()
    };
    let actual = settings
        .apply_padded(&pixels, 5, 5, [2, 2, 3, 3], 0)
        .unwrap()[0];
    let t = 1.0 - base;
    let expected = luma[12] + 0.5 * t * t * (3.0 - 2.0 * t) * (1.0 - luma[12]);
    assert!(
        (actual[0] - expected).abs() < 1e-6,
        "{} != {}",
        actual[0],
        expected
    );
    let edge: Vec<_> = (0..49)
        .map(|i| {
            let l = if i % 7 < 3 { 0.0 } else { 1.0 };
            [l, l, l, 1.0]
        })
        .collect();
    let mapped = settings.apply_padded(&edge, 7, 7, [2, 3, 4, 4], 0).unwrap();
    assert!(
        (mapped[0][0] - 0.5).abs() < 1e-6,
        "bilateral must preserve dark side of a hard edge"
    );
    assert!((mapped[1][0] - 1.0).abs() < 1e-6);
}
