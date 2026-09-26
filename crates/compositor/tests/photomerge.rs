use compositor::{Depth, DocOp, DocState, Document, LayerKind};
use engine_api::tile::Extent;
use merge::{
    LinearImage,
    layers::{AlignOptions, BlendOptions},
};

fn image() -> LinearImage {
    LinearImage {
        width: 24,
        height: 20,
        pixels: (0..480).map(|i| [(i % 17) as f32 / 17.; 3]).collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}

#[test]
fn photomerge_retains_source_transform_mask_and_atomic_history() {
    let mut doc = Document::new(DocState::new(Extent::new(1, 1), Depth::F32));
    let before = doc.state().clone();
    let result = doc
        .apply(DocOp::Photomerge {
            images: vec![("source".into(), image())],
            align: AlignOptions::default(),
            blend: BlendOptions::default(),
            fill: None,
        })
        .unwrap();
    assert_eq!(result.created.len(), 1);
    assert_eq!(doc.history().len(), 2);
    assert_eq!(doc.state().canvas, Extent::new(24, 20));
    assert!(doc.state().root[0].mask.is_some());
    assert!(matches!(
        doc.state().root[0].kind,
        LayerKind::SmartObject(_)
    ));
    let c = compositor::Compositor::new(1 << 24);
    let pixels = c.render_level_rgba(&doc, 0).unwrap().1;
    assert!((pixels[4] - image().pixels[1][0]).abs() < 1e-5);
    assert!(doc.undo());
    assert_eq!(doc.state().canvas, before.canvas);
    assert!(doc.state().root.is_empty());
    assert!(doc.redo());
    assert_eq!(pixels, c.render_level_rgba(&doc, 0).unwrap().1);
}

#[test]
fn layered_psd_export_keeps_masks_and_rasterizes_transforms() {
    let mut doc = Document::new(DocState::new(Extent::new(1, 1), Depth::F32));
    doc.apply(DocOp::Photomerge {
        images: vec![("a".into(), image())],
        align: AlignOptions::default(),
        blend: BlendOptions::default(),
        fill: None,
    })
    .unwrap();
    let export = doc.rasterized_layers_for_export().unwrap();
    let psd = compositor::psd::to_psd(&export).unwrap();
    let bytes = psd.write().unwrap();
    let loaded = Document::from_psd(psd::PsdDocument::read(&bytes).unwrap()).unwrap();
    assert_eq!(loaded.state().root.len(), 1);
    assert!(loaded.state().root[0].mask.is_some());
    let c = compositor::Compositor::new(1 << 24);
    let a = c.render_level_rgba(&doc, 0).unwrap().1;
    let b = c.render_level_rgba(&loaded, 0).unwrap().1;
    assert!(a.iter().zip(b).all(|(a, b)| (a - b).abs() < 1e-5));
    assert!(matches!(
        doc.state().root[0].kind,
        LayerKind::SmartObject(_)
    ));
}
