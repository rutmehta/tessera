use compositor::document::Fill;
use compositor::raster::Depth;
use compositor::{Compositor, DocState, Document, Layer, LayerKind};
use engine_api::tile::Extent;
use std::sync::Arc;

#[test]
fn overlay_survives_zero_fill_and_native_roundtrip() {
    let mut layer = Layer::new(
        "styled",
        LayerKind::Fill(Fill::Solid {
            color: [0.0, 0.0, 1.0],
        }),
    );
    layer.props.fill_opacity = 0.0;
    // Use serde so the regression first fails on the old ignored field.
    layer.props = serde_json::from_value(serde_json::json!({
        "fill_opacity": 0.0,
        "styles": {"effects": [{"kind":"color_overlay", "settings": {}}]}
    }))
    .unwrap();
    let mut state = DocState::new(Extent::new(3, 3), Depth::F32);
    state.root.push(Arc::new(layer));
    let doc = Document::new(state);
    let pixels = Compositor::new(1 << 20)
        .render_level_rgba(&doc, 0)
        .unwrap()
        .1;
    assert_eq!(&pixels[0..4], &[1.0, 0.0, 0.0, 1.0]);
    let bytes = compositor::format::to_bytes(doc.state()).unwrap();
    let loaded = compositor::format::from_bytes(&bytes).unwrap();
    assert_eq!(
        loaded.root[0].props.styles,
        doc.state().root[0].props.styles
    );
    assert_eq!(loaded.global_light, doc.state().global_light);
    assert_eq!(
        Compositor::new(1 << 20)
            .render_level_rgba(&Document::new(loaded), 0)
            .unwrap()
            .1,
        pixels
    );
}

#[test]
fn layer_opacity_fades_entire_style_stack_once() {
    let mut layer = Layer::new(
        "styled",
        LayerKind::Fill(Fill::Solid {
            color: [0.0, 0.0, 1.0],
        }),
    );
    layer.props = serde_json::from_value(serde_json::json!({
        "opacity": 0.5,
        "styles": {"effects": [{"kind":"color_overlay", "settings": {}}]}
    }))
    .unwrap();
    let mut state = DocState::new(Extent::new(3, 3), Depth::F32);
    state.root.push(Arc::new(layer));
    let pixels = Compositor::new(1 << 20)
        .render_level_rgba(&Document::new(state), 0)
        .unwrap()
        .1;
    assert_eq!(&pixels[..4], &[1.0, 0.0, 0.0, 0.5]);
}

#[test]
fn global_light_changes_all_layers_and_undo_restores_cached_result() {
    use compositor::geom::Rect;
    use compositor::render::styles::{GlobalLight, Shadow, StyleEffect};
    use compositor::{DocOp, Raster};
    let extent = Extent::new(9, 5);
    let mut doc = Document::new(DocState::new(extent, Depth::F32));
    for x in [2, 6] {
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster
            .edit_region(Rect::new(x, 2, x + 1, 3), 1, |_, _, p| *p = [1.0; 4])
            .unwrap();
        let mut layer = Layer::new("dot", LayerKind::Pixel(raster));
        layer
            .props
            .styles
            .effects
            .push(StyleEffect::DropShadow(Shadow {
                distance: 1.0,
                size: 0.0,
                opacity: 1.0,
                ..Default::default()
            }));
        doc.apply(DocOp::AddLayer {
            parent: None,
            index: 2,
            layer,
        })
        .unwrap();
    }
    let c = Compositor::new(1 << 20);
    doc.apply(DocOp::SetGlobalLight(GlobalLight {
        angle: 0.0,
        elevation: 30.0,
    }))
    .unwrap();
    let left = c.render_level_rgba(&doc, 0).unwrap().1;
    for x in [1, 5] {
        assert_eq!(left[(2 * 9 + x) * 4 + 3], 1.0);
    }
    doc.apply(DocOp::SetGlobalLight(GlobalLight {
        angle: 180.0,
        elevation: 30.0,
    }))
    .unwrap();
    let right = c.render_level_rgba(&doc, 0).unwrap().1;
    for x in [3, 7] {
        assert_eq!(right[(2 * 9 + x) * 4 + 3], 1.0);
    }
    assert_ne!(left, right);
    assert!(doc.undo());
    assert_eq!(left, c.render_level_rgba(&doc, 0).unwrap().1);
}

#[test]
fn overlay_preserves_antialiased_shape_alpha() {
    use compositor::{
        Raster,
        geom::Rect,
        render::styles::{Overlay, StyleEffect},
    };
    let extent = Extent::new(1, 1);
    let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
    raster
        .edit_region(Rect::of_extent(extent), 1, |_, _, p| {
            *p = [0.0, 0.0, 1.0, 0.5]
        })
        .unwrap();
    let mut layer = Layer::new("soft edge", LayerKind::Pixel(raster));
    layer
        .props
        .styles
        .effects
        .push(StyleEffect::ColorOverlay(Overlay::default()));
    let mut state = DocState::new(extent, Depth::F32);
    state.root.push(Arc::new(layer));
    let pixels = Compositor::new(1 << 20)
        .render_level_rgba(&Document::new(state), 0)
        .unwrap()
        .1;
    assert_eq!(pixels, vec![1.0, 0.0, 0.0, 0.5]);
}

#[test]
fn shadow_crosses_tiles_and_paint_invalidates_its_neighbour() {
    use compositor::{
        DocOp, PaintTarget, Raster,
        edit::paint_op,
        geom::Rect,
        render::styles::{Shadow, StyleEffect},
    };
    use engine_api::tile::TileCoord;
    let extent = Extent::new(260, 3);
    let mut r = Raster::new(extent, 4, Depth::F32, 0.0);
    r.edit_region(Rect::new(255, 1, 256, 2), 1, |_, _, p| *p = [1.0; 4])
        .unwrap();
    let mut layer = Layer::new("dot", LayerKind::Pixel(r));
    layer
        .props
        .styles
        .effects
        .push(StyleEffect::DropShadow(Shadow {
            angle: 180.0,
            use_global_light: false,
            distance: 1.0,
            size: 0.0,
            opacity: 1.0,
            ..Default::default()
        }));
    let mut doc = Document::new(DocState::new(extent, Depth::F32));
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer,
        })
        .unwrap()
        .created[0];
    assert!(matches!(
        doc.state().check_resident_effects(),
        Err(engine_api::EngineError::Unsupported { .. })
    ));
    let c = Compositor::new(1 << 20);
    let tile = c.render_tile(&doc, TileCoord::new(0, 1, 0)).unwrap();
    assert!(tile.samples::<f32>().unwrap()[3 * 12 + 4] > 0.99);
    let op = paint_op(
        doc.state(),
        id,
        PaintTarget::Content,
        Rect::new(255, 1, 256, 2),
        |_, _, p| *p = [0.0; 4],
    )
    .unwrap();
    assert_eq!(doc.apply(op).unwrap().damage, Rect::of_extent(extent));
    let tile = c.render_tile(&doc, TileCoord::new(0, 1, 0)).unwrap();
    assert_eq!(tile.samples::<f32>().unwrap()[3 * 12 + 4], 0.0);
}
