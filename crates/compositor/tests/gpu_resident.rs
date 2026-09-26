//! The GPU-resident renderer against the CPU reference (docs/11 §1.3: per
//! operator ≤ 1e-4, a full multi-layer chain ≤ 2e-3), plus dirty-rect
//! exactness, determinism, page sharing, presentation and the premultiplied
//! and level-count contracts.
mod common;
use common::*;
use compositor::gpu::GpuCompositor;
use compositor::resident::ResidentRenderer;
use compositor::*;
use engine_api::tile::{Extent, Pyramid, TileCoord};

fn gpu() -> Option<GpuCompositor> {
    // CI runners have no real Metal device (no exact-math path); these gates need one.
    if std::env::var_os("CI").is_some() {
        eprintln!("skipping: CI runner without a Metal device");
        return None;
    }
    match GpuCompositor::new() {
        Ok(g) => Some(g),
        Err(e) => {
            eprintln!("skipping: no Metal adapter ({e})");
            None
        }
    }
}

fn wave(seed: u32) -> impl Fn(u32, u32) -> [f32; 4] {
    move |x, y| {
        let f =
            |k: u32| (((x * (3 + k) + y * (5 + 2 * k) + seed * 37 + k * 11) % 97) as f32) / 96.0;
        [f(0), f(1), f(2), 0.25 + 0.75 * f(3)]
    }
}

/// max |resident − CPU| over every tile of `level` (premultiplied).
fn worst(r: &mut ResidentRenderer, d: &Document, level: u8) -> f32 {
    r.render(d, level).unwrap();
    let got = r.read_tiles(level).unwrap();
    let cpu = Compositor::new(256 << 20);
    let mut worst = 0.0f32;
    for g in &got {
        assert!(g.premultiplied());
        let want = cpu.render_tile_premultiplied(d, g.coord()).unwrap();
        for (p, q) in want
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(g.samples::<f32>().unwrap())
        {
            assert!(q.is_finite());
            worst = worst.max((p - q).abs());
        }
    }
    worst
}

fn bits(r: &ResidentRenderer, level: u8) -> Vec<u32> {
    r.read_level(level, true)
        .unwrap()
        .1
        .iter()
        .map(|v| v.to_bits())
        .collect()
}

/// Specialized kernels and the interpreter are compiled with IEEE maths from
/// the same formulas: every mode, group kind, knockout, Blend If, mask,
/// fill and adjustment of the chain scene is bit-identical between them
/// and across devices, and matches the CPU.
#[test]
fn specialized_matches_interpreter_and_reuses_structure() {
    let g = gpu().expect("Metal required");
    let mut d = scene(Depth::U8, Extent::new(31, 19));
    let mut fast = ResidentRenderer::new(&g).unwrap();
    let mut general = ResidentRenderer::new(&g).unwrap();
    general.set_specialization(false);
    // The first frame renders with the interpreter while the kernel
    // compiles in the background.
    fast.render(&d, 0).unwrap();
    assert_eq!(fast.specialized_pipeline_count(), 0);
    fast.wait_for_specializations();
    assert_eq!(fast.specialized_pipeline_count(), 1);
    fast.invalidate();
    assert_eq!(worst(&mut fast, &d, 0), 0.0);
    general.render(&d, 0).unwrap();
    assert_eq!(bits(&fast, 0), bits(&general, 0));
    let id = d.state().root[0].id;
    set_props(&mut d, id, |p| p.opacity = 0.6);
    fast.render(&d, 0).unwrap();
    fast.wait_for_specializations();
    assert_eq!(fast.specialized_pipeline_count(), 1);
    fast.invalidate();
    fast.render(&d, 0).unwrap();
    let before = bits(&fast, 0);
    fast.invalidate();
    fast.render(&d, 0).unwrap();
    assert_eq!(before, bits(&fast, 0));
    general.render(&d, 0).unwrap();
    assert_eq!(before, bits(&general, 0));
    let second_device = gpu().expect("second Metal device");
    let mut second = ResidentRenderer::new(&second_device).unwrap();
    second.render(&d, 0).unwrap();
    assert_eq!(before, bits(&second, 0));
    second.wait_for_specializations();
    second.invalidate();
    second.render(&d, 0).unwrap();
    assert_eq!(before, bits(&second, 0));
}

