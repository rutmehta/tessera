mod common;
use engine_api::recipe::DevelopSettings;
use image_core::{Headroom, PixelRect, RenderOutput, Renderer, RendererConfig};

#[test]
fn composed_optics_edr_matches_f32_scalar_reference() {
    let raw = common::synthetic(2203, 257, 190, common::RGGB, [0, 0, 257, 190]);
    let mut settings = DevelopSettings::default();
    settings.tone.exposure = 2.;
    // Calibrated distortion is the explicit opt-in since ENG-7 (Auto never
    // estimates from image content).
    settings.lens.profile = engine_api::recipe::settings::LensProfileSource::AutoCalibrated;
    let lens = pipeline_cpu::resolve_lens_sensor(
        raw.cfa().pyramid().pixels(),
        raw.metadata(),
        &settings,
        &Default::default(),
    )
    .unwrap();
    assert!(
        lens.plan(&settings, raw.metadata())
            .unwrap()
            .unwrap()
            .map
            .is_some(),
        "fixture must exercise calibrated distortion"
    );
    let expected = pipeline_cpu::render_linear_scaled_with_lens(
        &settings,
        &pipeline_cpu::RenderSource::Cfa {
            image: raw.cfa(),
            metadata: raw.metadata(),
        },
        1,
        &Default::default(),
    )
    .unwrap();
    let renderer = Renderer::new(RendererConfig::default());
    for headroom in [1., 2.5, 16.] {
        let actual = renderer
            .render_region_as(
                &raw,
                &settings,
                0,
                PixelRect::full(raw.active_extent()),
                RenderOutput::DisplayLinear(Headroom::new(headroom)),
            )
            .unwrap();
        let mut max = 0f32;
        for tile in actual {
            let reference = pipeline_cpu::display_linear(
                &expected.tile(tile.coord(), 0, 1).unwrap(),
                settings.output.gamut_mapping,
                headroom,
            )
            .unwrap();
            for (&a, &b) in tile
                .samples::<f32>()
                .unwrap()
                .iter()
                .zip(reference.samples::<f32>().unwrap())
            {
                max = max.max((a - b).abs() / b.max(0.05));
            }
        }
        eprintln!("HDR scalar reference headroom={headroom}, max relative error={max}");
        assert!(max < 5e-3, "headroom {headroom}: {max}");
    }
}
