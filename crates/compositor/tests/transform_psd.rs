use compositor::{
    Affine, Compositor, Depth, DocState, Document, Layer, LayerKind, Raster, Rect, SmartObject,
};
use engine_api::tile::Extent;
use std::sync::Arc;
#[test]
fn affine_placed_layer_roundtrips_real_psd_bytes_and_source() {
    let e = Extent::new(4, 3);
    let mut child = DocState::new(e, Depth::F32);
    let mut r = Raster::new(e, 4, Depth::F32, 0.);
    r.edit_region(Rect::new(0, 0, 2, 2), 1, |_, _, p| {
        *p = [0.8, 0.2, 0.1, 0.5]
    })
    .unwrap();
    child
        .root
        .push(Arc::new(Layer::new("original", LayerKind::Pixel(r))));
    let affine = Affine {
        m: [1., 0., 1., 0., 1., 0.],
    };
    let mut state = DocState::new(Extent::new(8, 6), Depth::F32);
    state.root.push(Arc::new(Layer::new(
        "placed",
        LayerKind::SmartObject(SmartObject::new(child, affine)),
    )));
    let doc = Document::new(state);
    let psd = compositor::psd::to_psd(&doc).unwrap();
    let record = &psd.layer_section.layers[0];
    let d = psd::metadata::parse_smart_object(&record.info(b"SoLd").unwrap().data).unwrap();
    assert!(d.descriptor.get(b"Trnf").is_some());
    let bytes = psd.write().unwrap();
    let loaded = Document::from_psd(psd::PsdDocument::read(&bytes).unwrap()).unwrap();
    let again = compositor::psd::to_psd(&loaded).unwrap();
    assert_eq!(
        again
            .layer_section
            .additional
            .iter()
            .filter(|b| b.key == *b"lnk2")
            .count(),
        1
    );
    let LayerKind::SmartObject(so) = &loaded.state().root[0].kind else {
        panic!()
    };
    assert_eq!(so.transform, affine);
    assert_eq!(so.state.canvas, e);
    let c = Compositor::new(1 << 20);
    assert_eq!(
        c.render_level_rgba(&doc, 0).unwrap().1,
        c.render_level_rgba(&loaded, 0).unwrap().1
    );
    // PlLd is the same descriptor-based structure, not legacy lowercase plLd.
    let mut alias = psd.clone();
    alias.layer_section.layers[0]
        .additional
        .iter_mut()
        .find(|b| b.key == *b"SoLd")
        .unwrap()
        .key = *b"PlLd";
    let alias =
        Document::from_psd(psd::PsdDocument::read(&alias.write().unwrap()).unwrap()).unwrap();
    let LayerKind::SmartObject(so) = &alias.state().root[0].kind else {
        panic!()
    };
    assert_eq!(so.transform, affine);
    // Without a usable embedded source, retain the already-rendered proxy.
    let mut missing = psd.clone();
    missing.layer_section.additional.clear();
    let missing = Document::from_psd(missing).unwrap();
    assert_eq!(
        c.render_level_rgba(&missing, 0).unwrap().1,
        c.render_level_rgba(&doc, 0).unwrap().1
    );
    for m in [
        [0., -1., 4., 1., 0., 0.],
        [-1., 0., 4., 0., 1., 0.],
        [1., 0.5, 0., 0., 1., 0.],
        [0.5, 0., 1., 0., 2., 0.],
    ] {
        let mut next = loaded.clone();
        next.apply(compositor::DocOp::SetSmartTransform {
            id: next.state().root[0].id,
            transform: Affine { m },
        })
        .unwrap();
        let bytes = compositor::psd::to_psd(&next).unwrap().write().unwrap();
        let round = Document::from_psd(psd::PsdDocument::read(&bytes).unwrap()).unwrap();
        let LayerKind::SmartObject(so) = &round.state().root[0].kind else {
            panic!()
        };
        assert_eq!(so.transform, Affine { m });
        assert_eq!(
            c.render_level_rgba(&next, 0).unwrap().1,
            c.render_level_rgba(&round, 0).unwrap().1
        );
    }
}