#[test]
fn viewport_pan_preserves_offscreen_damage() {
    let g = gpu().expect("Metal required");
    let e = Extent::new(97, 65);
    let mut d = doc(e, Depth::F32);
    let id = add(&mut d, None, layer_fn("pixels", e, Depth::F32, wave(3)));
    let mut r = ResidentRenderer::new(&g).unwrap();
    let a = Rect::new(1, 1, 15, 15);
    assert_eq!(r.render_viewport(&d, 0, a, 0).unwrap().blocks, 1);
    assert!(r.read_level(0, true).is_err());
    assert_eq!(r.render_viewport(&d, 0, a, 0).unwrap().blocks, 0);
    assert_eq!(r.render_viewport(&d, 0, a, 2).unwrap().blocks, 3);
    let b = Rect::new(64, 32, 96, 64);
    assert_eq!(r.render_viewport(&d, 0, b, 0).unwrap().blocks, 4);
    let op = paint_op(d.state(), id, PaintTarget::Content, a, |_, _, p| p[0] = 0.9).unwrap();
    d.apply(op).unwrap();
    assert_eq!(r.render_viewport(&d, 0, b, 0).unwrap().blocks, 0);
    assert_eq!(r.render_viewport(&d, 0, a, 0).unwrap().blocks, 1);
    r.render(&d, 0).unwrap();
    let mut cold = ResidentRenderer::new(&g).unwrap();
    cold.render(&d, 0).unwrap();
    assert_eq!(bits(&r, 0), bits(&cold, 0));
    assert_eq!(
        r.render_viewport(&d, 1, Rect::new(0, 0, 16, 16), 0)
            .unwrap()
            .blocks,
        1
    );
    assert!(
        r.render_viewport(&d, 0, Rect::new(200, 200, 300, 300), 0)
            .is_err()
    );
}

/// A viewport frame interns, uploads and mips only the tiles under the
/// viewport; offscreen edits and history jumps stay sound.
#[test]
fn viewport_resolves_only_visible_tiles() {
    let g = gpu().expect("Metal required");
    let e = Extent::new(1000, 1000);
    let mut d = doc(e, Depth::U8);
    let a = add(&mut d, None, layer_fn("a", e, Depth::U8, wave(1)));
    add(
        &mut d,
        None,
        layer_fn("b", e, Depth::U8, wave(2)).with_mode(BlendMode::Divide),
    );
    let mut r = ResidentRenderer::new(&g).unwrap();
    // L0 viewport inside tile (0, 0): one page per layer.
    let f = r
        .render_viewport(&d, 0, Rect::new(10, 10, 100, 90), 0)
        .unwrap();
    assert_eq!((f.uploaded_pages, f.mip_pages), (2, 0));
    // L1 tile (1, 0) needs L0 tiles (2..4, 0..2) and one mip per layer.
    let f = r
        .render_viewport(&d, 1, Rect::new(300, 0, 310, 10), 0)
        .unwrap();
    assert_eq!((f.uploaded_pages, f.mip_pages), (8, 2));
    // Offscreen paint: nothing under the viewport changes.
    let op = paint_op(
        d.state(),
        a,
        PaintTarget::Content,
        Rect::new(900, 900, 950, 950),
        |_, _, p| p[1] = 0.25,
    )
    .unwrap();
    d.apply(op).unwrap();
    let f = r
        .render_viewport(&d, 0, Rect::new(10, 10, 100, 90), 0)
        .unwrap();
    assert_eq!((f.blocks, f.uploaded_pages), (0, 0));
    let full = r.render(&d, 0).unwrap();
    assert_eq!(full.uploaded_pages, 2 * 16 - 10);
    let mut cold = ResidentRenderer::new(&g).unwrap();
    cold.render(&d, 0).unwrap();
    assert_eq!(bits(&r, 0), bits(&cold, 0));
    // Undo (no damage log) while only a viewport is rendered: the
    // offscreen blocks are invalid, and completing them is exact.
    assert!(d.undo());
    r.render_viewport(&d, 0, Rect::new(10, 10, 100, 90), 0)
        .unwrap();
    assert!(r.read_level(0, true).is_err());
    r.render(&d, 0).unwrap();
    let mut cold = ResidentRenderer::new(&g).unwrap();
    cold.render(&d, 0).unwrap();
    assert_eq!(bits(&r, 0), bits(&cold, 0));
    assert_eq!(worst(&mut r, &d, 0), 0.0);
}

