use color_mgmt::{Builtin, Registry};
use engine_api::{
    color::IccProfileHandle,
    recipe::{DevelopSettings, settings::GamutMapping},
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use pipeline_cpu::{Image, OutputContext, OutputTarget, RenderSource, render_managed_scaled};
use pipeline_gpu::{GpuContext, GpuManagedOutput};
use std::sync::Arc;

#[test]
fn managed_output_rejects_oversized_layout_without_panicking() {
    let device = Arc::new(GpuContext::new().unwrap());
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let output = GpuManagedOutput::new(
        device.clone(),
        &DevelopSettings::default(),
        &mut OutputContext {
            registry: &mut registry,
            target: OutputTarget::Display(&target),
            proof: None,
            options: Default::default(),
        },
    )
    .unwrap();
    let src = device.device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: 12,
        usage: wgpu::BufferUsages::STORAGE,
        mapped_at_creation: false,
    });
    let mut encoder = device.device.create_command_encoder(&Default::default());
    assert!(
        output
            .encode(
                &mut encoder,
                &src,
                TileLayout {
                    extent: Extent::new(u32::MAX, u32::MAX),
                    halo: 0,
                    channels: 3,
                }
            )
            .is_err()
    );
}

#[test]
fn managed_gpu_matches_cpu_tone_gamut_and_proof_policy() {
    let device = Arc::new(GpuContext::new().unwrap());
    let mut registry = Registry::new();
    let proof = registry.builtin(Builtin::Srgb).unwrap();
    let wrong = registry.builtin(Builtin::LinearRec2020).unwrap();
    let pixels = [
        [0.0; 3],
        [0.001; 3],
        [0.18; 3],
        [1.0; 3],
        [8.0; 3],
        [2.0, 0.05, 0.02],
        [0.01, 2.0, 0.03],
        [0.05, 0.02, 2.0],
        [0.3, 0.2, 0.1],
        [-0.01, 0.2, 0.4],
    ];
    let planes: Vec<Vec<f32>> = (0..3)
        .map(|c| pixels.iter().map(|p| p[c]).collect())
        .collect();
    let image = Image::new(pixels.len() as u32, 1, planes.clone()).unwrap();
    let linear = pipeline_cpu::render_linear_scaled(
        &DevelopSettings::default(),
        &RenderSource::Rgb(&image),
        1,
    )
    .unwrap();
    let tile = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(pixels.len() as u32, 1),
            halo: 0,
            channels: 3,
        },
        linear
            .planes()
            .iter()
            .flatten()
            .copied()
            .collect::<Vec<_>>(),
    )
    .unwrap();
    for builtin in [
        Builtin::Srgb,
        Builtin::DisplayP3,
        Builtin::AdobeRgb,
        Builtin::ProPhoto,
        Builtin::Rec2020,
    ] {
        let target = registry.builtin(builtin).unwrap();
        for proof_enabled in [false, true] {
            for gamut in [GamutMapping::Clip, GamutMapping::Perceptual] {
                let mut settings = DevelopSettings::default();
                settings.output.gamut_mapping = gamut;
                settings.output.proof_profile =
                    proof_enabled.then(|| IccProfileHandle::from_profile_bytes(proof.icc_bytes()));
                let mut context = OutputContext {
                    registry: &mut registry,
                    target: OutputTarget::Display(&target),
                    proof: proof_enabled.then_some(proof.as_ref()),
                    options: Default::default(),
                };
                let gpu = GpuManagedOutput::new(device.clone(), &settings, &mut context).unwrap();
                let expected =
                    render_managed_scaled(&settings, &RenderSource::Rgb(&image), 1, &mut context)
                        .unwrap();
                let actual = gpu.apply(&tile).unwrap();
                let samples = actual.pixels.samples::<f32>().unwrap();
                let greys = vec![0.0, 0.001, 0.01, 0.05, 0.18, 1.0, 8.0];
                let grey_tile = Tile::from_samples(
                    TileCoord::new(0, 0, 0),
                    TileLayout {
                        extent: Extent::new(7, 1),
                        halo: 0,
                        channels: 3,
                    },
                    [greys.clone(), greys.clone(), greys].concat(),
                )
                .unwrap();
                let grey_output = gpu.apply(&grey_tile).unwrap();
                assert!(
                    grey_output
                        .gamut_warnings
                        .iter()
                        .all(|w| !w.monitor && !w.proof),
                    "neutral warnings for {builtin:?}: {:?}",
                    grey_output.gamut_warnings
                );
                for (i, pixel) in expected.pixels.pixels().enumerate() {
                    for c in 0..3 {
                        assert!(
                            (samples[c * pixels.len() + i] - pixel[c]).abs() < 0.025,
                            "target={builtin:?} proof={proof_enabled} gamut={gamut:?} pixel={i} c={c} gpu={} cpu={}",
                            samples[c * pixels.len() + i],
                            pixel[c]
                        );
                    }
                }
                assert_eq!(
                    actual.gamut_warnings[5].monitor, expected.gamut_warnings[5].monitor,
                    "target={builtin:?} proof={proof_enabled} gamut={gamut:?} warning"
                );
                assert_eq!(actual.gamut_warnings[5].proof, proof_enabled);
                let mut mismatch = settings.clone();
                mismatch.output.proof_profile =
                    Some(IccProfileHandle::from_profile_bytes(wrong.icc_bytes()));
                assert!(gpu.validate(&mismatch).is_err());
            }
        }
    }
}
