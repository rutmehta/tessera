//! Bench: a 100-layer, 20 MP synthetic document composited at level 2.
//!
//! `cargo test -p compositor --release --test bench -- --ignored --nocapture`
//! (about 1.5 GB resident). Set `TESSERA_BENCH_ASSERT=1` to fail when the
//! composite-only time exceeds 100 ms.
use std::time::Instant;

use compositor::*;
use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord};

#[test]
#[ignore = "20 MP / 100 layers, hardware timing"]
fn m5_08_structure_and_viewport() {
    use compositor::{gpu::GpuCompositor, resident::ResidentRenderer};
    let g = GpuCompositor::new().expect("Metal required");
    println!("M5-08 adapter: {}", g.adapter);
    let (d, _) = build(Extent::new(5472, 3648));
    let cpu = Compositor::new(4 << 30);
    let mut failures = Vec::new();
    for specialized in [false, true] {
        let mut r = ResidentRenderer::new(&g).unwrap();
        r.set_specialization(specialized);
        let cold = Instant::now();
        let f = r.render(&d, 0).unwrap();
        r.wait().unwrap();
        let first = ms(cold);
        println!("cold full L0: {} pages uploaded", f.uploaded_pages);
        r.wait_for_specializations();
        println!(
            "requested_specialization={specialized}: first L0 {first:.2} ms (uploads; interpreter while the kernel compiles), kernel ready after {:.2} ms, compiled_pipelines={}",
            ms(cold),
            r.specialized_pipeline_count()
        );
        for viewport in [false, true] {
            // Start the viewport case with a fresh output allocation, not a
            // correct full-frame buffer that could hide missed viewport writes.
            if viewport {
                r = ResidentRenderer::new(&g).unwrap();
                r.set_specialization(specialized);
                let cold = Instant::now();
                let f = r
                    .render_viewport(&d, 0, Rect::new(512, 512, 4352, 2672), 0)
                    .unwrap();
                r.wait().unwrap();
                println!(
                    "cold 3840x2160 L0 viewport: {:.2} ms, {} pages uploaded, {} blocks",
                    ms(cold),
                    f.uploaded_pages,
                    f.blocks
                );
                r.wait_for_specializations();
            }
            let mut runs = Vec::new();
            let mut blocks = 0;
            for _ in 0..9 {
                r.invalidate();
                let t = Instant::now();
                let f = if viewport {
                    r.render_viewport(&d, 0, Rect::new(512, 512, 4352, 2672), 0)
                } else {
                    r.render(&d, 0)
                }
                .unwrap();
                r.wait().unwrap();
                blocks = f.blocks;
                runs.push(ms(t));
            }
            let (lo, med, hi) = median(runs);
            println!(
                "specialized={specialized} viewport_3840x2160={viewport}: min={lo:.3} median={med:.3} max={hi:.3} ms, blocks={blocks}"
            );
            // Complete only invalid blocks before explicit readback. The
            // already-valid viewport must survive untouched. Keep this outside
            // the timing loop and always gate correctness, not just when the
            // optional hardware performance assertion is enabled.
            if viewport {
                assert!(r.read_level(0, true).is_err());
                let completed = r.render(&d, 0).unwrap().blocks;
                let e = d.state().canvas;
                assert_eq!(
                    completed + blocks,
                    e.width.div_ceil(16) * e.height.div_ceil(16)
                );
            }
            let mut worst = 0.0f32;
            let mut worst_at = None;
            for got in r.read_tiles(0).unwrap() {
                let want = cpu.render_tile_premultiplied(&d, got.coord()).unwrap();
                for (i, (p, q)) in want
                    .samples::<f32>()
                    .unwrap()
                    .iter()
                    .zip(got.samples::<f32>().unwrap())
                    .enumerate()
                {
                    assert!(p.is_finite() && q.is_finite());
                    let error = (p - q).abs();
                    if error > worst {
                        worst = error;
                        worst_at = Some((got.coord(), i, *p, *q));
                    }
                }
            }
            println!(
                "L0 CPU gate specialized={specialized} viewport={viewport}: {worst:e}, worst (tile, planar sample, CPU, GPU)={worst_at:?}"
            );
            if worst > 2e-3 {
                failures.push(format!(
                    "L0 CPU gate specialized={specialized} viewport={viewport}: {worst:e}"
                ));
            }
            if specialized
                && std::env::var_os("TESSERA_BENCH_ASSERT").is_some()
                && med >= if viewport { 8.0 } else { 100.0 }
            {
                failures.push(format!("performance viewport={viewport}: {med} ms"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Minimal L0 regression from tile (5,3), sample 34646: Pin Light differed
/// by one ulp, then Hard Mix amplified it to 0.2983. Always runs, unlike benches.
#[test]
fn m5_08_hard_mix_boundary() {
    use compositor::{gpu::GpuCompositor, resident::ResidentRenderer};
    let g = GpuCompositor::new().expect("Metal required");
    let e = Extent::new(1, 1);
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.set_specialization(false);
    let cpu = Compositor::new(1 << 20);
    for (mode, opacity, rgba) in [
        (BlendMode::LinearLight, 0.5, [211u8, 175, 208, 170]),
        (BlendMode::PinLight, 0.6, [211, 239, 144, 195]),
        (BlendMode::HardMix, 0.7, [44, 140, 119, 170]),
    ] {
        let mut layer = Layer::pixel("boundary", e, Depth::U8);
        layer.props.blend_mode = mode;
        layer.props.opacity = opacity;
        let raster = layer.raster_mut().unwrap();
        let tile = Tile::from_samples(TileCoord::new(0, 0, 0), raster.layout(0, 0), rgba.to_vec())
            .unwrap();
        raster.set_slot(0, 0, Some(tile), 1).unwrap();
        d.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer,
        })
        .unwrap();
        r.render(&d, 0).unwrap();
        let (_, q) = r.read_level(0, true).unwrap();
        let p = cpu
            .render_tile_premultiplied(&d, TileCoord::new(0, 0, 0))
            .unwrap();
        for (a, b) in p.samples::<f32>().unwrap().iter().zip(q) {
            assert!(
                b.is_finite() && (a - b).abs() <= 2e-3,
                "{mode:?}: CPU={a}, GPU={b}"
            );
        }
    }
    let want = cpu
        .render_tile_premultiplied(&d, TileCoord::new(0, 0, 0))
        .unwrap();
    // Check a cold renderer with default specialization/fallback policy too.
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.render(&d, 0).unwrap();
    let (_, got) = r.read_level(0, true).unwrap();
    for (p, q) in want.samples::<f32>().unwrap().iter().zip(got) {
        assert!(q.is_finite() && (p - q).abs() <= 2e-3);
    }
}

/// Trace the remaining L0 failure through document-order layer prefixes.
/// This deliberately preserves layer IDs and global coordinates (Dissolve).
/// TESSERA_TRACE_PIXEL=x,y selects a pixel; default is the residual Divide
/// boundary at (1026,2049). The original Hard Mix case is (1366,903).
#[test]
#[ignore = "diagnostic: original 20 MP L0 failure, layer-prefix trace"]
fn m5_08_l0_prefix_trace() {
    use compositor::{gpu::GpuCompositor, resident::ResidentRenderer};
    let g = GpuCompositor::new().expect("Metal required");
    let (mut d, _) = build(Extent::new(5472, 3648));
    let ids: Vec<_> = d
        .state()
        .layer_ids()
        .into_iter()
        .filter(|id| d.state().find(*id).unwrap().raster().is_some())
        .collect();
    for id in &ids {
        let mut props = d.state().find(*id).unwrap().props.clone();
        props.visible = false;
        d.apply(DocOp::SetProps { id: *id, props }).unwrap();
    }
    let mut r = ResidentRenderer::new(&g).unwrap();
    r.set_specialization(false);
    let cpu = Compositor::new(1 << 28);
    let pixel = std::env::var("TESSERA_TRACE_PIXEL").unwrap_or_else(|_| "1026,2049".into());
    let (x, y) = pixel.split_once(',').expect("TESSERA_TRACE_PIXEL=x,y");
    let (x, y): (usize, usize) = (x.parse().unwrap(), y.parse().unwrap());
    assert!(x < 5472 && y < 3648);
    let coord = TileCoord::new(0, (x / 256) as u32, (y / 256) as u32);
    let width = (5472 - x / 256 * 256).min(256);
    let height = (3648 - y / 256 * 256).min(256);
    let sample = (y % 256) * width + x % 256;
    println!("trace pixel ({x},{y}), tile={coord:?}, sample={sample}");
    let mut worst = 0.0f32;
    for (prefix, id) in ids.iter().enumerate() {
        let mut props = d.state().find(*id).unwrap().props.clone();
        props.visible = true;
        let mode = props.blend_mode;
        d.apply(DocOp::SetProps { id: *id, props }).unwrap();
        r.render(&d, 0).unwrap();
        let (_, got) = r.read_level(0, true).unwrap();
        let want = cpu.render_tile_premultiplied(&d, coord).unwrap();
        let p: Vec<_> = (0..4)
            .map(|c| want.samples::<f32>().unwrap()[c * width * height + sample])
            .collect();
        let q = &got[(y * 5472 + x) * 4..(y * 5472 + x) * 4 + 4];
        let error = p
            .iter()
            .zip(q)
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        worst = worst.max(error);
        println!(
            "prefix={} id={id:?} mode={mode:?} CPU={p:?} GPU={q:?} error={error:e}",
            prefix + 1
        );
    }
    assert!(worst <= 2e-3, "prefix CPU gate: {worst:e}");
}

fn content(seed: u32, tx: u32, ty: u32, raster: &Raster) -> Tile {
    let l = raster.layout(tx, ty);
    let (w, h) = (l.extent.width, l.extent.height);
    let n = (w * h) as usize;
    let mut v = vec![0u8; 4 * n];
    for y in 0..h {
        for x in 0..w {
            let (gx, gy) = (tx * TILE_SIZE + x, ty * TILE_SIZE + y);
            let i = (y * w + x) as usize;
            let a = gx.wrapping_mul(3 + seed) ^ gy.wrapping_mul(7 + 2 * seed);
            v[i] = (a >> 2) as u8;
            v[n + i] = ((gx + gy * 2 + seed * 40) >> 3) as u8;
            v[2 * n + i] = ((gx / 7 + seed * 23) ^ (gy / 5)) as u8;
            // Semi-transparent everywhere (no occlusion shortcuts apply).
            v[3 * n + i] = 70 + ((gx / 64 + gy / 48 + seed) % 7) as u8 * 25;
        }
    }
    Tile::from_samples(TileCoord::new(0, tx, ty), l, v).unwrap()
}

fn build(e: Extent) -> (Document, Vec<LayerId>) {
    let mut d = Document::new(DocState::new(e, Depth::U8));
    let mut ids = Vec::new();
    // Background, opaque.
    let mut bg = Layer::pixel("background", e, Depth::U8);
    bg.props.background = true;
    let r = bg.raster_mut().unwrap();
    let (cols, rows) = r.grid();
    for ty in 0..rows {
        for tx in 0..cols {
            let mut t = content(99, tx, ty, r);
            let n = t.layout().plane_len();
            t.samples_mut::<u8>().unwrap()[3 * n..].fill(255);
            r.set_slot(tx, ty, Some(t), 1).unwrap();
        }
    }
    ids.push(
        d.apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: bg,
        })
        .unwrap()
        .created[0],
    );
    // Nine distinct base layers.
    for s in 0..9u32 {
        let mut l = Layer::pixel(format!("base {s}"), e, Depth::U8);
        let r = l.raster_mut().unwrap();
        for ty in 0..rows {
            for tx in 0..cols {
                let t = content(s, tx, ty, r);
                r.set_slot(tx, ty, Some(t), 1).unwrap();
            }
        }
        ids.push(
            d.apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: l,
            })
            .unwrap()
            .created[0],
        );
    }
    // Ninety more: copy-on-write duplicates, each with one repainted tile.
    for j in 10..100u32 {
        let src = ids[1 + (j as usize % 9)];
        let id = d.apply(DocOp::DuplicateLayer { id: src }).unwrap().created[0];
        let raster = d.state().find(id).unwrap().raster().unwrap().clone();
        let (tx, ty) = (j % cols, (j / cols) % rows);
        d.apply(DocOp::PaintTiles {
            id,
            target: PaintTarget::Content,
            tiles: vec![TileDelta {
                tx,
                ty,
                tile: Some(content(j, tx, ty, &raster)),
            }],
            dirty: Rect::of_extent(e),
        })
        .unwrap();
        ids.push(id);
    }
    // Varied modes/opacities; two groups (pass-through and isolated).
    let st = d.state().clone();
    for (k, id) in ids.iter().enumerate().skip(1) {
        let mut p = st.find(*id).unwrap().props.clone();
        p.blend_mode = BlendMode::ALL[k % 27];
        p.opacity = 0.5 + (k % 5) as f32 * 0.1;
        if k % 17 == 0 {
            p.blend_if.gray.underlying = [0.05, 0.2, 0.8, 0.95];
        }
        d.apply(DocOp::SetProps { id: *id, props: p }).unwrap();
    }
    for (name, mode, range) in [
        ("pt", GroupMode::PassThrough, 40..50),
        ("iso", GroupMode::Isolated, 70..80),
    ] {
        let g = d
            .apply(DocOp::AddLayer {
                parent: None,
                index: usize::MAX,
                layer: Layer::group(name, mode),
            })
            .unwrap()
            .created[0];
        for id in &ids[range] {
            d.apply(DocOp::MoveLayer {
                id: *id,
                parent: Some(g),
                index: usize::MAX,
            })
            .unwrap();
        }
    }
    (d, ids)
}