#[test]
fn smart_transform_invalidates_even_with_newer_child_revision() {
    let g = gpu().expect("Metal required");
    let e = Extent::new(48, 32);
    let mut child = doc(e, Depth::F32);
    add(&mut child, None, layer_fn("child", e, Depth::F32, wave(4)));
    let mut state = (**child.state()).clone();
    state.rev = 10000;
    let mut d = doc(e, Depth::F32);
    let id = add(
        &mut d,
        None,
        Layer::new(
            "smart",
            LayerKind::SmartObject(SmartObject::new(state, Affine::IDENTITY)),
        ),
    );
    let mut r = ResidentRenderer::new(&g).unwrap();
    assert!(worst(&mut r, &d, 0) < 1e-4);
    d.apply(DocOp::SetSmartTransform {
        id,
        transform: Affine::scale_translate(0.8, 0.8, 2.5, 1.5),
    })
    .unwrap();
    assert!(worst(&mut r, &d, 0) < 1e-4);
    assert!(d.undo());
    assert!(worst(&mut r, &d, 0) < 1e-4);
    assert!(d.redo());
    assert!(worst(&mut r, &d, 0) < 1e-4);
}

#[test]
fn specialization_cache_is_bounded_and_large_programs_fall_back() {
    let g = gpu().expect("Metal required");
    let e = Extent::new(3, 2);
    let mut d = doc(e, Depth::F32);
    add(&mut d, None, layer_fn("base", e, Depth::F32, wave(1)));
    let id = add(&mut d, None, layer_fn("top", e, Depth::F32, wave(2)));
    let mut r = ResidentRenderer::new(&g).unwrap();
    for mode in BlendMode::ALL.into_iter().take(10) {
        set_props(&mut d, id, |p| p.blend_mode = mode);
        r.render(&d, 0).unwrap();
        r.wait_for_specializations();
        r.invalidate();
        assert_eq!(worst(&mut r, &d, 0), 0.0);
        assert!(r.specialized_pipeline_count() <= 8);
    }
    assert_eq!(r.specialized_pipeline_count(), 8);
    for _ in 0..260 {
        d.apply(DocOp::DuplicateLayer { id }).unwrap();
    }
    let mut fallback = ResidentRenderer::new(&g).unwrap();
    assert_eq!(worst(&mut fallback, &d, 0), 0.0);
    fallback.wait_for_specializations();
    assert_eq!(fallback.specialized_pipeline_count(), 0);
}

fn adjustments() -> Vec<Adjustment> {
    vec![
        Adjustment::Invert,
        Adjustment::Exposure {
            exposure: 0.7,
            offset: -0.02,
            gamma: 1.3,
        },
        Adjustment::Threshold { level: 0.45 },
        Adjustment::Posterize { levels: 5 },
        Adjustment::Levels {
            master: LevelsChannel {
                in_black: 0.05,
                in_white: 0.9,
                gamma: 1.4,
                out_black: 0.02,
                out_white: 0.97,
            },
            rgb: [
                LevelsChannel::default(),
                LevelsChannel {
                    gamma: 0.8,
                    ..Default::default()
                },
                LevelsChannel::default(),
            ],
        },
        Adjustment::Curves {
            master: Curve(vec![[0.0, 0.0], [0.3, 0.2], [0.7, 0.85], [1.0, 1.0]]),
            rgb: [
                Curve::default(),
                Curve(vec![[0.0, 0.1], [1.0, 0.9]]),
                Curve::default(),
            ],
        },
        Adjustment::HueSaturation {
            hue: 40.0,
            saturation: -30.0,
            lightness: 10.0,
            colorize: false,
        },
        Adjustment::HueSaturation {
            hue: -120.0,
            saturation: 60.0,
            lightness: -20.0,
            colorize: true,
        },
        Adjustment::ChannelMixer {
            matrix: [[0.8, 0.3, -0.1], [0.1, 0.7, 0.2], [0.0, -0.2, 1.1]],
            constant: [0.02, 0.0, -0.03],
            monochrome: false,
        },
        Adjustment::ChannelMixer {
            matrix: [[0.3, 0.59, 0.11], [0.0; 3], [0.0; 3]],
            constant: [0.0; 3],
            monochrome: true,
        },
    ]
}

