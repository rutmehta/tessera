//! `.tessera-doc` round trip of the whole model.
mod common;
use common::*;
use compositor::*;
use engine_api::tile::Extent;

fn rich_doc() -> Document {
    let e = Extent::new(300, 270);
    let mut d = doc(e, Depth::U16);
    let bg = add(
        &mut d,
        None,
        layer_fn("bg", e, Depth::U16, |x, y| {
            opaque([x as f32 / 300.0, 0.5, y as f32 / 270.0])
        }),
    );
    set_props(&mut d, bg, |p| p.background = true);
    let px = add(
        &mut d,
        None,
        layer_fn("px", e, Depth::U16, |x, y| {
            [0.9, (x ^ y) as f32 / 511.0, 0.1, 0.5]
        }),
    );
    set_props(&mut d, px, |p| {
        p.blend_mode = BlendMode::Overlay;
        p.blend_if.gray.underlying = [0.1, 0.2, 0.8, 0.95];
        p.color_tag = Some("red".into());
        p.locks.position = true;
    });
    let mut m = Mask::reveal_all(e, Depth::U16);
    m.raster
        .edit_region(Rect::new(0, 0, 100, 100), 1, |_, _, p| p[0] = 0.25)
        .unwrap();
    m.density = 0.8;
    m.feather = 3.0;
    d.apply(DocOp::SetMask {
        id: px,
        mask: Some(m),
    })
    .unwrap();
    let dup = d.apply(DocOp::DuplicateLayer { id: px }).unwrap().created[0];
    set_props(&mut d, dup, |p| p.knockout = Knockout::Shallow);
    let g = add(
        &mut d,
        None,
        Layer::group("g", GroupMode::Isolated).with_mode(BlendMode::Screen),
    );
    add(
        &mut d,
        Some(g),
        Layer::new(
            "curves",
            LayerKind::Adjustment(Adjustment::Curves {
                master: Curve(vec![[0.0, 0.1], [0.5, 0.6], [1.0, 0.9]]),
                rgb: Default::default(),
            }),
        ),
    );
    add(
        &mut d,
        Some(g),
        Layer::new(
            "fill",
            LayerKind::Fill(Fill::Solid {
                color: [0.2, 0.3, 0.4],
            }),
        )
        .with_opacity(0.3),
    );
    let mut child = Document::new(DocState::new(Extent::new(40, 30), Depth::F32));
    child
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: layer_fn("c", Extent::new(40, 30), Depth::F32, |x, _| {
                opaque([x as f32 / 40.0, 0.2, 0.7])
            }),
        })
        .unwrap();
    let mut so = SmartObject::new(
        (**child.state()).clone(),
        Affine::scale_translate(2.0, 2.0, 50.0, 60.0),
    );
    so.filters.push(SmartFilter {
        name: "gaussian_blur".into(),
        enabled: true,
        params: serde_json::json!({"radius": 2.5}),
    });
    add(
        &mut d,
        None,
        Layer::new("so", LayerKind::SmartObject(so)).with_mode(BlendMode::Multiply),
    );
    add(
        &mut d,
        None,
        Layer::new(
            "text",
            LayerKind::Text(TextLayer {
                text: "Hello".into(),
                font: "Inter".into(),
                size: 24.0,
                color: [1.0, 1.0, 1.0],
                proxy: Raster::new(e, 4, Depth::U16, 0.0),
            }),
        ),
    );
    let mut vm = Layer::pixel("vm", e, Depth::U16);
    vm.vector_mask = Some(VectorMask {
        enabled: true,
        path: serde_json::json!([[0, 0], [10, 0], [10, 10]]),
    });
    add(&mut d, None, vm);
    // Erase a tile to leave a tombstone.
    d.apply(DocOp::PaintTiles {
        id: px,
        target: PaintTarget::Content,
        tiles: vec![TileDelta {
            tx: 1,
            ty: 1,
            tile: None,
        }],
        dirty: Rect::new(256, 256, 300, 270),
    })
    .unwrap();
    let sel = compositor::document::selection::rect(e, Rect::new(5, 5, 50, 50)).unwrap();
    d.apply(DocOp::SetSelection {
        selection: Some(sel),
    })
    .unwrap();
    let mut st = (**d.state()).clone();
    st.profile = Some(ColorProfile::from_icc("Fake RGB", vec![1, 2, 3, 4, 5]));
    st.ppi = 300.0;
    Document::new(st)
}

#[test]
fn round_trip_preserves_model_and_pixels() {
    let d = rich_doc();
    let bytes = format::to_bytes(d.state()).unwrap();
    let back = format::from_bytes(&bytes).unwrap();
    // Re-serializing yields identical bytes: nothing was lost or changed.
    assert_eq!(format::to_bytes(&back).unwrap(), bytes);
    let d2 = Document::new(back);
    assert_eq!(render(&d), render(&d2));
    for level in [1, 2] {
        let c = Compositor::new(64 << 20);
        assert_eq!(
            c.render_level_rgba(&d, level).unwrap(),
            c.render_level_rgba(&d2, level).unwrap()
        );
    }
    let s = d2.state();
    assert_eq!(s.ppi, 300.0);
    assert_eq!(
        s.profile
            .as_ref()
            .unwrap()
            .icc
            .as_deref()
            .map(|v| v.as_slice()),
        Some(&[1u8, 2, 3, 4, 5][..])
    );
    assert!(s.selection.is_some());
    assert_eq!(s.layer_ids(), d.state().layer_ids());
    // COW-shared tiles are stored once: the duplicate adds no chunks.
    let mut single = (**d.state()).clone();
    let dup_id = single.root[2].id;
    single.root.retain(|l| l.id != dup_id);
    let smaller = format::to_bytes(&single).unwrap();
    assert!(
        bytes.len() - smaller.len() < 2048,
        "duplicate costs only manifest bytes"
    );
}

#[test]
fn save_and_load_files() {
    let d = rich_doc();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("x.tessera-doc");
    format::save(&d, &path).unwrap();
    let back = format::load(&path).unwrap();
    assert_eq!(render(&d), render(&back));
    // Edits after load get fresh revisions above everything loaded.
    let mut back = back;
    let id = back.state().root[1].id;
    let rev = back
        .apply(
            paint_op(
                back.state(),
                id,
                PaintTarget::Content,
                Rect::new(0, 0, 4, 4),
                |_, _, p| *p = [1.0; 4],
            )
            .unwrap(),
        )
        .unwrap()
        .rev;
    assert!(rev > d.state().rev);
    assert!(format::from_bytes(b"not a document at all, definitely").is_err());
}