fn ms(t: Instant) -> f64 {
    t.elapsed().as_secs_f64() * 1e3
}

#[test]
#[ignore = "bench: run with --ignored --nocapture"]
fn composite_100_layers_20mp_at_level_2() {
    let e = Extent::new(5472, 3648); // 19.96 MP
    let t = Instant::now();
    let (mut d, ids) = build(e);
    let layers = d.state().layer_ids().len();
    println!(
        "built {layers} nodes (100 pixel layers + 2 groups), {} MP, in {:.0} ms",
        e.area() as f64 / 1e6,
        ms(t)
    );
    println!(
        "history bytes (COW-shared): {:.0} MB",
        d.history_bytes() as f64 / 1e6
    );
    let c = Compositor::new(4 << 30);
    let cancel = Default::default();
    let lvl = e.at_level(2);
    println!(
        "level 2: {}×{} ({} tiles), rayon threads {}",
        lvl.width,
        lvl.height,
        {
            let (a, b) = lvl.tile_grid(256);
            a * b
        },
        rayon::current_num_threads()
    );

    let t = Instant::now();
    c.render_level(&d, 2, &cancel).unwrap();
    println!(
        "cold (100 layers' L1+L2 mips from 20 MP + composite): {:.0} ms  {:?}",
        ms(t),
        c.stats()
    );

    let mut runs = Vec::new();
    for _ in 0..7 {
        c.clear_composites();
        c.reset_stats();
        let t = Instant::now();
        c.render_level(&d, 2, &cancel).unwrap();
        runs.push(ms(t));
    }
    let s = c.stats();
    runs.sort_by(f64::total_cmp);
    println!(
        "composite only (mips cached, composites cleared): min {:.1} ms, median {:.1} ms  [blends {}, mips {}, groups {}]",
        runs[0],
        runs[runs.len() / 2],
        s.blends,
        s.mip_tiles,
        s.group_tiles
    );

    c.reset_stats();
    let t = Instant::now();
    c.render_level(&d, 2, &cancel).unwrap();
    println!(
        "warm (all cached): {:.2} ms  hits {}",
        ms(t),
        c.stats().cache_hits
    );

    // A brush dab on a layer inside the isolated group.
    let target = ids[75];
    let dab = Rect::new(2000, 1500, 2064, 1564);
    let op = paint_op(d.state(), target, PaintTarget::Content, dab, |x, y, p| {
        let dx = x as f32 - 2032.0;
        let dy = y as f32 - 1532.0;
        let a = (1.0 - (dx * dx + dy * dy).sqrt() / 32.0).clamp(0.0, 1.0);
        p[0] += (1.0 - p[0]) * a;
        p[3] = p[3].max(a);
    })
    .unwrap();
    d.apply(op).unwrap();
    c.reset_stats();
    let t = Instant::now();
    c.render_level(&d, 2, &cancel).unwrap();
    let s = c.stats();
    println!(
        "brush dab 64² → level 2 update: {:.2} ms  [partial {}, full {}, mips {}, blends {}]",
        ms(t),
        s.root_partial,
        s.root_full,
        s.mip_tiles,
        s.blends
    );
    c.reset_stats();
    let t = Instant::now();
    for ty in 1500 / 256..=1563 / 256 {
        for tx in 2000 / 256..=2063 / 256 {
            c.render_tile(&d, TileCoord::new(0, tx, ty)).unwrap();
        }
    }
    println!(
        "level-0 tiles under the dab (cold, first view): {:.1} ms  {:?}",
        ms(t),
        c.stats()
    );
    let op = paint_op(
        d.state(),
        target,
        PaintTarget::Content,
        dab.inflate(-16),
        |_, _, p| p[1] = 1.0,
    )
    .unwrap();
    d.apply(op).unwrap();
    c.reset_stats();
    let t = Instant::now();
    for ty in 1500 / 256..=1563 / 256 {
        for tx in 2000 / 256..=2063 / 256 {
            c.render_tile(&d, TileCoord::new(0, tx, ty)).unwrap();
        }
    }
    let s = c.stats();
    println!(
        "second dab, level-0 dirty-rect update: {:.2} ms  [partial {}, full {}, blends {}]",
        ms(t),
        s.root_partial,
        s.root_full,
        s.blends
    );

    if std::env::var_os("TESSERA_BENCH_ASSERT").is_some() {
        assert!(
            runs[runs.len() / 2] < 100.0,
            "median {:.1} ms",
            runs[runs.len() / 2]
        );
    }
}

