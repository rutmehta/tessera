use engine_api::{recipe::DevelopSettings, tile::TileCoord};
use pipeline_cpu::{Image, RenderSource, detail, detail_halo, render_linear_scaled};

#[test]
fn default_detail_is_active_continuous_and_applied_once() {
    let input = Image::new(
        9,
        9,
        (0..3)
            .map(|c| {
                (0..81)
                    .map(|i| 0.3 + if (i + c) % 2 == 0 { 0.02 } else { -0.02 })
                    .collect()
            })
            .collect(),
    )
    .unwrap();
    let settings = DevelopSettings::default();
    assert_eq!(settings.detail.sharpening.amount, 40.0);
    assert_eq!(settings.detail.noise_reduction.color, 25.0);
    let run = |settings: &DevelopSettings| {
        let mut tile = input
            .tile(TileCoord::new(0, 0, 0), detail_halo(&settings.detail), 1)
            .unwrap();
        detail(&mut tile, &settings.detail).unwrap();
        let mut output = input.clone();
        output.put(&tile).unwrap();
        output
    };
    let baseline = run(&settings);
    for control in [0, 1] {
        let mut off = settings.clone();
        if control == 0 {
            off.detail.sharpening.amount = 0.0;
        } else {
            off.detail.noise_reduction.color = 0.0;
        }
        let difference = baseline
            .planes()
            .iter()
            .flatten()
            .zip(run(&off).planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            difference > 1e-5,
            "operator {control} must contribute at default"
        );
        let mut nearby = settings.clone();
        if control == 0 {
            nearby.detail.sharpening.amount += 0.001;
        } else {
            nearby.detail.noise_reduction.color += 0.001;
        }
        let difference = baseline
            .planes()
            .iter()
            .flatten()
            .zip(run(&nearby).planes().iter().flatten())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0f32, f32::max);
        assert!(
            difference < 1e-5,
            "operator {control} must not jump at default"
        );
    }
    let rendered = render_linear_scaled(&settings, &RenderSource::Rgb(&input), 1).unwrap();
    for (a, b) in rendered
        .planes()
        .iter()
        .flatten()
        .zip(baseline.planes().iter().flatten())
    {
        assert!((a - b).abs() < 1e-6);
    }
}