/// The per-tile GPU gate's 39-node chain plus adjustment layers, fills,
/// masks and a clip group, in `depth`, with an odd canvas.
fn scene(depth: Depth, e: Extent) -> Document {
    let mut d = doc(e, depth);
    let (w, h) = (e.width as f32, e.height as f32);
    let bg = add(
        &mut d,
        None,
        layer_fn("bg", e, depth, move |x, y| {
            opaque([x as f32 / w, y as f32 / h, 0.4])
        }),
    );
    set_props(&mut d, bg, |p| p.background = true);
    for (i, m) in BlendMode::ALL.into_iter().enumerate() {
        let id = add(
            &mut d,
            None,
            layer_fn("m", e, depth, wave(i as u32)).with_mode(m),
        );
        set_props(&mut d, id, |p| {
            p.opacity = 0.4 + 0.02 * i as f32;
            p.fill_opacity = 1.0 - 0.01 * i as f32;
        });
    }
    let pt = add(
        &mut d,
        None,
        Layer::group("pt", GroupMode::PassThrough).with_opacity(0.8),
    );
    add(
        &mut d,
        Some(pt),
        layer_fn("a", e, depth, wave(40)).with_mode(BlendMode::Overlay),
    );
    let ko = add(&mut d, Some(pt), layer_fn("ko", e, depth, wave(41)));
    set_props(&mut d, ko, |p| {
        p.knockout = Knockout::Shallow;
        p.fill_opacity = 0.3;
    });
    // An adjustment inside the pass-through group reaches the backdrop.
    let adj = add(
        &mut d,
        Some(pt),
        Layer::new("curves", LayerKind::Adjustment(adjustments()[5].clone())),
    );
    set_props(&mut d, adj, |p| p.opacity = 0.7);
    let iso = add(
        &mut d,
        None,
        Layer::group("iso", GroupMode::Isolated).with_mode(BlendMode::SoftLight),
    );
    let mut m = Mask::reveal_all(e, depth);
    m.raster
        .edit_region(
            Rect::new(0, 0, e.width as i64 / 2, e.height as i64),
            1,
            move |x, _, p| p[0] = x as f32 / (w / 2.0),
        )
        .unwrap();
    d.apply(DocOp::SetMask {
        id: iso,
        mask: Some(m),
    })
    .unwrap();
    add(
        &mut d,
        Some(iso),
        layer_fn("b", e, depth, wave(50)).with_mode(BlendMode::Hue),
    );
    let bi = add(&mut d, Some(iso), layer_fn("bi", e, depth, wave(51)));
    set_props(&mut d, bi, |p| {
        p.blend_if.gray.underlying = [0.1, 0.3, 0.7, 0.9];
        p.blend_if.rgb[1].this_layer = [0.0, 0.2, 0.8, 1.0];
    });
    let deep = add(&mut d, Some(iso), layer_fn("deep", e, depth, wave(52)));
    set_props(&mut d, deep, |p| {
        p.knockout = Knockout::Deep;
        p.fill_opacity = 0.5;
        p.opacity = 0.7;
    });
    add(
        &mut d,
        None,
        layer_fn("base", e, depth, wave(60)).with_mode(BlendMode::Multiply),
    );
    let c1 = add(
        &mut d,
        None,
        layer_fn("c1", e, depth, wave(61)).with_mode(BlendMode::ColorDodge),
    );
    set_props(&mut d, c1, |p| p.clipped = true);
    let c2 = add(
        &mut d,
        None,
        layer_fn("c2", e, depth, wave(62))
            .with_mode(BlendMode::Dissolve)
            .with_opacity(0.6),
    );
    set_props(&mut d, c2, |p| p.clipped = true);
    add(
        &mut d,
        None,
        Layer::new(
            "grad",
            LayerKind::Fill(Fill::Gradient {
                gradient: GradientKind::Radial,
                start: [w / 2.0, h / 2.0],
                end: [w, h / 2.0],
                stops: vec![
                    GradientStop {
                        position: 0.0,
                        color: [1.0, 0.8, 0.2, 0.6],
                    },
                    GradientStop {
                        position: 1.0,
                        color: [0.1, 0.2, 0.9, 0.0],
                    },
                ],
            }),
        )
        .with_mode(BlendMode::LinearLight),
    );
    // Masked hue/saturation adjustment with Blend If at the top.
    let hs = add(
        &mut d,
        None,
        Layer::new("hs", LayerKind::Adjustment(adjustments()[6].clone())),
    );
    let mut m = Mask::hide_all(e, depth);
    m.raster
        .edit_region(
            Rect::new(10, 10, e.width as i64 - 30, e.height as i64 - 20),
            1,
            |x, y, p| p[0] = ((x + y) % 50) as f32 / 49.0,
        )
        .unwrap();
    d.apply(DocOp::SetMask {
        id: hs,
        mask: Some(m),
    })
    .unwrap();
    set_props(&mut d, hs, |p| {
        p.blend_if.gray.this_layer = [0.0, 0.1, 0.9, 1.0];
        p.blend_mode = BlendMode::Color;
    });
    add(
        &mut d,
        None,
        Layer::new(
            "pattern",
            LayerKind::Fill(Fill::Pattern {
                width: 3,
                height: 2,
                rgba: (0..24).map(|i| (i % 7) as f32 / 6.0).collect(),
                origin: [1.5, -0.5],
            }),
        )
        .with_mode(BlendMode::Screen)
        .with_opacity(0.3),
    );
    d
}

