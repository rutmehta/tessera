#![cfg(feature = "ml-denoise")]
mod common;
#[path = "common/cfa.rs"]
mod inference;
use engine_api::{
    recipe::settings::{DenoiseMethod, HighlightReconstruction},
    tile::Extent,
};
use image_core::{PixelRect, Renderer, RendererConfig, cfa::PackedCfa};
use std::sync::{Arc, atomic::Ordering};
#[test]
fn packed_cfa_preserves_all_rotations_odd_edges_and_site_masks() {
    for (w, h) in [(7, 5), (6, 5), (7, 6), (2, 2), (263, 259)] {
        for turns in 0..4 {
            let packing = ml_enhance::BayerPacking::new(w, h, turns).unwrap();
            let data: Vec<f32> = (0..w * h)
                .map(|i| {
                    if i % 3 == 0 {
                        -0.0
                    } else {
                        (i as f32 * 0.01).sin()
                    }
                })
                .collect();
            let full: Vec<f32> = data.iter().map(|v| v * 0.9 + 0.03).collect();
            let mask: Vec<f32> = (0..w * h).map(|i| [0.0, 0.25, 1.0][i % 3]).collect();
            let packed = PackedCfa::new(
                Extent::new(w as u32, h as u32),
                turns,
                packing.pack(&full).unwrap().data().to_vec(),
                Some(packing.pack(&mask).unwrap().data().to_vec()),
            )
            .unwrap();
            let input = pipeline_cpu::Image::new(w as u32, h as u32, vec![data.clone()]).unwrap();
            for amount in [0.0, 0.5, 1.0] {
                let result = packed.blend_cpu(&input, amount).unwrap();
                for i in 0..w * h {
                    let alpha = amount * mask[i];
                    let expected = if alpha == 0.0 {
                        data[i]
                    } else if alpha == 1.0 {
                        full[i]
                    } else {
                        data[i] * (1.0 - alpha) + full[i] * alpha
                    };
                    assert_eq!(
                        result.planes()[0][i].to_bits(),
                        expected.to_bits(),
                        "{w}x{h} turn {turns}, site {i}"
                    );
                }
            }
        }
    }
    assert!(PackedCfa::new(Extent::new(3, 3), 4, vec![0.0; 16], None).is_err());
    assert!(PackedCfa::new(Extent::new(3, 3), 0, vec![f32::NAN; 16], None).is_err());
    assert!(PackedCfa::new(Extent::new(3, 3), 0, vec![0.0; 16], Some(vec![1.1; 16])).is_err());
}
#[test]
fn inference_identity_excludes_amount_tone_but_includes_image_upstream_model_adapter() {
    let infer = Arc::new(inference::Inference::default());
    let renderer = Renderer::new(RendererConfig::default()).with_cfa_denoise(infer.clone());
    let image = common::synthetic(810, 37, 35, common::RGGB, [1, 1, 35, 33]);
    let rect = PixelRect::full(image.level_extent(0));
    let mut s = inference::settings();
    for amount in [100.0, 30.0, 0.0, 75.0] {
        s.denoise.amount = amount;
        renderer.render_region(&image, &s, 0, rect).unwrap();
        assert_eq!(infer.calls.load(Ordering::SeqCst), 1);
    }
    s.tone.exposure = 0.2;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(infer.calls.load(Ordering::SeqCst), 1);
    s.linearize.highlight_reconstruction = HighlightReconstruction::Clip;
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(infer.calls.load(Ordering::SeqCst), 2);
    if let DenoiseMethod::Neural { model, .. } = &mut s.denoise.method {
        model.version = "b".repeat(64);
    }
    renderer.render_region(&image, &s, 0, rect).unwrap();
    assert_eq!(infer.calls.load(Ordering::SeqCst), 3);
    let second = common::synthetic(811, 37, 35, common::RGGB, [1, 1, 35, 33]);
    renderer.render_region(&second, &s, 0, rect).unwrap();
    assert_eq!(infer.calls.load(Ordering::SeqCst), 4);
    let next = Arc::new(inference::Inference {
        revision: "test/noise-2".into(),
        ..Default::default()
    });
    let renderer = renderer.with_cfa_denoise(next.clone());
    renderer.render_region(&second, &s, 0, rect).unwrap();
    assert_eq!(next.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn packed_handoff_retains_owned_tensor_storage() {
    let tensor = ml_runtime::Tensor::new(4, 3, 4, vec![0.3; 48]).unwrap();
    let pointer = tensor.data().as_ptr();
    let full = PackedCfa::from_tensors(Extent::new(7, 5), 0, tensor, None).unwrap();
    assert_eq!(pointer, full.samples().as_ptr());
}