fn median(mut v: Vec<f64>) -> (f64, f64, f64) {
    v.sort_by(f64::total_cmp);
    (v[0], v[v.len() / 2], v[v.len() - 1])
}

/// The same document on the GPU-resident path (COMPOSITOR.md §10.2).
///
/// `cargo test -p compositor --release --test bench -- --ignored --nocapture resident`
/// Set `TESSERA_BENCH_ASSERT=1` to fail when a target is missed.
#[test]
#[ignore = "bench: run with --ignored --nocapture"]
fn resident_100_layers_20mp() {
    use compositor::gpu::GpuCompositor;
    use compositor::resident::ResidentRenderer;
    let e = Extent::new(5472, 3648);
    let (mut d, ids) = build(e);
    let t = Instant::now();
    let gpu = match GpuCompositor::new() {
        Ok(g) => g,
        Err(err) => {
            println!("skipping: no Metal adapter ({err})");
            return;
        }
    };
    println!("device + pipelines: {:.0} ms ({})", ms(t), gpu.adapter);

    // Cold open: nothing resident; every tile uploaded once, L1/L2 mips on
    // the GPU, first level-2 frame complete.
    let t = Instant::now();
    let mut r = ResidentRenderer::new(&gpu).unwrap();
    let f = r.render(&d, 2).unwrap();
    r.wait().unwrap();
    let open = ms(t);
    println!(
        "cold open → first L2 frame: {open:.0} ms  [{} pages / {:.0} MB uploaded, {} mip pages, {} blocks]",
        f.uploaded_pages,
        f.uploaded_bytes as f64 / 1e6,
        f.mip_pages,
        f.blocks
    );

    let mut runs = Vec::new();
    for _ in 0..9 {
        r.invalidate();
        let t = Instant::now();
        r.render(&d, 2).unwrap();
        r.wait().unwrap();
        runs.push(ms(t));
    }
    let (lo, med, _) = median(runs);
    println!("L2 full recomposite (100 layers, 1368×912): min {lo:.2} ms, median {med:.2} ms");

    // Brush dabs on a layer inside the isolated group, walking across a
    // tile boundary.
    let target = ids[75];
    let mut dab_runs = Vec::new();
    let mut apply_runs = Vec::new();
    let mut last = None;
    for k in 0..16i64 {
        let (cx, cy) = (2000 + 24 * k, 1500 + 9 * k);
        let dab = Rect::new(cx - 32, cy - 32, cx + 32, cy + 32);
        let op = paint_op(d.state(), target, PaintTarget::Content, dab, |x, y, p| {
            let dx = x as f32 - cx as f32;
            let dy = y as f32 - cy as f32;
            let a = (1.0 - (dx * dx + dy * dy).sqrt() / 32.0).clamp(0.0, 1.0);
            p[0] += (1.0 - p[0]) * a;
            p[3] = p[3].max(a);
        })
        .unwrap();
        let t = Instant::now();
        d.apply(op).unwrap();
        apply_runs.push(ms(t));
        let t = Instant::now();
        let f = r.render(&d, 2).unwrap();
        r.wait().unwrap();
        dab_runs.push(ms(t));
        last = Some(f);
    }
    let (lo, med, hi) = median(dab_runs.clone());
    let f = last.unwrap();
    println!(
        "64² dab → L2 recomposite: min {lo:.2} ms, median {med:.2} ms, max {hi:.2} ms  [last: {} blocks, {} uploads, {} mips; Document::apply median {:.2} ms]",
        f.blocks,
        f.uploaded_pages,
        f.mip_pages,
        median(apply_runs).1
    );
    let dab_med = med;

    // Full level 0 (20 MP × 100 layers).
    let t = Instant::now();
    let f = r.render(&d, 0).unwrap();
    r.wait().unwrap();
    println!(
        "first L0 frame: {:.1} ms  [{} blocks, {} uploads]",
        ms(t),
        f.blocks,
        f.uploaded_pages
    );
    let mut runs = Vec::new();
    for _ in 0..5 {
        r.invalidate();
        let t = Instant::now();
        r.render(&d, 0).unwrap();
        r.wait().unwrap();
        runs.push(ms(t));
    }
    let (lo, l0_med, _) = median(runs);
    println!("L0 full composite (20 MP × 100 layers): min {lo:.1} ms, median {l0_med:.1} ms");

    let dab = Rect::new(2600, 1700, 2664, 1764);
    let op = paint_op(d.state(), target, PaintTarget::Content, dab, |_, _, p| {
        p[1] = 1.0;
        p[3] = 1.0;
    })
    .unwrap();
    d.apply(op).unwrap();
    let t = Instant::now();
    let f = r.render(&d, 0).unwrap();
    r.wait().unwrap();
    println!(
        "64² dab → L0 dirty-rect update: {:.2} ms  [{} blocks]",
        ms(t),
        f.blocks
    );

    // Opacity drag on one layer: every pixel under it recomposites.
    let mut runs = Vec::new();
    for k in 0..5 {
        let mut p = d.state().find(ids[30]).unwrap().props.clone();
        p.opacity = 0.3 + 0.1 * k as f32;
        d.apply(DocOp::SetProps {
            id: ids[30],
            props: p,
        })
        .unwrap();
        let t = Instant::now();
        r.render(&d, 2).unwrap();
        r.wait().unwrap();
        runs.push(ms(t));
    }
    println!("opacity change → L2 frame: median {:.2} ms", median(runs).1);

    // Gate at scale: the resident L2 against the CPU reference.
    r.render(&d, 2).unwrap();
    let got = r.read_tiles(2).unwrap();
    let cpu = Compositor::new(4 << 30);
    let mut worst = 0.0f32;
    for g in &got {
        let want = cpu.render_tile_premultiplied(&d, g.coord()).unwrap();
        for (p, q) in want
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(g.samples::<f32>().unwrap())
        {
            worst = worst.max((p - q).abs());
        }
    }
    println!("L2 max |resident − CPU| over the bench document: {worst:e}");
    r.set_specialization(false);
    r.render(&d, 2).unwrap();
    let general = r.read_tiles(2).unwrap();
    let mut general_worst = 0.0f32;
    let mut drift = 0.0f32;
    for (g, fast) in general.iter().zip(&got) {
        let want = cpu.render_tile_premultiplied(&d, g.coord()).unwrap();
        for ((p, q), f) in want
            .samples::<f32>()
            .unwrap()
            .iter()
            .zip(g.samples::<f32>().unwrap())
            .zip(fast.samples::<f32>().unwrap())
        {
            general_worst = general_worst.max((p - q).abs());
            drift = drift.max((q - f).abs());
        }
    }
    println!("L2 general CPU error {general_worst:e}, specialized/general drift {drift:e}");
    assert!(worst <= 2e-3, "resident CPU gate: {worst:e}");
    assert!(general_worst <= 2e-3, "general CPU gate: {general_worst:e}");
    r.set_specialization(true);

    // Adjustment layers on top (Curves and Hue/Saturation) run on the GPU.
    for (name, adj) in [
        (
            "curves",
            Adjustment::Curves {
                master: Curve(vec![[0.0, 0.05], [0.5, 0.6], [1.0, 0.95]]),
                rgb: Default::default(),
            },
        ),
        (
            "hue/sat",
            Adjustment::HueSaturation {
                hue: 20.0,
                saturation: 15.0,
                lightness: 0.0,
                colorize: false,
            },
        ),
    ] {
        d.apply(DocOp::AddLayer {
            parent: None,
            index: usize::MAX,
            layer: Layer::new(name, LayerKind::Adjustment(adj)),
        })
        .unwrap();
    }
    let mut runs = Vec::new();
    for _ in 0..5 {
        r.invalidate();
        let t = Instant::now();
        r.render(&d, 2).unwrap();
        let cpu = ms(t);
        r.wait().unwrap();
        runs.push((cpu, ms(t)));
    }
    println!("L2 full recomposite with 2 adjustment layers (cpu, total): {runs:.2?} ms");
    let s = r.stats();
    println!(
        "resident: {:.0} MB GPU ({} live pages), {:?}",
        s.resident_bytes as f64 / 1e6,
        s.live_pages,
        s
    );

    // Before: the CPU compositor's full level 0, and the per-tile GPU port
    // (CPU-resolved sources uploaded per tile) at level 2.
    let c = Compositor::new(4 << 30);
    let t = Instant::now();
    c.render_level(&d, 0, &Default::default()).unwrap();
    println!(
        "before: CPU L0 full composite (10 threads): {:.0} ms",
        ms(t)
    );
    c.render_level(&d, 1, &Default::default()).ok();
    let t = Instant::now();
    let (cols, rows) = e.at_level(2).tile_grid(256);
    let mut ok = true;
    for y in 0..rows {
        for x in 0..cols {
            ok &= gpu
                .render_tile_premultiplied(&c, &d, TileCoord::new(2, x, y))
                .is_ok();
        }
    }
    println!(
        "before: per-tile GPU port, L2 ({} tiles, sources uploaded per tile): {:.0} ms{}",
        cols * rows,
        ms(t),
        if ok {
            ""
        } else {
            " (adjustments unsupported there)"
        }
    );

    if std::env::var_os("TESSERA_BENCH_ASSERT").is_some() {
        assert!(dab_med < 16.0, "dab → L2 median {dab_med:.2} ms");
        assert!(l0_med < 100.0, "L0 median {l0_med:.1} ms");
        assert!(open < 1500.0, "cold open {open:.0} ms");
        assert!(worst <= 2e-3, "{worst:e}");
    }
}
