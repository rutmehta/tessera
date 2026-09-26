use compositor::{Compositor, Depth, DocOp, DocState, Document, LayerKind};
use engine_api::tile::Extent;
use merge::{
    LinearImage,
    layers::{AlignMode, AlignOptions, BlendOptions},
};

fn crop(offset: f64, angle: f64) -> LinearImage {
    let (s, c) = angle.sin_cos();
    LinearImage {
        width: 240,
        height: 180,
        pixels: (0..240 * 180)
            .map(|i| {
                let u = (i % 240) as f64;
                let v = (i / 240) as f64;
                let x = c * u - s * v + offset;
                let y = s * u + c * v;
                let p = (0.5
                    + 0.12 * (x * 0.17 + y * 0.09).sin()
                    + 0.1 * (x * 0.07 - y * 0.21).cos()
                    + 0.12 * ((x * 0.039).sin() * 7. + (y * 0.051).cos() * 9.).sin())
                    as f32;
                [p, p * 0.8, p * 0.6]
            })
            .collect(),
        color_matrix: [[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]],
        as_shot_neutral: [1.; 3],
    }
}
#[test]
fn three_source_photomerge_renders_blend_and_exports_nontrivial_masks() {
    let images = vec![crop(0., 0.), crop(140., 0.018), crop(280., -0.012)];
    let options = AlignOptions {
        mode: AlignMode::Collage,
        ..Default::default()
    };
    let aligned = merge::layers::align_layers(&images, &options).unwrap();
    let expected =
        merge::layers::blend_layers(&aligned.images, &aligned.coverage, &BlendOptions::default())
            .unwrap();
    let mut doc = Document::new(DocState::new(Extent::new(1, 1), Depth::F32));
    doc.apply(DocOp::Photomerge {
        images: images
            .into_iter()
            .enumerate()
            .map(|(i, im)| (format!("source {i}"), im))
            .collect(),
        align: options,
        blend: BlendOptions::default(),
        fill: None,
    })
    .unwrap();
    let c = Compositor::new(64 << 20);
    let (_, pixels) = c.render_level_rgba(&doc, 0).unwrap();
    assert_eq!(
        doc.state().canvas,
        Extent::new(aligned.width as u32, aligned.height as u32)
    );
    let mut checked = 0;
    for (i, p) in pixels.as_chunks::<4>().0.iter().enumerate() {
        if p[3] > 0.9999 {
            for (channel, expected) in p[..3].iter().zip(expected.image.pixels[i]) {
                assert!(
                    (channel - expected).abs() < 1e-4,
                    "pixel {i}: {p:?} != {:?}",
                    expected
                );
            }
            checked += 1;
        }
    }
    assert!(checked > 50000);
    let export = doc.rasterized_layers_for_export().unwrap();
    let psd = compositor::psd::to_psd(&export).unwrap();
    let loaded =
        Document::from_psd(psd::PsdDocument::read(&psd.write().unwrap()).unwrap()).unwrap();
    assert_eq!(loaded.state().root.len(), 3);
    for layer in &loaded.state().root {
        assert!(matches!(layer.kind, LayerKind::Pixel(_)));
        let mask = &layer.mask.as_ref().unwrap().raster;
        let mut black = false;
        let mut white = false;
        for y in 0..mask.extent().height {
            for x in 0..mask.extent().width {
                black |= mask.pixel(x, y)[0] == 0.;
                white |= mask.pixel(x, y)[0] == 1.;
            }
        }
        assert!(black && white);
    }
    let restored = c.render_level_rgba(&loaded, 0).unwrap().1;
    assert!(
        pixels
            .iter()
            .zip(restored)
            .all(|(a, b)| (a - b).abs() < 1e-4)
    );
    assert_eq!(doc.history().len(), 2);
    assert!(doc.undo());
    assert!(doc.state().root.is_empty());
}
