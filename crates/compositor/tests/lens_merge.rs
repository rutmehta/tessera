use compositor::{Depth, DocOp, DocState, Document, LayerKind};
use engine_api::tile::Extent;
use merge::{LinearImage, layers::*};

#[test]
fn vignette_is_editable_and_survives_photomerge_history() {
    let (w, h) = (32, 24);
    let pixels: Vec<_> = (0..w * h)
        .map(|i| {
            let r2 = (2. * (i % w) as f64 / w as f64 + 1. / w as f64 - 1.).powi(2)
                + (2. * (i / w) as f64 / h as f64 + 1. / h as f64 - 1.).powi(2);
            [(0.6 * (1. - 0.2 * r2)) as f32; 3]
        })
        .collect();
    let image = LinearImage {
        width: w,
        height: h,
        pixels,
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    };
    let mut doc = Document::new(DocState::new(Extent::new(1, 1), Depth::F32));
    doc.apply(DocOp::Photomerge {
        images: vec![("vignetted".into(), image)],
        align: AlignOptions {
            vignette_removal: true,
            lens_corrections: vec![LensCorrection {
                vignette: [-0.2, 0., 0.],
                ..Default::default()
            }],
            ..Default::default()
        },
        blend: BlendOptions::default(),
        fill: None,
    })
    .unwrap();
    let compositor = compositor::Compositor::new(1 << 24);
    let rendered = compositor.render_level_rgba(&doc, 0).unwrap().1;
    assert!(
        rendered
            .as_chunks::<4>()
            .0
            .iter()
            .all(|p| (p[0] - 0.6).abs() < 2e-6)
    );
    fn has_gain(layers: &[std::sync::Arc<compositor::Layer>]) -> bool {
        layers.iter().any(|layer| {
            layer.props.name == "Vignette removal"
                || match &layer.kind {
                    LayerKind::SmartObject(so) => has_gain(&so.state.root),
                    _ => false,
                }
        })
    }
    assert!(has_gain(&doc.state().root));
    assert!(doc.undo());
    assert!(doc.state().root.is_empty());
    assert!(doc.redo());
    assert_eq!(rendered, compositor.render_level_rgba(&doc, 0).unwrap().1);
}
