//! Bench: a 100-layer, 20 MP synthetic document composited at level 2.
//!
//! `cargo test -p compositor --release --test bench -- --ignored --nocapture`
//! (about 1.5 GB resident). Set `TESSERA_BENCH_ASSERT=1` to fail when the
//! composite-only time exceeds 100 ms.
use std::time::Instant;

use compositor::*;
use engine_api::tile::{Extent, TILE_SIZE, Tile, TileCoord};

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
