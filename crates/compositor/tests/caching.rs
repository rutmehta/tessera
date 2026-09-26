//! Copy-on-write, cache invalidation counts, dirty-rect compositing and
//! non-linear history.
mod common;
use common::*;
use compositor::*;
use engine_api::tile::{Extent, TileCoord};

fn pattern(seed: u32) -> impl Fn(u32, u32) -> [f32; 4] {
    move |x, y| {
        let h = x
            .wrapping_mul(31)
            .wrapping_add(y.wrapping_mul(17))
            .wrapping_add(seed * 101);
        [
            (h % 251) as f32 / 250.0,
            ((h / 7) % 241) as f32 / 240.0,
            ((x + seed) % 97) as f32 / 96.0,
            0.35 + ((y + seed) % 13) as f32 / 20.0,
        ]
    }
}

fn three_layer_doc(e: Extent) -> (Document, Vec<LayerId>) {
    let mut d = doc(e, Depth::U8);
    let mut ids = vec![];
    for (i, mode) in [BlendMode::Normal, BlendMode::Multiply, BlendMode::Screen]
        .into_iter()
        .enumerate()
    {
        ids.push(add(
            &mut d,
            None,
            layer_fn("l", e, Depth::U8, pattern(i as u32)).with_mode(mode),
        ));
    }
    (d, ids)
}

fn dab(d: &mut Document, id: LayerId, r: Rect) {
    let op = paint_op(d.state(), id, PaintTarget::Content, r, |_, _, p| {
        *p = [1.0, 0.5, 0.0, 1.0]
    })
    .unwrap();
    d.apply(op).unwrap();
}

#[test]
fn duplicate_then_edit_one_tile_leaves_original_untouched() {
    let e = Extent::new(512, 512);
    let mut d = doc(e, Depth::U8);
    let a = add(&mut d, None, layer_fn("a", e, Depth::U8, pattern(3)));
    let before_bytes = d.history_bytes();
    let dup = d.apply(DocOp::DuplicateLayer { id: a }).unwrap().created[0];
    // Duplication shares every tile.
    let ra = d.state().find(a).unwrap().raster().unwrap().clone();
    let rd = d.state().find(dup).unwrap().raster().unwrap().clone();
    assert!(ra.shares_all_tiles_with(&rd));
    assert_eq!(
        d.history_bytes(),
        before_bytes,
        "duplicate allocates no pixels"
    );
    // Paint inside tile (0, 0) of the duplicate.
    dab(&mut d, dup, Rect::new(10, 10, 40, 40));
    let ra2 = d.state().find(a).unwrap().raster().unwrap();
    let rd2 = d.state().find(dup).unwrap().raster().unwrap();
    assert!(ra2.shares_all_tiles_with(&ra), "original unchanged");
    assert_eq!(ra2.pixel(20, 20), ra.pixel(20, 20));
    assert_eq!(rd2.pixel(20, 20), [1.0, 128.0 / 255.0, 0.0, 1.0]);
    assert!(
        !rd2.tile(0, 0)
            .unwrap()
            .shares_buffer_with(ra2.tile(0, 0).unwrap())
    );
    for (tx, ty) in [(1, 0), (0, 1), (1, 1)] {
        assert!(
            rd2.tile(tx, ty)
                .unwrap()
                .shares_buffer_with(ra2.tile(tx, ty).unwrap())
        );
    }
    // History holds exactly one extra tile (256² × 4 B).
    assert_eq!(d.history_bytes(), before_bytes + 256 * 256 * 4);
}

#[test]
fn cache_invalidation_is_local_to_the_edited_tiles() {
    let e = Extent::new(1024, 1024); // 4×4 tiles at L0, 2×2 at L1
    let (mut d, ids) = three_layer_doc(e);
    let c = Compositor::new(512 << 20);
    c.render_level(&d, 1, &Default::default()).unwrap();
    let s = c.stats();
    assert_eq!(
        (s.mip_tiles, s.root_full),
        (12, 4),
        "cold L1: 3 layers × 4 mips, 4 composites"
    );

    c.reset_stats();
    c.render_level(&d, 1, &Default::default()).unwrap();
    let s = c.stats();
    assert_eq!(
        (s.mip_tiles, s.root_full, s.root_partial, s.blends),
        (0, 0, 0, 0),
        "warm: all hits"
    );

    // A dab inside L0 tile (1, 1) → L1 tile (0, 0) only.
    c.reset_stats();
    dab(&mut d, ids[1], Rect::new(300, 300, 340, 330));
    c.render_level(&d, 1, &Default::default()).unwrap();
    let s = c.stats();
    assert_eq!(s.mip_tiles, 1, "only the painted layer's mip of one tile");
    assert_eq!(s.root_full + s.root_partial, 1, "one composite tile redone");
    assert_eq!(s.root_partial, 1, "and only its dirty rectangle");
    assert_eq!(s.blends, 3);

    // Opacity change: every composite tile the layer covers, no mips.
    c.reset_stats();
    set_props(&mut d, ids[0], |p| p.opacity = 0.5);
    c.render_level(&d, 1, &Default::default()).unwrap();
    let s = c.stats();
    assert_eq!(
        s.mip_tiles, 0,
        "mips are keyed by raster revisions, not props"
    );
    assert_eq!(s.root_full, 4);
}

