//! Resident style-stack semantics against the CPU reference in linear,
//! premultiplied F32 (including when the source document stores U8).
mod common;
use common::*;
use compositor::gpu::GpuCompositor;
use compositor::psd::{ImportedPsd, from_psd, to_psd};
use compositor::render::styles::*;
use compositor::resident::ResidentRenderer;
use compositor::*;
use engine_api::tile::{Extent, TileCoord};
use std::time::Instant;

fn gpu() -> Option<GpuCompositor> {
    Some(GpuCompositor::new().expect("M5-31 requires a real GPU"))
}

fn styles() -> LayerStyles {
    LayerStyles {
        effects: vec![
            StyleEffect::DropShadow(Shadow {
                distance: 7.0,
                size: 4.0,
                spread: 1.0,
                opacity: 0.6,
                ..Default::default()
            }),
            StyleEffect::ColorOverlay(Overlay {
                fill: Fill::Solid {
                    color: [0.9, 0.15, 0.3],
                },
                opacity: 0.45,
                mode: BlendMode::Screen,
                ..Default::default()
            }),
        ],
        ..Default::default()
    }
}

fn shape(e: Extent, depth: Depth, shift: u32) -> Layer {
    layer_fn("soft patterned shape", e, depth, move |x, y| {
        let inside = x >= 9 + shift && x < e.width - 12 && y >= 8 && y < e.height - 9;
        [
            (x % 23) as f32 / 22.0,
            (y % 19) as f32 / 18.0,
            0.35,
            if inside {
                0.35 + 0.65 * ((x + y) % 7) as f32 / 6.0
            } else {
                0.0
            },
        ]
    })
}

fn scene(depth: Depth) -> (Document, LayerId) {
    let e = Extent::new(67, 49); // Odd L0 and L2 extents and partially covered mip edges.
    let mut d = doc(e, depth);
    add(
        &mut d,
        None,
        layer_fn("backdrop", e, depth, |x, y| {
            [0.15 + 0.65 * x as f32 / 66.0, y as f32 / 60.0, 0.6, 0.8]
        }),
    );
    let mut l = shape(e, depth, 0);
    l.props.styles = styles();
    let id = add(&mut d, None, l);
    (d, id)
}

#[track_caller]
fn close(got: &[f32], want: &[f32], context: &str) {
    assert_eq!(got.len(), want.len(), "{context}: sample count");
    let mut worst = (0.0f32, 0);
    for (i, (&g, &w)) in got.iter().zip(want).enumerate() {
        assert!(
            g.is_finite() && w.is_finite(),
            "{context}: nonfinite sample {i}"
        );
        if (g - w).abs() > worst.0 {
            worst = ((g - w).abs(), i);
        }
    }
    assert!(
        worst.0 <= 1e-4,
        "{context}: max linear premult error {} at sample {}: GPU {}, CPU {}",
        worst.0,
        worst.1,
        got[worst.1],
        want[worst.1]
    );
}

/// Fresh CPU cache on each check prevents a stale CPU result from masking a
/// resident invalidation bug. Readback synchronizes GPU work, not just submission.
fn parity(r: &mut ResidentRenderer, d: &Document, level: u8, context: &str) -> Vec<f32> {
    r.render(d, level).unwrap();
    let tiles = r.read_tiles(level).unwrap();
    assert!(!tiles.is_empty(), "{context}: empty readback");
    let cpu = Compositor::new(64 << 20);
    let mut pixels = Vec::new();
    for tile in tiles {
        assert!(tile.premultiplied());
        let expected = cpu.render_tile_premultiplied(d, tile.coord()).unwrap();
        let got = tile.samples::<f32>().unwrap();
        close(
            got,
            expected.samples::<f32>().unwrap(),
            &format!("{context} L{level} {:?}", tile.coord()),
        );
        pixels.extend_from_slice(got);
    }
    pixels
}

fn both(r: &mut ResidentRenderer, d: &Document, context: &str) -> [Vec<f32>; 2] {
    [parity(r, d, 0, context), parity(r, d, 2, context)]
}

fn changed(before: &[Vec<f32>; 2], after: &[Vec<f32>; 2], context: &str) {
    for i in 0..2 {
        assert!(
            before[i]
                .iter()
                .zip(&after[i])
                .any(|(a, b)| (a - b).abs() > 1e-4),
            "{context}: fixture must visibly change at {}",
            ["L0", "L2"][i]
        );
    }
}

