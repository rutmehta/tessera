use ml_enhance::{CfaNoise, DENOISE_AUTO_MAX_SIGMA, estimate_drunet_sigma};

fn noisy_flat(level: f32, amplitude: f32) -> Tensor {
    let mut seed = 17u32;
    let data = (0..3 * 64 * 64)
        .map(|_| {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            level + amplitude * (2.0 * (seed >> 8) as f32 / 16777216.0 - 1.0)
        })
        .collect();
    Tensor::new(3, 64, 64, data).unwrap()
}

#[test]
fn automatic_sigma_propagates_linear_variance_through_srgb_derivative() {
    for level in [0.001, 0.25] {
        let rgb = noisy_flat(level, 0.0001);
        let noise = CfaNoise::estimate_rgb(&rgb).unwrap();
        let mut display_variance = 0.0f64;
        for (c, plane) in rgb.data().as_chunks::<{ 64 * 64 }>().0.iter().enumerate() {
            let mean = plane.iter().map(|&v| v as f64).sum::<f64>() / plane.len() as f64;
            let derivative = if mean <= 0.0031308 {
                12.92
            } else {
                (1.055 / 2.4) * mean.powf(1.0 / 2.4 - 1.0)
            };
            display_variance +=
                derivative * derivative * (noise.read[c] as f64 + noise.shot[c] as f64 * mean);
        }
        let expected = (display_variance / 3.0).sqrt() as f32;
        assert!((estimate_drunet_sigma(&rgb).unwrap() - expected).abs() < 1e-7);
    }
    let low = estimate_drunet_sigma(&noisy_flat(0.25, 0.001)).unwrap();
    let high = estimate_drunet_sigma(&noisy_flat(0.25, 0.01)).unwrap();
    assert!((high / low - 10.0).abs() < 0.05);
    let extreme = Tensor::new(
        3,
        64,
        64,
        (0..3 * 64 * 64)
            .map(|i| ((i / 64 + i % 64) % 2) as f32)
            .collect(),
    )
    .unwrap();
    assert_eq!(
        estimate_drunet_sigma(&extreme).unwrap(),
        DENOISE_AUTO_MAX_SIGMA
    );
    assert_eq!(estimate_drunet_sigma(&noisy_flat(0.25, 0.0)).unwrap(), 0.0);
}
use ml_runtime::Tensor;

#[test]
fn automatic_amount_and_mask_only_blend_never_change_sigma() {
    let input = noisy_flat(0.25, 0.01);
    let sigma = estimate_drunet_sigma(&input).unwrap();
    let mut mask = vec![0.5; 64 * 64];
    mask[0] = 0.0;
    for amount in [25.0, 100.0] {
        let output =
            ml_enhance::denoise_automatic_with(&input, amount, Some(&mask), |rgb, actual| {
                assert_eq!(actual, sigma);
                Tensor::new(3, 64, 64, vec![0.125; rgb.data().len()])
            })
            .unwrap();
        for (i, (&src, &dst)) in input.data().iter().zip(output.data()).enumerate() {
            let alpha = amount / 100.0 * mask[i % (64 * 64)];
            if alpha == 0.0 {
                assert_eq!(src.to_bits(), dst.to_bits());
            } else {
                assert_eq!(dst, src * (1.0 - alpha) + 0.125 * alpha);
            }
        }
    }
}

#[test]
fn automatic_bypasses_zero_noise_and_zero_selection_without_inference() {
    let flat = noisy_flat(0.25, 0.0);
    let out = ml_enhance::denoise_automatic_with(&flat, 100.0, None, |_, _| {
        panic!("noiseless input must bypass inference")
    })
    .unwrap();
    assert_eq!(out.data(), flat.data());
    let tiny = Tensor::new(3, 1, 2, vec![-0.0, -1.0, 2.0, 0.0, 0.5, 1.0]).unwrap();
    for (amount, mask) in [(0.0, None), (100.0, Some(&[0.0, 0.0][..]))] {
        let out = ml_enhance::denoise_automatic_with(&tiny, amount, mask, |_, _| panic!("bypass"))
            .unwrap();
        assert_eq!(
            out.data().iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
            tiny.data().iter().map(|v| v.to_bits()).collect::<Vec<_>>()
        );
    }
}

#[test]
fn automatic_rejects_invalid_inputs_before_inference() {
    for input in [
        Tensor::new(3, 4, 4, vec![0.25; 48]).unwrap(),
        Tensor::new(4, 8, 8, vec![0.25; 256]).unwrap(),
        Tensor::new(3, 8, 8, vec![f32::NAN; 192]).unwrap(),
        Tensor::new(3, 8, 8, vec![1.01; 192]).unwrap(),
        Tensor::new(3, 8, 8, vec![-0.01; 192]).unwrap(),
    ] {
        assert!(
            ml_enhance::denoise_automatic_with(&input, 100.0, None, |_, _| panic!("invalid"))
                .is_err()
        );
    }
    let input = noisy_flat(0.25, 0.01);
    for amount in [f32::NAN, -1.0, 101.0] {
        assert!(
            ml_enhance::denoise_automatic_with(&input, amount, None, |_, _| panic!("invalid"))
                .is_err()
        );
    }
    assert!(
        ml_enhance::denoise_automatic_with(&input, 100.0, Some(&[1.0]), |_, _| panic!("invalid"))
            .is_err()
    );
}

#[test]
fn rgb_estimate_reuses_cfa_estimator_without_treating_blue_as_green() {
    let (h, w) = (64, 64);
    let mut seed = 7u32;
    let mut rgb = Vec::new();
    for c in 0..3 {
        for _ in 0..h * w {
            seed = seed.wrapping_mul(1664525).wrapping_add(1013904223);
            rgb.push(0.25 + 0.002 * (c + 1) as f32 * (2.0 * (seed >> 8) as f32 / 16777216.0 - 1.0));
        }
    }
    let input = Tensor::new(3, h, w, rgb.clone()).unwrap();
    let estimated = CfaNoise::estimate_rgb(&input).unwrap();
    // Same independent plane estimator as RGGB; duplicated G is only a
    // reference layout here, not a reinterpretation of RGB as sensor samples.
    let packed = [
        &rgb[..h * w],
        &rgb[h * w..2 * h * w],
        &rgb[h * w..2 * h * w],
        &rgb[2 * h * w..],
    ]
    .concat();
    let reference = CfaNoise::estimate(&Tensor::new(4, h, w, packed).unwrap()).unwrap();
    for (rgb_channel, cfa_channel) in [0, 1, 3].into_iter().enumerate() {
        assert_eq!(estimated.read[rgb_channel], reference.read[cfa_channel]);
        assert_eq!(estimated.shot[rgb_channel], reference.shot[cfa_channel]);
        let expected = (0.002 * (rgb_channel + 1) as f32).powi(2) / 3.0;
        assert!((estimated.read[rgb_channel] / expected - 1.0).abs() < 0.3);
    }
}
