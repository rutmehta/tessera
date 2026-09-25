use color_mgmt::{Builtin, Registry, Transform, TransformOptions};
use engine_api::recipe::DevelopSettings;
use pipeline_cpu::{Image, OutputContext, OutputTarget, RenderSource, render_managed_scaled};

#[test]
fn managed_render_uses_selected_target_without_srgb_intermediate() {
    let mut registry = Registry::new();
    let working = registry.builtin(Builtin::LinearRec2020).unwrap();
    let target = registry.builtin(Builtin::DisplayP3).unwrap();
    let input = Image::new(
        259,
        2,
        vec![vec![0.25; 518], vec![0.15; 518], vec![0.10; 518]],
    )
    .unwrap();
    let settings = DevelopSettings::default();
    let source = RenderSource::Rgb(&input);
    let linear = pipeline_cpu::render_linear_scaled(&settings, &source, 2).unwrap();
    let transform = Transform::new(&working, &target, TransformOptions::default()).unwrap();
    let output = render_managed_scaled(
        &settings,
        &source,
        2,
        &mut OutputContext {
            registry: &mut registry,
            target: OutputTarget::Display(&target),
            proof: None,
            options: TransformOptions::default(),
        },
    )
    .unwrap();
    assert_eq!(output.pixels.dimensions(), (130, 1));
    assert_eq!(output.gamut_warnings.len(), 130);
    for (i, pixel) in output.pixels.pixels().enumerate() {
        let rgb = std::array::from_fn(|c| linear.planes()[c][i]);
        let y = 0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2];
        let toned = rgb.map(|v| v * pipeline_cpu::sigmoid(y, Default::default()) / y);
        let expected = transform.apply(toned);
        for c in 0..3 {
            assert!((pixel[c] - expected[c]).abs() < 1e-6);
        }
        assert_eq!(output.gamut_warnings[i], transform.gamut_warning(toned));
    }
}

#[test]
fn proof_handle_is_checked_and_shared_proof_transform_is_used() {
    use engine_api::color::IccProfileHandle;
    let mut registry = Registry::new();
    let working = registry.builtin(Builtin::LinearRec2020).unwrap();
    let display = registry.builtin(Builtin::DisplayP3).unwrap();
    let proof = registry.builtin(Builtin::Srgb).unwrap();
    let input = Image::new(1, 1, vec![vec![0.18]; 3]).unwrap();
    let source = RenderSource::Rgb(&input);
    let mut settings = DevelopSettings::default();
    settings.output.proof_profile = Some(IccProfileHandle::from_profile_bytes(proof.icc_bytes()));
    let options = TransformOptions {
        simulate_paper: true,
        ..Default::default()
    };
    let mut context = OutputContext {
        registry: &mut registry,
        target: OutputTarget::Display(&display),
        proof: Some(&proof),
        options,
    };
    let output = render_managed_scaled(&settings, &source, 1, &mut context).unwrap();
    let expected = Transform::proof(&working, &display, &proof, options).unwrap();
    for c in 0..3 {
        assert!((output.pixels.get_pixel(0, 0)[c] - expected.apply([0.18; 3])[c]).abs() < 1e-5);
    }
    context.proof = None;
    assert!(render_managed_scaled(&settings, &source, 1, &mut context).is_err());
    context.proof = Some(&display);
    assert!(render_managed_scaled(&settings, &source, 1, &mut context).is_err());
    context.proof = Some(&proof);
    context.target = OutputTarget::Export(&display);
    assert!(render_managed_scaled(&settings, &source, 1, &mut context).is_err());
    settings.output.proof_profile = None;
    context.proof = None;
    assert!(render_managed_scaled(&settings, &source, 1, &mut context).is_ok());
    settings.output.hdr = true;
    assert!(render_managed_scaled(&settings, &source, 1, &mut context).is_err());
}

#[test]
fn output_gamut_mapping_bounds_saturated_pixels_and_keeps_warnings() {
    use engine_api::recipe::settings::GamutMapping;
    let mut registry = Registry::new();
    let target = registry.builtin(Builtin::Srgb).unwrap();
    let input = Image::new(1, 1, vec![vec![2.0], vec![0.05], vec![0.02]]).unwrap();
    let source = RenderSource::Rgb(&input);
    let mut settings = DevelopSettings::default();
    let mut context = OutputContext {
        registry: &mut registry,
        target: OutputTarget::Export(&target),
        proof: None,
        options: Default::default(),
    };
    let compressed = render_managed_scaled(&settings, &source, 1, &mut context).unwrap();
    assert!(
        compressed
            .pixels
            .as_raw()
            .iter()
            .all(|v| (0.0..=1.0).contains(v))
    );
    settings.output.gamut_mapping = GamutMapping::Clip;
    let clipped = render_managed_scaled(&settings, &source, 1, &mut context).unwrap();
    assert!(
        clipped
            .pixels
            .as_raw()
            .iter()
            .all(|v| (0.0..=1.0).contains(v))
    );
    assert_ne!(compressed.pixels, clipped.pixels);
    assert_eq!(compressed.gamut_warnings, clipped.gamut_warnings);
    assert!(clipped.gamut_warnings[0].monitor);
}