#[test]
fn dirty_rect_compositing_is_bit_exact() {
    let e = Extent::new(600, 520);
    let (mut d, ids) = three_layer_doc(e);
    let g = add(&mut d, None, Layer::group("g", GroupMode::Isolated));
    add(
        &mut d,
        Some(g),
        layer_fn("in", e, Depth::U8, pattern(9)).with_mode(BlendMode::Overlay),
    );
    let c = Compositor::new(512 << 20);
    for level in [0, 1] {
        c.render_level(&d, level, &Default::default()).unwrap();
    }
    for (i, r) in [
        Rect::new(250, 250, 270, 262),
        Rect::new(0, 500, 20, 520),
        Rect::new(510, 5, 599, 30),
    ]
    .into_iter()
    .enumerate()
    {
        dab(&mut d, ids[i], r);
        for level in [0, 1] {
            c.reset_stats();
            let inc = c.render_level(&d, level, &Default::default()).unwrap();
            assert!(
                c.stats().root_partial >= 1,
                "incremental path used: L{level} rect {i} {:?}",
                c.stats()
            );
            let fresh = Compositor::new(512 << 20)
                .render_level(&d, level, &Default::default())
                .unwrap();
            for (a, b) in inc.iter().zip(&fresh) {
                assert_eq!(
                    a.samples::<f32>().unwrap(),
                    b.samples::<f32>().unwrap(),
                    "L{level} {}",
                    a.coord()
                );
            }
        }
    }
}

#[test]
fn non_linear_history_and_snapshots() {
    let e = Extent::new(300, 300);
    let (mut d, ids) = three_layer_doc(e);
    let c = Compositor::new(256 << 20);
    let s0 = c.render_level_rgba(&d, 0).unwrap().1;
    d.snapshot("start");
    dab(&mut d, ids[2], Rect::new(0, 0, 100, 100));
    let node_a = d.history().current();
    let sa = c.render_level_rgba(&d, 0).unwrap().1;
    assert_ne!(s0, sa);
    assert!(d.undo());
    assert_eq!(c.render_level_rgba(&d, 0).unwrap().1, s0);
    // Branch: a different edit from the same parent.
    set_props(&mut d, ids[0], |p| p.visible = false);
    let sb = c.render_level_rgba(&d, 0).unwrap().1;
    // Jump back to the other branch — both states are retained.
    d.checkout(node_a).unwrap();
    assert_eq!(c.render_level_rgba(&d, 0).unwrap().1, sa);
    d.restore_snapshot("start").unwrap();
    assert_eq!(c.render_level_rgba(&d, 0).unwrap().1, s0);
    assert!(d.redo());
    assert_eq!(
        c.render_level_rgba(&d, 0).unwrap().1,
        sb,
        "redo goes to the newest branch"
    );
    // Composites after history jumps equal a cold render.
    assert_eq!(render(&d), sb);

    // Pruning keeps memory bounded but never drops the current path or snapshots.
    d.set_max_states(4);
    for i in 0..10 {
        dab(&mut d, ids[1], Rect::new(i * 10, 0, i * 10 + 5, 5));
    }
    assert!(d.history().len() <= 4 + 1);
    d.restore_snapshot("start").unwrap();
    assert_eq!(c.render_level_rgba(&d, 0).unwrap().1, s0);
}

#[test]
fn pyramid_view_matches_level_render() {
    use engine_api::tile::Pyramid;
    let e = Extent::new(700, 300);
    let (d, _) = three_layer_doc(e);
    let c = Compositor::new(256 << 20);
    let p = c.pyramid(&d);
    assert_eq!(p.level_count(), 3);
    let t = p.tile(TileCoord::new(1, 1, 0)).unwrap();
    let lvl = c.render_level(&d, 1, &Default::default()).unwrap();
    assert_eq!(
        t.samples::<f32>().unwrap(),
        lvl[1].samples::<f32>().unwrap()
    );
    assert!(p.tile(TileCoord::new(0, 9, 0)).is_err());
}
