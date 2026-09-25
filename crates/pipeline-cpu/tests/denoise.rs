use engine_api::{
    id::ModelRef,
    recipe::{DevelopSettings, settings::DenoiseMethod},
};

use engine_api::color::{ColorMatrix3, WorkingSpace};
use pipeline_cpu::{Image, PostDemosaicDenoise, post_demosaic_denoise};
struct Identity;
impl PostDemosaicDenoise for Identity {
    fn adapter_revision(&self) -> &str {
        "identity"
    }
    fn denoise(&self, image: &Image, _: f32) -> engine_api::EngineResult<Image> {
        assert!(
            image
                .planes()
                .iter()
                .flatten()
                .all(|v| (0.0..=1.0).contains(v))
        );
        Ok(image.clone())
    }
}
fn neural() -> engine_api::recipe::settings::DenoiseSettings {
    engine_api::recipe::settings::DenoiseSettings {
        method: DenoiseMethod::Neural {
            model: ModelRef {
                id: pipeline_cpu::POST_DENOISE_MODEL_ID.into(),
                version: pipeline_cpu::POST_DENOISE_VERSION.into(),
            },
            joint_demosaic: false,
        },
        amount: 50.0,
        chroma_only: false,
    }
}
#[test]
fn colour_residual_preserves_hdr_and_out_of_gamut() {
    let input = Image::new(
        3,
        1,
        vec![
            vec![-2.0, 0.3, 4.0],
            vec![1.5, -0.1, 0.7],
            vec![0.4, 2.0, -1.0],
        ],
    )
    .unwrap();
    let matrix = WorkingSpace::LinearRec2020.to_xyz() * ColorMatrix3::diagonal([0.8, 1.1, 1.3]);
    let out = post_demosaic_denoise(input.clone(), matrix, &neural(), Some(&Identity)).unwrap();
    for (a, b) in input
        .planes()
        .iter()
        .flatten()
        .zip(out.planes().iter().flatten())
    {
        assert!((a - b).abs() < 1e-6);
    }
}
#[test]
fn off_and_zero_skip_colour_transform_and_runtime_exactly() {
    let input = Image::new(1, 1, vec![vec![-2.0], vec![5.0], vec![-0.0]]).unwrap();
    let mut s = neural();
    s.amount = 0.0;
    for settings in [s, Default::default()] {
        let out = post_demosaic_denoise(
            input.clone(),
            ColorMatrix3::diagonal([0.0; 3]),
            &settings,
            None,
        )
        .unwrap();
        assert_eq!(
            input
                .planes()
                .iter()
                .flatten()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>(),
            out.planes()
                .iter()
                .flatten()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    }
}
#[test]
fn unsupported_settings_fail_even_at_zero() {
    let mut s = neural();
    s.amount = 0.0;
    s.chroma_only = true;
    assert!(pipeline_cpu::validate_denoise(&s).is_err());
    s.chroma_only = false;
    if let DenoiseMethod::Neural {
        joint_demosaic,
        model,
    } = &mut s.method
    {
        *joint_demosaic = true;
        model.version = "unversioned".into();
    }
    assert!(pipeline_cpu::validate_denoise(&s).is_err());
    s = neural();
    s.amount = f32::NAN;
    assert!(pipeline_cpu::validate_denoise(&s).is_err());
}

#[test]
fn neural_zero_is_supported_without_a_runtime() {
    let mut settings = DevelopSettings::default();
    settings.denoise.method = DenoiseMethod::Neural {
        model: ModelRef {
            id: "enhance/drunet-color".into(),
            version: "a2b9fccfa27b197f44a3876c567f5e48970c44a7".into(),
        },
        joint_demosaic: false,
    };
    settings.denoise.amount = 0.0;
    assert!(pipeline_cpu::validate_settings(&settings).is_ok());
}
