use engine_api::recipe::{
    DevelopSettings,
    mask::{LocalAdjustment, LocalParams, MaskComponent, MaskKind},
};
use pipeline_cpu::{Image, RenderSource, render_linear_scaled};

fn group() -> LocalAdjustment {
    LocalAdjustment {
        components: vec![MaskComponent::new(MaskKind::Radial {
            center: [0.25, 0.5],
            radii: [0.24, 1.0],
            angle: 0.0,
            feather: 0.0,
        })],
        params: LocalParams {
            exposure: 1.0,
            ..Default::default()
        },
        ..Default::default()
    }
}
#[test]
fn local_exposure_only_doubles_selected_linear_pixels() {
    let input = Image::new(4, 1, vec![vec![0.125; 4]; 3]).unwrap();
    let mut settings = DevelopSettings::default();
    settings.detail.sharpening.amount = 0.0;
    settings.detail.noise_reduction.color = 0.0;
    settings.locals.adjustments.push(group());
    let out = render_linear_scaled(&settings, &RenderSource::Rgb(&input), 1).unwrap();
    assert_eq!(out.planes(), &vec![vec![0.25, 0.25, 0.125, 0.125]; 3]);
}

#[test]
fn every_local_slider_changes_a_textured_colour_image() {
    let input = Image::new(
        17,
        13,
        (0..3)
            .map(|c| {
                (0..221)
                    .map(|i| 0.05 + ((i * 17 + c * 31) % 91) as f32 / 120.0)
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let sliders: [fn(&mut LocalParams); 16] = [
        |p| p.contrast = 60.,
        |p| p.highlights = 60.,
        |p| p.shadows = 60.,
        |p| p.whites = 60.,
        |p| p.blacks = 60.,
        |p| p.temperature = 60.,
        |p| p.tint = 60.,
        |p| p.texture = 60.,
        |p| p.clarity = 60.,
        |p| p.dehaze = 60.,
        |p| p.saturation = 60.,
        |p| p.sharpness = 60.,
        |p| p.noise = 60.,
        |p| p.hue = 60.,
        |p| p.sharpness = -60.,
        |p| p.noise = -60.,
    ];
    for (index, set) in sliders.into_iter().enumerate() {
        let mut params = LocalParams::default();
        set(&mut params);
        let out = pipeline_cpu::adjust_local(&input, &params, 100.).unwrap();
        assert!(
            out.planes()
                .iter()
                .flatten()
                .zip(input.planes().iter().flatten())
                .any(|(a, b)| (a - b).abs() > 1e-5),
            "slider {index}"
        );
    }
}