#[test]
fn resident_chain_matches_cpu_at_every_depth_and_level() {
    let Some(gpu) = gpu() else { return };
    for depth in [Depth::F32, Depth::U16, Depth::U8] {
        let d = scene(depth, Extent::new(301, 290));
        let mut r = ResidentRenderer::new(&gpu).unwrap();
        for level in [0u8, 1, 2, 4, 9] {
            let w = worst(&mut r, &d, level);
            eprintln!("{depth:?} L{level}: chain max |gpu − cpu| = {w:e}");
            // The docs/11 bound is 2e-3. IEEE compilation (division,
            // sqrt, no contraction) makes the chain bit-exact, which is
            // what keeps Hard Mix / Divide / Darker Colour from amplifying
            // one-ulp drift on large documents: gate exactness.
            assert!(w == 0.0, "{depth:?} L{level}: {w:e}");
        }
    }
}

#[test]
fn every_mode_and_adjustment_within_operator_tolerance() {
    let Some(gpu) = gpu() else { return };
    let e = Extent::new(260, 140);
    let base = |d: &mut Document| {
        add(
            d,
            None,
            layer_fn("bg", e, Depth::F32, |x, y| {
                opaque([x as f32 / 300.0, y as f32 / 140.0, 0.4])
            }),
        );
        for j in 0..3 {
            add(
                d,
                None,
                layer_fn("m", e, Depth::F32, wave(j + 30)).with_opacity(0.7),
            );
        }
    };
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    for (i, m) in BlendMode::ALL.into_iter().enumerate() {
        let mut d = doc(e, Depth::F32);
        base(&mut d);
        let id = add(
            &mut d,
            None,
            layer_fn("m", e, Depth::F32, wave(i as u32)).with_mode(m),
        );
        set_props(&mut d, id, |p| {
            p.opacity = 0.4 + 0.02 * i as f32;
            p.fill_opacity = 1.0 - 0.01 * i as f32;
        });
        let w = worst(&mut r, &d, 0);
        eprintln!("{m:?}: {w:e}");
        assert!(w <= 1e-4, "{m:?}: {w:e}");
    }
    for (i, a) in adjustments().into_iter().enumerate() {
        for depth in [Depth::F32, Depth::U8] {
            let mut d = doc(e, depth);
            add(
                &mut d,
                None,
                layer_fn("bg", e, depth, |x, y| {
                    [
                        x as f32 / 260.0,
                        y as f32 / 140.0,
                        0.4,
                        0.3 + 0.7 * (x % 5) as f32 / 4.0,
                    ]
                }),
            );
            let id = add(
                &mut d,
                None,
                Layer::new("adj", LayerKind::Adjustment(a.clone())),
            );
            set_props(&mut d, id, |p| {
                p.opacity = 0.9;
                p.fill_opacity = 0.8;
                if i % 2 == 1 {
                    p.blend_mode = BlendMode::Overlay;
                }
            });
            let w = worst(&mut r, &d, 0);
            eprintln!("{a:?} {depth:?}: {w:e}");
            // Threshold and Posterize are step functions: a 1-ulp
            // difference at a step flips a whole code value.
            let steps = matches!(
                a,
                Adjustment::Threshold { .. } | Adjustment::Posterize { .. }
            );
            assert!(w <= if steps { 2e-3 } else { 1e-4 }, "{a:?}: {w:e}");
        }
    }
}

