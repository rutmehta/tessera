use super::*;
#[test]
fn output_sharpening_chart_strength_density_and_determinism() {
    let token = CancellationToken::new();
    let chart = Rgb32FImage::from_fn(65, 41, |x, y| {
        image::Rgb([if x > 32 + y / 8 { 0.6 } else { 0.4 }; 3])
    });
    for medium in [SharpenFor::Screen, SharpenFor::Matte, SharpenFor::Glossy] {
        let mut energies = Vec::new();
        for strength in [
            SharpenAmount::Low,
            SharpenAmount::Standard,
            SharpenAmount::High,
        ] {
            let run = |ppi| sharpen_output(chart.clone(), medium, strength, ppi, &token).unwrap();
            let out = run(300);
            assert_eq!(out, run(300));
            energies.push(
                out.as_raw()
                    .iter()
                    .zip(chart.as_raw())
                    .map(|(a, b)| (a - b).abs())
                    .sum::<f32>(),
            );
            if matches!(medium, SharpenFor::Screen) {
                assert_eq!(out, run(150));
            } else {
                assert_ne!(out, run(150));
            }
            let flat = Rgb32FImage::from_pixel(17, 13, image::Rgb([0.4; 3]));
            let flat = sharpen_output(flat, medium, strength, 300, &token).unwrap();
            assert!(flat.as_raw().iter().all(|v| (v - 0.4).abs() < 1e-6));
        }
        assert!(energies[0] > 0.0 && energies[0] < energies[1] && energies[1] < energies[2]);
    }
    for ppi in [0, 9601] {
        assert!(
            sharpen_output(
                chart.clone(),
                SharpenFor::Matte,
                SharpenAmount::Standard,
                ppi,
                &token
            )
            .is_err()
        );
    }
    token.cancel();
    assert!(
        sharpen_output(
            chart,
            SharpenFor::Matte,
            SharpenAmount::Standard,
            300,
            &token
        )
        .is_err()
    );
}

#[test]
fn lanczos_preserves_constants_and_suppresses_aliasing() {
    let token = CancellationToken::new();
    let flat = Rgb32FImage::from_pixel(63, 47, image::Rgb([0.3, 0.5, 0.8]));
    let out = resize(flat, Resize::Fit(17, 11), &token).unwrap();
    for p in out.pixels() {
        for (a, b) in p.0.into_iter().zip([0.3, 0.5, 0.8]) {
            assert!((a - b).abs() < 1e-5);
        }
    }
    let checker = Rgb32FImage::from_fn(128, 128, |x, y| image::Rgb([((x + y) % 2) as f32; 3]));
    let out = resize(checker, Resize::LongEdge(16), &token).unwrap();
    for y in 2..14 {
        for x in 2..14 {
            assert!((out.get_pixel(x, y)[0] - 0.5).abs() < 0.005);
        }
    }
}
#[test]
fn lanczos_matches_independent_reference() {
    let src = Rgb32FImage::from_fn(73, 41, |x, y| {
        image::Rgb([((x * 17 + y * 3) % 101) as f32 / 100.0; 3])
    });
    let expected = image::imageops::resize(&src, 23, 13, image::imageops::FilterType::Lanczos3);
    let actual = resize(src, Resize::LongEdge(23), &CancellationToken::new()).unwrap();
    assert_eq!(actual.dimensions(), expected.dimensions());
    // Both are separable, but pass ordering and edge extension differ.
    for y in 3..10 {
        for x in 3..20 {
            assert!((actual.get_pixel(x, y)[0] - expected.get_pixel(x, y)[0]).abs() < 0.002);
        }
    }
}
