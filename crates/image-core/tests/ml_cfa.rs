#![cfg(feature = "ml-denoise")]
use engine_api::{
    id::ModelRef,
    recipe::settings::{DenoiseMethod, DenoiseSettings},
};
use ml_runtime::ModelRegistry;
use pipeline_cpu::{Image, PostDemosaicDenoise};
use std::sync::Arc;

#[test]
fn cfa_adapter_zero_amount_never_loads_missing_weights() -> engine_api::EngineResult<()> {
    let registry = Arc::new(
        ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
            std::env::temp_dir().join("tessera-cfa-adapter-tests"),
        )
        .unwrap(),
    );
    let model = ModelRef {
        id: "enhance/cfa-unet-fp32".into(),
        version: "0".repeat(64),
    };
    let adapter = image_core::MlCfaDenoise::new(
        registry,
        Default::default(),
        model.clone(),
        ml_enhance::CfaNoise {
            shot: [0.003; 4],
            read: [0.0008; 4],
        },
    );
    let settings = DenoiseSettings {
        method: DenoiseMethod::Neural {
            model,
            joint_demosaic: false,
        },
        amount: 0.0,
        ..Default::default()
    };
    let input = Image::new(4, 4, vec![vec![-0.0; 16]])?;
    let output = adapter.denoise_raw(
        &input,
        raw_decode::CfaLayout::Bayer([[0, 1], [3, 2]]),
        &settings,
    )?;
    assert!(
        output.planes()[0]
            .iter()
            .all(|v| v.to_bits() == (-0.0f32).to_bits())
    );
    Ok(())
}

#[test]
fn calibration_and_runtime_policy_change_adapter_identity() {
    let registry = Arc::new(
        ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
            std::env::temp_dir().join("tessera-cfa-identities"),
        )
        .unwrap(),
    );
    let model = ModelRef {
        id: "enhance/cfa-unet-fp32".into(),
        version: "a".repeat(64),
    };
    let noise = ml_enhance::CfaNoise {
        shot: [0.003; 4],
        read: [0.0008; 4],
    };
    let a = image_core::MlCfaDenoise::new(
        registry.clone(),
        ml_runtime::SessionOptions::cpu(),
        model.clone(),
        noise,
    );
    let b = image_core::MlCfaDenoise::new(
        registry.clone(),
        ml_runtime::SessionOptions::cpu(),
        model.clone(),
        ml_enhance::CfaNoise {
            read: [0.0009; 4],
            ..noise
        },
    );
    let c = image_core::MlCfaDenoise::new(
        registry.clone(),
        ml_runtime::SessionOptions::cpu(),
        ModelRef {
            version: "b".repeat(64),
            ..model.clone()
        },
        noise,
    );
    let mut options = ml_runtime::SessionOptions::cpu();
    options.coreml = true;
    let d = image_core::MlCfaDenoise::new(registry, options, model, noise);
    for revision in [
        b.adapter_revision(),
        c.adapter_revision(),
        d.adapter_revision(),
    ] {
        assert_ne!(a.adapter_revision(), revision);
    }
}

#[test]
fn all_zero_site_mask_never_loads_weights_for_odd_rotations() {
    let registry = Arc::new(
        ModelRegistry::open(
            concat!(env!("CARGO_MANIFEST_DIR"), "/../ml-runtime/models.toml"),
            std::env::temp_dir().join("tessera-cfa-empty-mask"),
        )
        .unwrap(),
    );
    let model = ModelRef {
        id: "enhance/cfa-unet-fp32".into(),
        version: "0".repeat(64),
    };
    let adapter = image_core::MlCfaDenoise::new(
        registry,
        Default::default(),
        model.clone(),
        ml_enhance::CfaNoise {
            shot: [0.003; 4],
            read: [0.0008; 4],
        },
    )
    .with_mask(7, 5, vec![0.0; 35])
    .unwrap();
    let input = Image::new(7, 5, vec![vec![-0.0; 35]]).unwrap();
    let settings = DenoiseSettings {
        method: DenoiseMethod::Neural {
            model,
            joint_demosaic: false,
        },
        amount: 100.0,
        ..Default::default()
    };
    for pattern in [
        [[0, 1], [3, 2]],
        [[1, 0], [2, 3]],
        [[2, 3], [1, 0]],
        [[3, 2], [0, 1]],
    ] {
        let output = adapter
            .denoise_raw(&input, raw_decode::CfaLayout::Bayer(pattern), &settings)
            .unwrap();
        assert!(
            output.planes()[0]
                .iter()
                .all(|v| v.to_bits() == (-0.0_f32).to_bits())
        );
    }
}