#[test]
fn blend_if_filters_styled_stack_at_l0_and_l2() {
    let Some(g) = gpu() else { return };
    for depth in [Depth::F32, Depth::U8, Depth::U16] {
        let (mut d, id) = scene(depth);
        let mut r = ResidentRenderer::new(&g).unwrap();
        let baseline = both(&mut r, &d, "no blend-if");
        set_props(&mut d, id, |p| {
            p.blend_if.gray.underlying = [0.15, 0.35, 0.65, 0.85];
            p.blend_if.rgb[0].this_layer = [0.05, 0.25, 0.7, 0.95];
        });
        changed(&baseline, &both(&mut r, &d, "split blend-if"), "blend-if");
    }
}

#[test]
fn zero_fill_keeps_effects_but_whole_opacity_fades_once() {
    let Some(g) = gpu() else { return };
    let e = Extent::new(32, 24);
    let mut d = doc(e, Depth::F32);
    let mut l = layer_fn("blue", e, Depth::F32, |_, _| [0.0, 0.0, 1.0, 1.0]);
    l.props.styles.effects = vec![StyleEffect::ColorOverlay(Overlay::default())];
    l.props.fill_opacity = 0.0;
    let id = add(&mut d, None, l);
    let mut r = ResidentRenderer::new(&g).unwrap();
    for opacity in [1.0, 0.37, 0.0] {
        set_props(&mut d, id, |p| p.opacity = opacity);
        for level in [0, 2] {
            parity(&mut r, &d, level, "zero fill / whole opacity");
            // Tile samples are planar; read_level is interleaved for this oracle.
            let (_, pixels) = r.read_level(level, true).unwrap();
            for p in pixels.as_chunks::<4>().0 {
                close(
                    p,
                    &[opacity, 0.0, 0.0, opacity],
                    "red effect survives fill, whole stack fades once",
                );
            }
        }
    }
    // Multiple overlapping planes must also fade as a single stack.
    let (mut d, id) = scene(Depth::U8);
    let mut r = ResidentRenderer::new(&g).unwrap();
    let before = both(&mut r, &d, "full stack");
    set_props(&mut d, id, |p| {
        p.fill_opacity = 0.0;
        p.opacity = 0.37;
    });
    changed(
        &before,
        &both(&mut r, &d, "faded stack without fill"),
        "stack opacity",
    );
}

#[test]
fn shallow_and_deep_knockout_through_both_ancestor_kinds() {
    let Some(g) = gpu() else { return };
    let e = Extent::new(67, 49);
    for outer_mode in [GroupMode::Isolated, GroupMode::PassThrough] {
        for inner_mode in [GroupMode::Isolated, GroupMode::PassThrough] {
            for knockout in [Knockout::Shallow, Knockout::Deep] {
                let mut d = doc(e, Depth::F32);
                let bg = add(
                    &mut d,
                    None,
                    layer_fn("background", e, Depth::F32, |_, _| [0.1, 0.2, 0.8, 1.0]),
                );
                set_props(&mut d, bg, |p| p.background = true);
                add(
                    &mut d,
                    None,
                    layer_fn("root paint", e, Depth::F32, |_, _| [0.8, 0.7, 0.1, 0.9]),
                );
                let outer = add(
                    &mut d,
                    None,
                    Layer::group("outer", outer_mode).with_opacity(0.85),
                );
                add(
                    &mut d,
                    Some(outer),
                    layer_fn("outer paint", e, Depth::F32, |_, _| [0.1, 0.8, 0.3, 0.8]),
                );
                let inner = add(&mut d, Some(outer), Layer::group("inner", inner_mode));
                add(
                    &mut d,
                    Some(inner),
                    layer_fn("inner paint", e, Depth::F32, |_, _| [0.7, 0.1, 0.6, 0.85]),
                );
                let mut l = shape(e, Depth::F32, 0);
                l.props.styles = styles();
                l.props.fill_opacity = 0.0;
                l.props.opacity = 0.7;
                let id = add(&mut d, Some(inner), l);
                let mut r = ResidentRenderer::new(&g).unwrap();
                let before = both(&mut r, &d, "no knockout");
                set_props(&mut d, id, |p| p.knockout = knockout);
                let context = format!("{outer_mode:?}/{inner_mode:?}/{knockout:?}");
                changed(&before, &both(&mut r, &d, &context), &context);
            }
        }
    }
}

