use compositor::{Compositor, Depth, DocOp, DocState, Document};
use engine_api::tile::Extent;
use merge::{LinearImage, layers::*};

#[test]
fn corrected_photomerge_matches_merge_and_roundtrips_native_and_psd() {
    let image = LinearImage {
        width: 40,
        height: 32,
        pixels: (0..1280)
            .map(|i| [0.2 + 0.6 * (i % 40) as f32 / 40., 0.3, 0.4])
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    };
    let align = AlignOptions {
        geometric_distortion: true,
        vignette_removal: true,
        lens_corrections: vec![LensCorrection {
            distortion: [0.08, 0., 0.],
            vignette: [-0.15, 0., 0.],
        }],
        ..Default::default()
    };
    let aligned = align_layers(std::slice::from_ref(&image), &align).unwrap();
    let mut doc = Document::new(DocState::new(Extent::new(1, 1), Depth::F32));
    doc.apply(DocOp::Photomerge {
        images: vec![("calibrated source".into(), image)],
        align,
        blend: BlendOptions::default(),
        fill: None,
    })
    .unwrap();
    assert_eq!(
        doc.state().canvas,
        Extent::new(aligned.width as u32, aligned.height as u32)
    );
    let compositor = Compositor::new(1 << 24);
    let rendered = compositor.render_level_rgba(&doc, 0).unwrap().1;
    let mut compared = 0;
    for (p, expected) in rendered
        .as_chunks::<4>()
        .0
        .iter()
        .zip(&aligned.images[0].pixels)
    {
        if p[3] > 0.99999 {
            for c in 0..3 {
                assert!((p[c] - expected[c]).abs() < 2e-5);
            }
            compared += 1;
        }
    }
    assert!(compared > 500);
    let bytes = compositor::format::to_bytes(doc.state()).unwrap();
    let loaded = Document::new(compositor::format::from_bytes(&bytes).unwrap());
    assert_eq!(
        rendered,
        compositor.render_level_rgba(&loaded, 0).unwrap().1
    );
    let proxy = doc.rasterized_layers_for_export().unwrap();
    let bytes = compositor::psd::to_psd(&proxy).unwrap().write().unwrap();
    let loaded = Document::from_psd(psd::PsdDocument::read(&bytes).unwrap()).unwrap();
    assert!(loaded.state().root[0].mask.is_some());
    let exported = compositor.render_level_rgba(&loaded, 0).unwrap().1;
    assert!(
        rendered
            .iter()
            .zip(exported)
            .all(|(a, b)| (a - b).abs() < 2e-5)
    );
    assert!(doc.undo());
    assert!(doc.state().root.is_empty());
    assert!(doc.redo());
    assert_eq!(rendered, compositor.render_level_rgba(&doc, 0).unwrap().1);
}

#[test]
fn invalid_lens_calibration_leaves_history_and_document_unchanged() {
    let mut doc = Document::new(DocState::new(Extent::new(1, 1), Depth::F32));
    let before = doc.state().clone();
    for calibration in [
        LensCorrection {
            distortion: [-0.5, 0., 0.],
            ..Default::default()
        },
        LensCorrection {
            distortion: [f64::NAN, 0., 0.],
            ..Default::default()
        },
        LensCorrection {
            vignette: [-1., 0., 0.],
            ..Default::default()
        },
    ] {
        assert!(
            doc.apply(DocOp::Photomerge {
                images: vec![(
                    "bad lens".into(),
                    LinearImage {
                        width: 8,
                        height: 8,
                        pixels: vec![[0.5; 3]; 64],
                        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
                        as_shot_neutral: [1.; 3],
                    }
                )],
                align: AlignOptions {
                    vignette_removal: true,
                    geometric_distortion: true,
                    lens_corrections: vec![calibration],
                    ..Default::default()
                },
                blend: BlendOptions::default(),
                fill: None,
            })
            .is_err()
        );
        assert!(std::sync::Arc::ptr_eq(&before, doc.state()));
        assert_eq!(doc.history().len(), 1);
    }
}