#[test]
fn integer_mips_are_exact() {
    let Some(gpu) = gpu() else { return };
    // Odd extents at every level; mask with reveal default; semi-transparent
    // content so the alpha-weighted mean is exercised.
    for depth in [Depth::U8, Depth::U16] {
        let e = Extent::new(1037, 555);
        let mut d = doc(e, depth);
        let id = add(
            &mut d,
            None,
            layer_fn("a", e, depth, |x, y| {
                [
                    ((x * 7 + y * 3) % 256) as f32 / 255.0,
                    ((x ^ y) % 256) as f32 / 255.0,
                    (y % 256) as f32 / 255.0,
                    ((x * 13 + y * 5) % 256) as f32 / 255.0,
                ]
            }),
        );
        let mut m = Mask::reveal_all(e, depth);
        m.raster
            .edit_region(Rect::new(100, 50, 700, 400), 1, |x, y, p| {
                p[0] = ((x * y) % 256) as f32 / 255.0
            })
            .unwrap();
        d.apply(DocOp::SetMask { id, mask: Some(m) }).unwrap();
        let mut r = ResidentRenderer::new(&gpu).unwrap();
        for level in 0..=11u8 {
            // Identical mips; only the blend arithmetic's rounding remains.
            let w = worst(&mut r, &d, level);
            assert!(w <= 1e-6, "{depth:?} L{level}: {w:e}");
        }
    }
}

#[test]
fn dirty_rect_frames_are_bit_exact_and_local() {
    let Some(gpu) = gpu() else { return };
    let e = Extent::new(700, 520);
    let mut d = scene(Depth::U8, e);
    let target = d.state().layer_ids()[5];
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    for level in [0u8, 2] {
        let f = r.render(&d, level).unwrap();
        assert!(f.full);
    }
    // Nothing changed: no dispatch.
    let f = r.render(&d, 2).unwrap();
    assert_eq!(f.blocks, 0);
    assert!(f.damage.is_empty());

    let dab = Rect::new(300, 200, 364, 264);
    let op = paint_op(d.state(), target, PaintTarget::Content, dab, |x, y, p| {
        let dx = x as f32 - 332.0;
        let dy = y as f32 - 232.0;
        let a = (1.0 - (dx * dx + dy * dy).sqrt() / 32.0).clamp(0.0, 1.0);
        p[0] += (1.0 - p[0]) * a;
        p[3] = p[3].max(a);
    })
    .unwrap();
    d.apply(op).unwrap();
    for level in [2u8, 0] {
        let f = r.render(&d, level).unwrap();
        assert!(!f.full, "L{level} {f:?}");
        assert!(f.uploaded_pages <= 4, "{f:?}");
        let dab_l = dab.to_level(level);
        let blocks = (dab_l.width() as u32).div_ceil(16) + 1;
        assert!(f.blocks <= blocks * blocks, "L{level}: {f:?}");
        let mut fresh = ResidentRenderer::new(&gpu).unwrap();
        fresh.render(&d, level).unwrap();
        assert!(
            bits(&r, level) == bits(&fresh, level),
            "L{level} partial ≠ cold"
        );
    }
    // A property change, an adjustment edit, undo (new epoch) and redo.
    let id = d.state().layer_ids()[12];
    set_props(&mut d, id, |p| p.opacity = 0.25);
    let adj = d
        .state()
        .layer_ids()
        .into_iter()
        .find(|id| {
            matches!(
                d.state().find(*id).unwrap().kind,
                LayerKind::Adjustment(Adjustment::Curves { .. })
            )
        })
        .unwrap();
    d.apply(DocOp::SetAdjustment {
        id: adj,
        adjustment: Adjustment::Invert,
    })
    .unwrap();
    for step in 0..4 {
        match step {
            1 => assert!(d.undo()),
            2 => assert!(d.undo()),
            3 => assert!(d.redo()),
            _ => {}
        }
        for level in [2u8, 0] {
            r.render(&d, level).unwrap();
            let mut fresh = ResidentRenderer::new(&gpu).unwrap();
            fresh.render(&d, level).unwrap();
            assert!(
                bits(&r, level) == bits(&fresh, level),
                "step {step} L{level}"
            );
            assert!(worst(&mut fresh, &d, level) <= 2e-3);
        }
    }
}