#[test]
fn clipped_style_stack_uses_base_alpha() {
    let Some(g) = gpu() else { return };
    let e = Extent::new(67, 49);
    let mut d = doc(e, Depth::U8);
    add(
        &mut d,
        None,
        layer_fn("clip base", e, Depth::U8, |x, y| {
            [0.15, 0.3, 0.8, if x < 33 && y > 12 { 0.6 } else { 0.0 }]
        }),
    );
    let mut l = shape(e, Depth::U8, 0);
    l.props.styles = styles();
    let id = add(&mut d, None, l);
    let mut r = ResidentRenderer::new(&g).unwrap();
    let unclipped = both(&mut r, &d, "unclipped");
    set_props(&mut d, id, |p| {
        p.clipped = true;
        p.opacity = 0.65;
    });
    changed(&unclipped, &both(&mut r, &d, "clipped styles"), "clip");
    let (_, pixels) = r.read_level(0, true).unwrap();
    for y in 0..e.height {
        for x in 40..e.width {
            assert_eq!(
                px(&pixels, e.width, x, y)[3],
                0.0,
                "style escaped clipping at {x},{y}"
            );
        }
    }
}

#[test]
fn nested_styled_isolated_groups_use_composite_alpha() {
    let Some(g) = gpu() else { return };
    let e = Extent::new(67, 49);
    let mut d = doc(e, Depth::F32);
    let outer = add(
        &mut d,
        None,
        Layer::group("styled outer", GroupMode::Isolated),
    );
    let inner = add(
        &mut d,
        Some(outer),
        Layer::group("styled inner", GroupMode::Isolated),
    );
    let mut l = shape(e, Depth::F32, 0);
    l.props.styles = styles();
    add(&mut d, Some(inner), l);
    add(
        &mut d,
        Some(outer),
        shape(e, Depth::F32, 16).with_opacity(0.4),
    );
    let mut r = ResidentRenderer::new(&g).unwrap();
    let before = both(&mut r, &d, "unstyled ancestors");
    for id in [inner, outer] {
        set_props(&mut d, id, |p| {
            p.styles = styles();
            p.opacity = 0.73;
            p.fill_opacity = 0.6;
        });
    }
    changed(
        &before,
        &both(&mut r, &d, "nested styled isolated groups"),
        "group styles",
    );
}

#[test]
fn global_light_edits_invalidate_warm_styles_and_undo_restores() {
    let Some(g) = gpu() else { return };
    let (mut d, _) = scene(Depth::U8);
    let mut r = ResidentRenderer::new(&g).unwrap();
    let before = both(&mut r, &d, "original light");
    d.apply(DocOp::SetGlobalLight(GlobalLight {
        angle: 15.0,
        elevation: 65.0,
    }))
    .unwrap();
    let after = both(&mut r, &d, "edited global light");
    changed(&before, &after, "global light");
    assert!(d.undo());
    let undone = both(&mut r, &d, "undo light");
    assert_eq!(before, undone);
    assert!(d.redo());
    assert_eq!(after, both(&mut r, &d, "redo light"));
}

#[test]
fn source_and_mask_paint_invalidate_style_geometry_and_cached_levels() {
    let Some(g) = gpu() else { return };
    let (mut d, id) = scene(Depth::U8);
    d.apply(DocOp::SetMask {
        id,
        mask: Some(Mask::reveal_all(Extent::new(67, 49), Depth::U8)),
    })
    .unwrap();
    let mut r = ResidentRenderer::new(&g).unwrap();
    let mut before = both(&mut r, &d, "original source/mask");
    for target in [PaintTarget::Content, PaintTarget::Mask] {
        let op = paint_op(
            d.state(),
            id,
            target,
            Rect::new(20, 16, 44, 32),
            |_, _, p| {
                if target == PaintTarget::Content {
                    *p = [0.95, 0.05, 0.7, 0.1];
                } else {
                    p[0] = 0.0;
                }
            },
        )
        .unwrap();
        d.apply(op).unwrap();
        let after = both(&mut r, &d, "source/mask paint");
        changed(&before, &after, "source/mask edit");
        let mut cold = ResidentRenderer::new(&g).unwrap();
        assert_eq!(after, both(&mut cold, &d, "cold after edit"));
        for level in [0, 2] {
            let idle = r.render(&d, level).unwrap();
            assert_eq!(idle.blocks, 0, "unchanged style frame must be cached");
            assert_eq!(idle.uploaded_pages, 0);
        }
        assert!(d.undo());
        assert_eq!(before, both(&mut r, &d, "undo paint"));
        assert!(d.redo());
        assert_eq!(after, both(&mut r, &d, "redo paint"));
        before = after;
    }
}

