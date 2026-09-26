use compositor::{Compositor, Depth, DocOp, DocState, Document, Layer, LayerKind, Raster, Rect};
use engine_api::tile::Extent;
use merge::layers::{AlignOptions, BlendMode, BlendOptions};
use std::sync::atomic::AtomicBool;

fn document() -> (Document, compositor::LayerId) {
    let e = Extent::new(32, 24);
    let mut doc = Document::new(DocState::new(e, Depth::F32));
    let mut r = Raster::new(e, 4, Depth::F32, 0.);
    r.edit_region(Rect::of_extent(e), 1, |x, y, p| {
        *p = [
            0.3 + (x % 7) as f32 * 0.04,
            0.2,
            0.4,
            if (14..18).contains(&x) && (10..14).contains(&y) {
                0.
            } else {
                1.
            },
        ];
    })
    .unwrap();
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new("input", LayerKind::Pixel(r)),
        })
        .unwrap()
        .created[0];
    (doc, id)
}
fn caf(input: &Raster, holes: &[f32], seed: u64) -> engine_api::EngineResult<Raster> {
    Ok(filters::caf::fill(
        input,
        holes,
        &filters::caf::FillParams {
            seed,
            patch_radius: 1,
            iterations: 2,
            ..Default::default()
        },
        &AtomicBool::new(false),
    )?
    .composite)
}
#[test]
fn actual_caf_fills_only_union_hole_and_is_undoable() {
    let (mut doc, id) = document();
    let c = Compositor::new(1 << 24);
    let before = c.render_level_rgba(&doc, 0).unwrap().1;
    let result = doc
        .apply(DocOp::AutoBlendLayers {
            ids: vec![id],
            options: BlendOptions {
                mode: BlendMode::StackImages,
                fill_transparent: true,
                seed: 9,
                ..Default::default()
            },
            fill: Some(caf),
        })
        .unwrap();
    assert_eq!(result.created.len(), 1);
    let fill = doc
        .state()
        .find(result.created[0])
        .unwrap()
        .raster()
        .unwrap();
    assert_eq!(fill.pixel(0, 0)[3], 0.);
    assert_eq!(fill.pixel(15, 11)[3], 1.);
    let after = c.render_level_rgba(&doc, 0).unwrap().1;
    for (i, pixel) in before.as_chunks::<4>().0.iter().enumerate() {
        if pixel[3] == 1. {
            assert_eq!(pixel, &after[i * 4..i * 4 + 4]);
        }
    }
    assert!(doc.undo());
    assert_eq!(before, c.render_level_rgba(&doc, 0).unwrap().1);
    assert!(doc.redo());
    assert_eq!(after, c.render_level_rgba(&doc, 0).unwrap().1);
}
#[test]
fn invalid_ops_are_atomic_and_native_roundtrip_keeps_sources() {
    let (mut doc, id) = document();
    let before = doc.state().clone();
    for op in [
        DocOp::AutoAlignLayers {
            ids: vec![id, id],
            options: AlignOptions::default(),
        },
        DocOp::AutoBlendLayers {
            ids: vec![id],
            options: BlendOptions {
                fill_transparent: true,
                ..Default::default()
            },
            fill: None,
        },
        DocOp::AutoAlignLayers {
            ids: vec![id],
            options: AlignOptions {
                vignette_removal: true,
                ..Default::default()
            },
        },
    ] {
        assert!(doc.apply(op).is_err());
        assert!(std::sync::Arc::ptr_eq(&before, doc.state()));
    }
    doc.apply(DocOp::AutoAlignLayers {
        ids: vec![id],
        options: AlignOptions::default(),
    })
    .unwrap();
    let native = compositor::format::to_bytes(doc.state()).unwrap();
    let loaded = Document::new(compositor::format::from_bytes(&native).unwrap());
    let c = Compositor::new(1 << 24);
    assert_eq!(
        c.render_level_rgba(&doc, 0).unwrap().1,
        c.render_level_rgba(&loaded, 0).unwrap().1
    );
    assert!(matches!(
        loaded.state().find(id).unwrap().kind,
        LayerKind::SmartObject(_)
    ));
}
#[test]
fn locked_source_is_rejected_without_changing_history() {
    let (mut doc, id) = document();
    let mut props = doc.state().find(id).unwrap().props.clone();
    props.locks.position = true;
    doc.apply(DocOp::SetProps { id, props }).unwrap();
    let before = doc.state().clone();
    assert!(
        doc.apply(DocOp::AutoAlignLayers {
            ids: vec![id],
            options: AlignOptions::default()
        })
        .is_err()
    );
    assert!(std::sync::Arc::ptr_eq(&before, doc.state()));
}
#[test]
fn non_image_layer_is_not_silently_blended_away() {
    let (mut doc, _) = document();
    let id = doc
        .apply(DocOp::AddLayer {
            parent: None,
            index: 0,
            layer: Layer::new(
                "fill",
                LayerKind::Fill(compositor::Fill::Solid { color: [0.2; 3] }),
            ),
        })
        .unwrap()
        .created[0];
    assert!(
        doc.apply(DocOp::AutoBlendLayers {
            ids: vec![id],
            options: BlendOptions::default(),
            fill: None
        })
        .is_err()
    );
}
