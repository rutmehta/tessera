use engine_api::color::WhitePoint;
use pipeline_cpu::temperature_white;

fn uv(w: WhitePoint) -> [f64; 2] {
    let d = -2.0 * w.x + 12.0 * w.y + 3.0;
    [4.0 * w.x / d, 6.0 * w.y / d]
}

#[test]
fn fixture_as_shot_roundtrip_and_slider_directions() {
    use engine_api::{
        color::ColorMatrix3,
        recipe::settings::{WhiteBalanceMode, WhiteBalanceSettings},
    };
    use pipeline_cpu::{as_shot_temperature_tint, camera_to_xyz, white_balance_matrix};
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/raw");
    let mut failures = Vec::new();
    for name in [
        "canon-cr3.CR3",
        "nikon-nef.NEF",
        "sony-arw.ARW",
        "fuji-raf.RAF",
        "sample.dng",
    ] {
        let mut source = raw_decode::RawSource::open(root.join(name)).unwrap();
        source.decode_cfa().unwrap();
        let m = source.metadata();

        let camera = camera_to_xyz(ColorMatrix3(std::array::from_fn(|r| {
            m.cam_xyz[r].map(f64::from)
        })))
        .unwrap();

        let (temperature, tint) = match as_shot_temperature_tint(camera, m.as_shot_wb) {
            Ok(pair) => pair,
            Err(error) => {
                failures.push(format!("{name}: {error}"));
                continue;
            }
        };
        eprintln!("{name}: {temperature} K, tint {tint}");
        let as_shot =
            white_balance_matrix(&WhiteBalanceSettings::default(), camera, m.as_shot_wb).unwrap();
        let custom = |dt, di| {
            white_balance_matrix(
                &WhiteBalanceSettings {
                    mode: WhiteBalanceMode::Custom,
                    temperature: temperature + dt,
                    tint: tint + di,
                },
                camera,
                m.as_shot_wb,
            )
            .unwrap()
        };
        let identity = custom(0.0, 0.0) * as_shot.inverse().unwrap();
        for r in 0..3 {
            for c in 0..3 {
                assert!(
                    (identity.0[r][c] - f64::from(r == c)).abs() < 1e-4,
                    "{name}: {identity:?}"
                );
            }
        }
        let sample = as_shot.inverse().unwrap().apply([0.18; 3]);
        for (dt, di) in [(0.01, 0.0), (-0.01, 0.0), (0.0, 0.001), (0.0, -0.001)] {
            let nearby = custom(dt, di).apply(sample);
            assert!(
                nearby.iter().all(|v| (v - 0.18).abs() < 1e-4),
                "{name}: first-touch discontinuity {nearby:?}"
            );
        }
        let mut last = 0.0;
        for step in 0..=10 {
            let rgb = custom(step as f32 * 50.0, 0.0).apply(sample);
            let ratio = rgb[0] / rgb[2];
            assert!(ratio > last, "{name}: warming must be monotonic");
            last = ratio;
        }
        let a = custom(0.0, 0.0).apply(sample);
        let b = custom(0.0, 50.0).apply(sample);
        assert!(
            b[0] / b[1] > a[0] / a[1] && b[2] / b[1] > a[2] / a[1],
            "{name}: magenta {a:?} -> {b:?}"
        );
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn camera_calibration_preserves_known_illuminant_chromaticity() {
    use engine_api::color::ColorMatrix3;
    use pipeline_cpu::{as_shot_temperature_tint, camera_to_xyz};
    let calibration = ColorMatrix3([[0.7, 0.2, 0.1], [0.1, 0.8, 0.1], [0.05, 0.1, 0.85]]);
    let camera = camera_to_xyz(calibration).unwrap();
    for temperature in [2000.0, 4500.0, 6500.0, 12000.0, 24000.0] {
        // Keep generated illuminants physical even at 2000 K. The full
        // mathematical slider domain is tested separately below.
        for tint in [-10.0, 0.0, 10.0] {
            let white = temperature_white(temperature, tint).unwrap();
            let response = calibration.apply(white.to_xyz());
            let multipliers = [
                (1.0 / response[0]) as f32,
                (1.0 / response[1]) as f32,
                (1.0 / response[2]) as f32,
                1.0,
            ];
            let (t, i) = as_shot_temperature_tint(camera, multipliers).unwrap();
            assert!((t - temperature).abs() < 0.1, "{temperature}, {t}");
            assert!((i - tint).abs() < 0.001, "{tint}, {i}");
        }
    }
}

#[test]
fn tint_is_perpendicular_duv_with_full_slider_range() {
    for t in [2000.0, 4000.0, 6500.0, 12000.0, 24000.0] {
        let base = uv(temperature_white(t, 0.0).unwrap());
        let before = uv(temperature_white(t - 1.0, 0.0).unwrap());
        let after = uv(temperature_white(t + 1.0, 0.0).unwrap());
        for tint in [-150.0, 50.0, 150.0] {
            let shifted = uv(temperature_white(t, tint).unwrap());
            let delta = [shifted[0] - base[0], shifted[1] - base[1]];
            assert!((delta[0].hypot(delta[1]) - f64::from(tint).abs() / 3000.0).abs() < 1e-8);
            let tangent = [after[0] - before[0], after[1] - before[1]];
            assert!(
                (delta[0] * tangent[0] + delta[1] * tangent[1]).abs()
                    / tangent[0].hypot(tangent[1])
                    < 1e-6
            );
        }
    }
}