// Native ActionDescriptor bytes, independent of the compositor style encoder.
fn descriptor_id(key: &[u8]) -> Vec<u8> {
    [
        (if key.len() == 4 { 0 } else { key.len() as u32 })
            .to_be_bytes()
            .as_slice(),
        key,
    ]
    .concat()
}
fn descriptor(class: &[u8], entries: Vec<(&[u8], &[u8; 4], Vec<u8>)>) -> Vec<u8> {
    let mut out = [
        0u32.to_be_bytes().to_vec(),
        descriptor_id(class),
        (entries.len() as u32).to_be_bytes().to_vec(),
    ]
    .concat();
    for (key, ty, data) in entries {
        out.extend(descriptor_id(key));
        out.extend(ty);
        out.extend(data);
    }
    out
}
fn unit(unit: &[u8; 4], value: f64) -> Vec<u8> {
    [unit.as_slice(), &value.to_be_bytes()].concat()
}

#[test]
fn independent_native_lfx2_u8_psd_renders_on_resident() {
    let Some(g) = gpu() else { return };
    let e = Extent::new(67, 49);
    let mut seed = doc(e, Depth::U8);
    add(&mut seed, None, shape(e, Depth::U8, 0));
    // Only the unstyled raster carrier uses the adapter. Effect bytes below
    // have not passed through to_psd's style serialization.
    let imported = ImportedPsd::from_state((**seed.state()).clone()).unwrap();
    let mut native = to_psd(&imported).unwrap();
    assert!(native.layer_section.layers[0].info(b"lfx2").is_none());
    let color = descriptor(
        b"RGBC",
        vec![
            (b"Rd  ", b"doub", 255f64.to_be_bytes().to_vec()),
            (b"Grn ", b"doub", 127.5f64.to_be_bytes().to_vec()),
            (b"Bl  ", b"doub", 0f64.to_be_bytes().to_vec()),
        ],
    );
    let shadow = descriptor(
        b"DrSh",
        vec![
            (b"enab", b"bool", vec![1]),
            (
                b"Md  ",
                b"enum",
                [descriptor_id(b"BlnM"), descriptor_id(b"Mltp")].concat(),
            ),
            (b"Clr ", b"Objc", color),
            (b"Opct", b"UntF", unit(b"#Prc", 60.0)),
            (b"blur", b"UntF", unit(b"#Pxl", 4.0)),
            (b"Dstn", b"UntF", unit(b"#Pxl", 7.0)),
            (b"Ckmt", b"UntF", unit(b"#Prc", 25.0)),
            (b"uglg", b"bool", vec![0]),
            (b"lagl", b"UntF", unit(b"#Ang", 35.0)),
        ],
    );
    native.layer_section.layers[0]
        .additional
        .push(::psd::AdditionalInfo {
            signature: *b"8BIM",
            key: *b"lfx2",
            data: [
                0u32.to_be_bytes().to_vec(),
                16u32.to_be_bytes().to_vec(),
                descriptor(
                    b"Lefx",
                    vec![
                        (b"Scl ", b"UntF", unit(b"#Prc", 125.0)),
                        (b"DrSh", b"Objc", shadow),
                    ],
                ),
            ]
            .concat(),
        });
    let native = ::psd::PsdDocument::read(&native.write().unwrap()).unwrap();
    let imported = from_psd(&native).unwrap();
    assert_eq!(imported.state.depth, Depth::U8);
    assert_eq!(imported.root[0].props.styles.effects.len(), 1);
    assert_eq!(imported.root[0].props.styles.scale, 1.25);
    let StyleEffect::DropShadow(s) = &imported.root[0].props.styles.effects[0] else {
        panic!("native shadow missing")
    };
    assert_eq!(
        (s.size, s.distance, s.spread, s.opacity),
        (4.0, 7.0, 1.0, 0.6)
    );
    assert_eq!(s.color, [1.0, 0.5, 0.0, 1.0]);
    let mut d = Document::new(imported.state.clone());
    let mut r = ResidentRenderer::new(&g).unwrap();
    let styled = both(&mut r, &d, "native lfx2 U8 imported content");
    let id = d.state().root[0].id;
    set_props(&mut d, id, |p| p.styles.effects.clear());
    changed(
        &styled,
        &both(&mut r, &d, "native content without shadow"),
        "imported effect renders",
    );
}

