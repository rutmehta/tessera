use color_mgmt::{Builtin, Registry, Transform, TransformOptions};
use engine_api::{
    color::IccProfileHandle,
    recipe::{DevelopSettings, settings::GamutMapping},
    tile::{Extent, Tile, TileCoord, TileLayout},
};
use pipeline_cpu::{OutputContext, OutputTarget};
use pipeline_gpu::{GpuContext, GpuManagedOutput};
use std::sync::Arc;

#[test]
fn synthetic_printer_gpu_warns_and_simulates_paper_against_cpu() {
    use lcms2::{
        CIEXYZ, CIExyY, CIExyYTRIPLE, ProfileClassSignature, Tag, TagSignature, ToneCurve,
    };
    let xy = |x, y| CIExyY { x, y, Y: 1.0 };
    let curve = ToneCurve::new(2.2);
    let mut printer = lcms2::Profile::new_rgb(
        &xy(0.3457, 0.3585),
        &CIExyYTRIPLE {
            Red: xy(0.48, 0.34),
            Green: xy(0.30, 0.48),
            Blue: xy(0.23, 0.20),
        },
        &[&curve, &curve, &curve],
    )
    .unwrap();
    printer.set_version(2.4);
    printer.set_device_class(ProfileClassSignature::OutputClass);
    assert!(printer.write_tag(
        TagSignature::MediaWhitePointTag,
        Tag::CIEXYZ(&CIEXYZ {
            X: 0.80,
            Y: 0.85,
            Z: 0.55
        })
    ));
    let device = Arc::new(GpuContext::new().unwrap());
    let mut registry = Registry::new();
    let source = registry.builtin(Builtin::LinearRec2020).unwrap();
    let target = registry.builtin(Builtin::DisplayP3).unwrap();
    let proof = registry.load_bytes(&printer.icc().unwrap()).unwrap();
    let mut settings = DevelopSettings::default();
    settings.output.proof_profile = Some(IccProfileHandle::from_profile_bytes(proof.icc_bytes()));
    settings.output.gamut_mapping = GamutMapping::Clip;
    let samples = vec![1.0, 8.0, 0.0, 8.0, 0.0, 8.0]; // red, white, planar
    let input = Tile::from_samples(
        TileCoord::new(0, 0, 0),
        TileLayout {
            extent: Extent::new(2, 1),
            channels: 3,
            halo: 0,
        },
        samples,
    )
    .unwrap();
    let mut whites = Vec::new();
    for simulate_paper in [false, true] {
        let options = TransformOptions {
            simulate_paper,
            ..Default::default()
        };
        let gpu = GpuManagedOutput::new(
            device.clone(),
            &settings,
            &mut OutputContext {
                registry: &mut registry,
                target: OutputTarget::Display(&target),
                proof: Some(&proof),
                options,
            },
        )
        .unwrap();
        let actual = gpu.apply(&input).unwrap();
        assert!(actual.gamut_warnings[0].monitor && actual.gamut_warnings[0].proof);
        let values = actual.pixels.samples::<f32>().unwrap();
        assert!(values.iter().all(|v| v.is_finite()));
        let cpu = Transform::proof(&source, &target, &proof, options).unwrap();
        let white = cpu.apply([pipeline_cpu::sigmoid(8.0, Default::default()); 3]);
        for c in 0..3 {
            assert!((values[c * 2 + 1] - white[c]).abs() < 0.025);
        }
        whites.push(values[5]);
    }
    assert!(whites[1] < whites[0] - 0.05, "{whites:?}");
}
