use compositor::{Adjustment, Compositor, Depth, DocOp, DocState, Document, Layer, LayerKind};
use engine_api::tile::{Extent, TileCoord};

#[test]
fn direct_serde_invalid_lookup_returns_error_instead_of_panicking() {
    let adjustment: Adjustment =
        serde_json::from_str(r#"{"kind":"color_lookup","size":2,"data":[]}"#).unwrap();
    let extent = Extent::new(1, 1);
    let mut doc = Document::new(DocState::new(extent, Depth::F32));
    doc.apply(DocOp::AddLayer {
        parent: None,
        index: 0,
        layer: Layer::new("invalid", LayerKind::Adjustment(adjustment)),
    })
    .unwrap();
    assert!(
        Compositor::new(1024)
            .render_tile_premultiplied(&doc, TileCoord::new(0, 0, 0))
            .is_err()
    );
}
