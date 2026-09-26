#![cfg(feature = "ml-denoise")]
use engine_api::{
    id::ModelRef,
    recipe::settings::{DenoiseMethod, DenoiseSettings},
};
use ml_runtime::ModelRegistry;
use pipeline_cpu::{Image, PostDemosaicDenoise};
use std::sync::Arc;

#[test]
#[ignore = "requires locally trained weights; run after documented training/export"]
fn local_trained_adapter_preserves_sensor_mask_for_every_pattern() {
    let registry = Arc::new(
        ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
            std::env::temp_dir().join("tessera-cfa-adapter-real"),
        )
        .unwrap(),
    );
    let spec = registry
        .models()
        .iter()
        .find(|m| m.id == "enhance/cfa-unet-fp32")
        .unwrap();
    let model = ModelRef {
        id: spec.id.as_str().into(),
        version: spec.version.clone(),
    };
    let settings = DenoiseSettings {
        method: DenoiseMethod::Neural {
            model: model.clone(),
            joint_demosaic: false,
        },
        amount: 100.0,
        ..Default::default()
    };
    let samples: Vec<f32> = (0..63 * 65)
        .map(|i| if i % 2 == 0 { 0.2 } else { 0.3 })
        .collect();
    let input = Image::new(63, 65, vec![samples.clone()]).unwrap();
    let mask: Vec<f32> = (0..63 * 65)
        .map(|i| if i % 7 == 0 { 1.0 } else { 0.0 })
        .collect();
    for pattern in [
        [[0, 1], [3, 2]],
        [[1, 0], [2, 3]],
        [[2, 3], [1, 0]],
        [[3, 2], [0, 1]],
    ] {
        let adapter = image_core::MlCfaDenoise::new(
            registry.clone(),
            ml_runtime::SessionOptions::cpu(),
            model.clone(),
            ml_enhance::CfaNoise {
                shot: [0.003; 4],
                read: [0.0008; 4],
            },
        )
        .with_mask(63, 65, mask.clone())
        .unwrap();
        let output = adapter
            .denoise_raw(&input, raw_decode::CfaLayout::Bayer(pattern), &settings)
            .unwrap();
        let mut changed = false;
        for i in 0..samples.len() {
            if mask[i] == 0.0 {
                assert_eq!(samples[i].to_bits(), output.planes()[0][i].to_bits());
            } else {
                changed |= samples[i] != output.planes()[0][i];
            }
        }
        assert!(changed);
    }
}
