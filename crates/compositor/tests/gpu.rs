//! The WGSL compositor against the CPU reference (docs/11 §1.3: per
//! operator ≤ 1e-4, full chain ≤ 2e-3).
mod common;
use common::*;
use compositor::gpu::GpuCompositor;
use compositor::*;
use engine_api::tile::{Extent, TileCoord};

fn wave(seed: u32) -> impl Fn(u32, u32) -> [f32; 4] {
    move |x, y| {
        let f =
            |k: u32| (((x * (3 + k) + y * (5 + 2 * k) + seed * 37 + k * 11) % 97) as f32) / 96.0;
        [f(0), f(1), f(2), 0.25 + 0.75 * f(3)]
    }
}

fn scene() -> Document {
    let e = Extent::new(300, 140);
    let mut d = doc(e, Depth::F32);
    let bg = add(
        &mut d,
        None,
        layer_fn("bg", e, Depth::F32, |x, y| {
            opaque([x as f32 / 300.0, y as f32 / 140.0, 0.4])
        }),
    );
    set_props(&mut d, bg, |p| p.background = true);
    // Every mode, one layer each, partial alpha, varying opacity/fill.
    for (i, m) in BlendMode::ALL.into_iter().enumerate() {
        let id = add(
            &mut d,
            None,
            layer_fn("m", e, Depth::F32, wave(i as u32)).with_mode(m),
        );
        set_props(&mut d, id, |p| {
            p.opacity = 0.4 + 0.02 * i as f32;
            p.fill_opacity = 1.0 - 0.01 * i as f32;
        });
    }
    // Pass-through group with a shallow knockout.
    let pt = add(
        &mut d,
        None,
        Layer::group("pt", GroupMode::PassThrough).with_opacity(0.8),
    );
    add(
        &mut d,
        Some(pt),
        layer_fn("a", e, Depth::F32, wave(40)).with_mode(BlendMode::Overlay),
    );
    let ko = add(&mut d, Some(pt), layer_fn("ko", e, Depth::F32, wave(41)));
    set_props(&mut d, ko, |p| {
        p.knockout = Knockout::Shallow;
        p.fill_opacity = 0.3;
    });
    // Isolated group with a mask, Blend If, deep knockout.
    let iso = add(
        &mut d,
        None,
        Layer::group("iso", GroupMode::Isolated).with_mode(BlendMode::SoftLight),
    );
    let mut m = Mask::reveal_all(e, Depth::F32);
    m.raster
        .edit_region(Rect::new(0, 0, 150, 140), 1, |x, _, p| {
            p[0] = x as f32 / 150.0
        })
        .unwrap();
    d.apply(DocOp::SetMask {
        id: iso,
        mask: Some(m),
    })
    .unwrap();
    add(
        &mut d,
        Some(iso),
        layer_fn("b", e, Depth::F32, wave(50)).with_mode(BlendMode::Hue),
    );
    let bi = add(&mut d, Some(iso), layer_fn("bi", e, Depth::F32, wave(51)));
    set_props(&mut d, bi, |p| {
        p.blend_if.gray.underlying = [0.1, 0.3, 0.7, 0.9];
        p.blend_if.rgb[1].this_layer = [0.0, 0.2, 0.8, 1.0];
    });
    let deep = add(&mut d, Some(iso), layer_fn("deep", e, Depth::F32, wave(52)));
    set_props(&mut d, deep, |p| {
        p.knockout = Knockout::Deep;
        p.fill_opacity = 0.5;
        p.opacity = 0.7;
    });
    // A clip group (Colour Dodge and Dissolve clipped to a Multiply base).
    add(
        &mut d,
        None,
        layer_fn("base", e, Depth::F32, wave(60)).with_mode(BlendMode::Multiply),
    );
    let c1 = add(
        &mut d,
        None,
        layer_fn("c1", e, Depth::F32, wave(61)).with_mode(BlendMode::ColorDodge),
    );
    set_props(&mut d, c1, |p| p.clipped = true);
    let c2 = add(
        &mut d,
        None,
        layer_fn("c2", e, Depth::F32, wave(62))
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
                start: [150.0, 70.0],
                end: [300.0, 70.0],
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
    d
}

#[test]
fn gpu_matches_cpu_reference() {
    let gpu = match GpuCompositor::new() {
        Ok(g) => g,
        Err(e) => {
            eprintln!("skipping: no Metal adapter ({e})");
            return;
        }
    };
    eprintln!("adapter: {}", gpu.adapter);
    let d = scene();
    let cpu = Compositor::new(256 << 20);
    let mut worst = 0.0f32;
    for level in [0u8, 1] {
        let (cols, rows) = d.state().canvas.at_level(level).tile_grid(256);
        for y in 0..rows {
            for x in 0..cols {
                let c = TileCoord::new(level, x, y);
                let a = cpu.render_tile_premultiplied(&d, c).unwrap();
                let b = gpu
                    .render_tile_premultiplied(&Compositor::new(64 << 20), &d, c)
                    .unwrap();
                let (a, b) = (a.samples::<f32>().unwrap(), b.samples::<f32>().unwrap());
                for (p, q) in a.iter().zip(b) {
                    worst = worst.max((p - q).abs());
                }
            }
        }
    }
    // 30+ stacked layers are a chain: docs/11 §1.3 allows 2e-3 there; each
    // mode on its own is gated at 1e-4 in `every_mode_matches_on_the_gpu`.
    eprintln!("chain: max |gpu − cpu| = {worst:e}");
    assert!(worst <= 2e-3, "{worst:e}");
}

#[test]
fn every_mode_matches_on_the_gpu() {
    let Ok(gpu) = GpuCompositor::new() else {
        return;
    };
    let e = Extent::new(256, 140);
    for (i, m) in BlendMode::ALL.into_iter().enumerate() {
        let mut d = doc(e, Depth::F32);
        add(
            &mut d,
            None,
            layer_fn("bg", e, Depth::F32, |x, y| {
                opaque([x as f32 / 300.0, y as f32 / 140.0, 0.4])
            }),
        );
        for j in 0..3 {
            add(
                &mut d,
                None,
                layer_fn("m", e, Depth::F32, wave(j + 30)).with_opacity(0.7),
            );
        }
        let id = add(
            &mut d,
            None,
            layer_fn("m", e, Depth::F32, wave(i as u32)).with_mode(m),
        );
        set_props(&mut d, id, |p| {
            p.opacity = 0.4 + 0.02 * i as f32;
            p.fill_opacity = 1.0 - 0.01 * i as f32;
        });
        let c = TileCoord::new(0, 0, 0);
        let a = Compositor::new(64 << 20)
            .render_tile_premultiplied(&d, c)
            .unwrap();
        let b = gpu
            .render_tile_premultiplied(&Compositor::new(64 << 20), &d, c)
            .unwrap();
        let worst = a
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(b.samples::<f32>().unwrap())
            .map(|(p, q)| (p - q).abs())
            .fold(0.0f32, f32::max);
        assert!(worst <= 1e-4, "{m:?}: {worst:e}");
        eprintln!("{m:?}: {worst:e}");
    }
}

#[test]
fn gpu_declines_adjustment_layers() {
    let Ok(gpu) = GpuCompositor::new() else {
        return;
    };
    let mut d = doc(Extent::new(8, 8), Depth::F32);
    add(
        &mut d,
        None,
        Layer::new("inv", LayerKind::Adjustment(Adjustment::Invert)),
    );
    let r = gpu.render_tile(&Compositor::new(1 << 20), &d, TileCoord::new(0, 0, 0));
    assert!(matches!(
        r,
        Err(engine_api::EngineError::Unsupported { .. })
    ));
}