#[test]
fn rendering_is_deterministic() {
    let Some(gpu) = gpu() else { return };
    let d = scene(Depth::U16, Extent::new(517, 300));
    let mut a = ResidentRenderer::new(&gpu).unwrap();
    let mut b = ResidentRenderer::new(&gpu).unwrap();
    for level in [0u8, 1, 3] {
        a.render(&d, level).unwrap();
        b.render(&d, 0).unwrap();
        b.render(&d, level).unwrap();
        let first = bits(&a, level);
        assert!(first == bits(&b, level), "L{level}");
        // Re-running the same program over the same pages is idempotent.
        for _ in 0..3 {
            a.render(&d, level).unwrap();
            assert!(first == bits(&a, level));
        }
    }
    // A second device agrees bit for bit.
    let gpu2 = GpuCompositor::new().unwrap();
    let mut c = ResidentRenderer::new(&gpu2).unwrap();
    c.render(&d, 1).unwrap();
    assert!(bits(&a, 1) == bits(&c, 1));
}

#[test]
fn copy_on_write_duplicates_share_pages() {
    let Some(gpu) = gpu() else { return };
    let e = Extent::new(600, 520); // 3×3 tiles
    let mut d = doc(e, Depth::U8);
    let a = add(&mut d, None, layer_fn("a", e, Depth::U8, wave(3)));
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    let f = r.render(&d, 2).unwrap();
    assert_eq!(f.uploaded_pages, 9);
    assert_eq!(f.mip_pages, 4 + 1);
    for _ in 0..5 {
        d.apply(DocOp::DuplicateLayer { id: a }).unwrap();
    }
    let f = r.render(&d, 2).unwrap();
    assert_eq!((f.uploaded_pages, f.mip_pages), (0, 0), "{f:?}");
    let dup = *d.state().layer_ids().last().unwrap();
    let op = paint_op(
        d.state(),
        dup,
        PaintTarget::Content,
        Rect::new(10, 10, 20, 20),
        |_, _, p| p[1] = 1.0,
    )
    .unwrap();
    d.apply(op).unwrap();
    let f = r.render(&d, 2).unwrap();
    // One replaced tile, one new mip page per level above it.
    assert_eq!((f.uploaded_pages, f.mip_pages), (1, 2), "{f:?}");
    assert!(!f.full);
    assert!(worst(&mut r, &d, 2) <= 1e-4);
    assert!(worst(&mut r, &d, 0) <= 1e-4);
}

#[test]
fn small_budget_evicts_history_pages_and_stays_correct() {
    let Some(gpu) = gpu() else { return };
    let e = Extent::new(700, 700);
    let mut d = doc(e, Depth::U8);
    let id = add(&mut d, None, layer_fn("a", e, Depth::U8, wave(9)));
    // A budget of about one slab: repainting everything forces eviction of
    // the previous state's pages.
    let mut r = ResidentRenderer::with_budget(&gpu, 32 * 256 * 1024).unwrap();
    for k in 0..4u32 {
        let op = paint_op(
            d.state(),
            id,
            PaintTarget::Content,
            Rect::of_extent(e),
            move |x, _, p| p[0] = ((x + k * 40) % 256) as f32 / 255.0,
        )
        .unwrap();
        d.apply(op).unwrap();
        r.render(&d, 0).unwrap();
        r.render(&d, 1).unwrap();
    }
    assert!(r.stats().evicted_pages > 0, "{:?}", r.stats());
    assert!(d.undo());
    assert!(worst(&mut r, &d, 0) <= 1e-4);
    assert!(worst(&mut r, &d, 1) <= 1e-4);
}

#[test]
fn smart_objects_render_through_gpu_resampled_pages() {
    let Some(gpu) = gpu() else { return };
    let ce = Extent::new(120, 90);
    let mut child = DocState::new(ce, Depth::F32);
    let mut l = layer_fn("c", ce, Depth::F32, wave(7));
    l.id = LayerId(1);
    child.root.push(std::sync::Arc::new(l));
    child.next_id = 2;
    let e = Extent::new(300, 260);
    let mut d = doc(e, Depth::F32);
    add(&mut d, None, layer_fn("bg", e, Depth::F32, wave(2)));
    add(
        &mut d,
        None,
        Layer::new(
            "so",
            LayerKind::SmartObject(SmartObject::new(
                child,
                Affine::scale_translate(1.5, 1.25, 40.0, 30.0),
            )),
        )
        .with_mode(BlendMode::Multiply),
    );
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    for level in [0u8, 1, 2] {
        assert!(worst(&mut r, &d, level) <= 1e-4);
    }
    assert!(r.stats().smart_pages > 0);
}

