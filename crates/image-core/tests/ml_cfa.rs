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
