//! Behaviour contracts, not evidence of Lightroom visual equivalence.
use engine_api::recipe::DevelopSettings;
use pipeline_adobe::{Image, RenderSource, render_linear_scaled};

fn settings() -> DevelopSettings {
    let mut s = DevelopSettings::default();
    s.detail.sharpening.amount = 0.;
    s.detail.noise_reduction.color = 0.;
    s
}
fn patch() -> Image {
    Image::new(
        48,
        48,
        (0..3)
            .map(|c| {
                (0..48 * 48)
                    .map(|i| {
                        let (x, y) = (i % 48, i / 48);
                        0.08 + (x as f32 / 48.) * 0.2
                            + ((x * 7 + y * 11 + c * 13) % 17) as f32 * 0.004
                    })
                    .collect()
            })
            .collect(),
    )
    .unwrap()
}
fn run(s: &DevelopSettings, input: &Image) -> Image {
    render_linear_scaled(s, &RenderSource::Rgb(input), 1).unwrap()
}
fn distance(a: &Image, b: &Image) -> f32 {
    a.planes()
        .iter()
        .flatten()
        .zip(b.planes().iter().flatten())
        .map(|(x, y)| (x - y).abs())
        .sum()
}

#[test]
fn all_native_approximation_parameters_reach_compat_render() {
    let input = patch();
    let neutral = settings();
    let base = run(&neutral, &input);
    // Native approximations remain live, rather than silently dropping imported controls.
    for i in 0..14 {
        let mut previous = 0.;
        for amount in [25., 50., 75.] {
            let mut s = neutral.clone();
            match i {
                0 => s.tone.texture = amount,
                1 => s.tone.clarity = amount,
                2 => s.tone.dehaze = -amount,
                3 => s.color.vibrance = amount,
                4 => s.color.saturation = -amount,
                5 => s.color.hsl.hue.orange = amount,
                6 => s.color.hsl.saturation.orange = amount,
                7 => s.color.hsl.luminance.orange = amount,
                8 => {
                    s.color.grading.shadows.hue = 220.;
                    s.color.grading.shadows.saturation = amount;
                }
                9 => s.detail.sharpening.amount = amount,
                10 => s.detail.noise_reduction.luminance = amount,
                11 => s.detail.noise_reduction.color = amount,
                12 => s.effects.vignette.amount = -amount,
                _ => s.effects.grain.amount = amount,
            }
            let delta = distance(&base, &run(&s, &input));
            assert!(
                delta > previous + 0.001,
                "operator {i}, amount {amount}: {delta} <= {previous}"
            );
            previous = delta;
        }
    }
}

#[test]
fn crop_and_angle_apply_before_downsampling() {
    let input = patch();
    let mut s = settings();
    s.geometry.crop.rect.right = 0.5;
    s.geometry.crop.rect.bottom = 0.5;
    let first = run(&s, &input);
    assert_eq!((first.width(), first.height()), (24, 24));
    s.geometry.crop.angle = 10.;
    let rotated = run(&s, &input);
    assert!(distance(&first, &rotated) > 0.01);
    let small = render_linear_scaled(&s, &RenderSource::Rgb(&input), 4).unwrap();
    let expected = rotated.downsample_crop([0, 0, 24, 24], 4).unwrap();
    assert_eq!(small.planes(), expected.planes());
}