#[test]
fn present_flattens_into_an_rgba8_storage_texture() {
    let Some(gpu) = gpu() else { return };
    let e = Extent::new(90, 70);
    let d = scene(Depth::U8, e);
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    r.render(&d, 0).unwrap();
    let (device, queue) = gpu.handles();
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size: wgpu::Extent3d {
            width: 128,
            height: 80,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::STORAGE_BINDING | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let src = Rect::new(10, 5, 70, 65);
    r.present(0, &tex, src, (3, 4), Some([1.0, 1.0, 1.0]))
        .unwrap();
    let buf = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 512 * 80,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut enc = device.create_command_encoder(&Default::default());
    enc.copy_texture_to_buffer(
        tex.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buf,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(512),
                rows_per_image: None,
            },
        },
        wgpu::Extent3d {
            width: 128,
            height: 80,
            depth_or_array_layers: 1,
        },
    );
    queue.submit([enc.finish()]);
    buf.slice(..).map_async(wgpu::MapMode::Read, |_| {});
    device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
    let px = buf.slice(..).get_mapped_range().unwrap().to_vec();
    let (_, pm) = r.read_level(0, true).unwrap();
    for y in 0..60u32 {
        for x in 0..60u32 {
            let s = (((5 + y) * 90 + 10 + x) * 4) as usize;
            let o = ((4 + y) * 512 + (3 + x) * 4) as usize;
            for c in 0..3 {
                let want = (pm[s + c] + (1.0 - pm[s + 3])).clamp(0.0, 1.0) * 255.0;
                assert!(
                    (f32::from(px[o + c]) - want).abs() <= 0.51,
                    "({x},{y}) c{c}: {} vs {want}",
                    px[o + c]
                );
            }
            assert_eq!(px[o + 3], 255);
        }
    }
}

#[test]
fn one_shared_device_serves_the_compositor() {
    let shared = match gpu_core::GpuDevice::new() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("skipping: no Metal adapter ({e})");
            return;
        }
    };
    eprintln!("shared device: {shared:?}");
    let gpu = GpuCompositor::from_shared(&shared).unwrap();
    let d = scene(Depth::U8, Extent::new(64, 48));
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    assert!(worst(&mut r, &d, 0) <= 2e-3);
    r.drop_level(0);
    assert!(r.read_level(0, true).is_err());
    // A device with default limits (8 storage buffers) is refused clearly.
    let adapter = pollster::block_on(
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle())
            .request_adapter(&Default::default()),
    )
    .unwrap();
    let (device, queue) = pollster::block_on(adapter.request_device(&Default::default())).unwrap();
    let small = GpuCompositor::from_device(device, queue, "defaults".into()).unwrap();
    assert!(matches!(
        ResidentRenderer::new(&small),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}

#[test]
fn cached_composites_are_flagged_premultiplied_and_levels_go_to_one_pixel() {
    let e = Extent::new(1000, 600);
    let mut d = doc(e, Depth::U8);
    let g = add(&mut d, None, Layer::group("g", GroupMode::Isolated));
    add(&mut d, Some(g), layer_fn("a", e, Depth::U8, wave(1)));
    let c = Compositor::new(64 << 20);
    let coord = TileCoord::new(1, 0, 0);
    assert!(
        c.render_tile_premultiplied(&d, coord)
            .unwrap()
            .premultiplied()
    );
    assert!(!c.render_tile(&d, coord).unwrap().premultiplied());
    let p = c.pyramid(&d);
    assert!(!p.premultiplied());
    assert_eq!(p.level_count(), e.full_level_count());
    assert_eq!(p.level_extent(p.level_count() - 1), Extent::new(1, 1));
    let deepest = TileCoord::new(p.level_count() - 1, 0, 0);
    assert!(p.contains(deepest));
    let t = p.tile(deepest).unwrap();
    assert_eq!(t.layout().extent, Extent::new(1, 1));
}