/// Explicit opt-in: cargo test -p compositor --release --test resident_styles_semantics
/// benchmark_4k_twenty_styled_layers_l2_tile -- --ignored --nocapture
#[test]
#[ignore = "4K/20-layer synchronized CPU versus resident timing; run explicitly"]
fn benchmark_4k_twenty_styled_layers_l2_tile() {
    let Some(g) = gpu() else { return };
    let e = Extent::new(3840, 2160);
    let mut d = doc(e, Depth::F32);
    // Sparse source tiles bound fixture memory; every layer crosses the measured
    // tile, with a distinct small-radius shadow. The canvas remains true 4K.
    for i in 0..20 {
        let mut l = Layer::pixel(format!("styled {i}"), e, Depth::F32);
        l.raster_mut()
            .unwrap()
            .edit_region(
                Rect::new(100 + i * 8, 100 + i * 5, 500 + i * 8, 500 + i * 5),
                1,
                |_, _, p| *p = [0.15 + i as f32 * 0.03, 0.4, 0.7, 0.6],
            )
            .unwrap();
        l.props.styles.effects = vec![StyleEffect::DropShadow(Shadow {
            size: 2.0,
            distance: 3.0,
            opacity: 0.4,
            ..Default::default()
        })];
        add(&mut d, None, l);
    }
    assert_eq!(d.state().root.len(), 20);
    let coord = TileCoord::new(2, 0, 0);
    let viewport = Rect::new(0, 0, 256, 256); // Exactly one tile in L2 coordinates.
    let cpu = Compositor::new(512 << 20);
    let start = Instant::now();
    let want = cpu.render_tile(&d, coord).unwrap();
    let cpu_cold = start.elapsed();
    let start = Instant::now();
    let _ = cpu.render_tile(&d, coord).unwrap();
    let cpu_warm = start.elapsed();
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.wait().unwrap();
    let start = Instant::now();
    let cold = r.render_viewport(&d, 2, viewport, 0).unwrap();
    r.wait().unwrap();
    let gpu_cold = start.elapsed();
    let start = Instant::now();
    let warm = r.render_viewport(&d, 2, viewport, 0).unwrap();
    r.wait().unwrap();
    let gpu_warm = start.elapsed();
    assert_eq!(warm.blocks, 0);
    assert_eq!(
        cold.blocks,
        16 * 16,
        "timed viewport is exactly one L2 tile"
    );
    // Public read_tiles requires a complete level. Complete GPU L2 *outside*
    // the timings only for readback; CPU still renders just the single tile.
    r.render(&d, 2).unwrap();
    let tiles = r.read_tiles(2).unwrap();
    let tile = tiles.iter().find(|tile| tile.coord() == coord).unwrap();
    let mut expected = want.samples::<f32>().unwrap().to_vec();
    let n = expected.len() / 4;
    for i in 0..n {
        for c in 0..3 {
            expected[c * n + i] *= expected[3 * n + i];
        }
    }
    close(
        tile.samples::<f32>().unwrap(),
        &expected,
        "4K L2 tile benchmark parity",
    );
    eprintln!(
        "3840x2160, 20 styled layers, L2 256x256 viewport: CPU tile cold={cpu_cold:?} warm={cpu_warm:?}; synchronized resident cold={gpu_cold:?} warm={gpu_warm:?}; cold/warm blocks={}/{}",
        cold.blocks, warm.blocks
    );
}
