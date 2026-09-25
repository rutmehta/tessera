//! Pixelwise display-referred sRGB fidelity, measured in CIEDE2000.
//!
//! Public sources:
//! - Sharma, Wu & Dalal (2005), implementation notes and supplementary test data:
//!   <https://hajim.rochester.edu/ece/sites/gsharma/ciede2000/>.
//! - sRGB transfer function, D65 XYZ matrix and CIELAB conversion:
//!   <http://www.brucelindbloom.com/index.html?Eqn_RGB_to_XYZ.html> and
//!   <http://www.brucelindbloom.com/index.html?Eqn_XYZ_to_Lab.html>.
//!
//! Images must already be encoded as sRGB; no ICC interpretation, registration,
//! resampling, alpha compositing or chromatic adaptation is performed.

/// Per-pixel Delta E 2000 summary (unit weighting factors).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FidelityStats {
    pub mean: f64,
    /// Nearest-rank 95th percentile: sorted sample at ceil(0.95 * N).
    pub p95: f64,
    pub samples: usize,
}

/// Compare equally sized, nonempty sRGB images without resizing or subsampling.
pub fn compare(
    render: &image::RgbImage,
    reference: &image::RgbImage,
) -> Result<FidelityStats, String> {
    if render.dimensions() != reference.dimensions() {
        return Err(format!(
            "image dimensions differ: render {:?}, reference {:?}",
            render.dimensions(),
            reference.dimensions()
        ));
    }
    if render.width() == 0 || render.height() == 0 {
        return Err("fidelity comparison requires nonempty images".into());
    }
    let mut differences: Vec<f64> = render
        .pixels()
        .zip(reference.pixels())
        .map(|(a, b)| delta_e_2000(srgb_to_lab(a.0), srgb_to_lab(b.0)))
        .collect();
    let samples = differences.len();
    let mean = differences.iter().sum::<f64>() / samples as f64;
    // ceil(19*N/20)-1 = N-floor(N/20)-1, without multiplication overflow.
    let rank = samples - samples / 20 - 1;
    let (_, p95, _) = differences.select_nth_unstable_by(rank, f64::total_cmp);
    Ok(FidelityStats {
        mean,
        p95: *p95,
        samples,
    })
}

fn srgb_to_lab(rgb: [u8; 3]) -> [f64; 3] {
    let [r, g, b] = rgb.map(|v| {
        let v = f64::from(v) / 255.0;
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    });
    // XYZ normalized to Y=1; CIE 1931 2-degree D65 reference white.
    let xyz = [
        (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047,
        0.2126729 * r + 0.7151522 * g + 0.0721750 * b,
        (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883,
    ];
    let [x, y, z] = xyz.map(|t| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    });
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

// Sharma et al., equations 2–22, k_L = k_C = k_H = 1.
// Private: inputs are finite Lab produced from bounded 8-bit sRGB.
fn delta_e_2000([l1, a1, b1]: [f64; 3], [l2, a2, b2]: [f64; 3]) -> f64 {
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
    let chroma_ratio = |c: f64| {
        let c7 = c.powi(7);
        (c7 / (c7 + 25.0_f64.powi(7))).sqrt()
    };
    let g = 0.5 * (1.0 - chroma_ratio(c_bar));
    let ap1 = (1.0 + g) * a1;
    let ap2 = (1.0 + g) * a2;
    let cp1 = ap1.hypot(b1);
    let cp2 = ap2.hypot(b2);
    let hue = |a: f64, b: f64| {
        if a == 0.0 && b == 0.0 {
            0.0
        } else {
            b.atan2(a).to_degrees().rem_euclid(360.0)
        }
    };
    let hp1 = hue(ap1, b1);
    let hp2 = hue(ap2, b2);
    let achromatic = cp1 == 0.0 || cp2 == 0.0;
    let mut dh = hp2 - hp1;
    if achromatic {
        dh = 0.0;
    } else if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    let delta_h = 2.0 * (cp1 * cp2).sqrt() * (dh / 2.0).to_radians().sin();
    let mean_l = (l1 + l2) / 2.0;
    let mean_c = (cp1 + cp2) / 2.0;
    let mean_h = if achromatic {
        hp1 + hp2
    } else if (hp1 - hp2).abs() <= 180.0 {
        (hp1 + hp2) / 2.0
    } else if hp1 + hp2 < 360.0 {
        (hp1 + hp2 + 360.0) / 2.0
    } else {
        (hp1 + hp2 - 360.0) / 2.0
    };
    let cos = |degrees: f64| degrees.to_radians().cos();
    let t =
        1.0 - 0.17 * cos(mean_h - 30.0) + 0.24 * cos(2.0 * mean_h) + 0.32 * cos(3.0 * mean_h + 6.0)
            - 0.20 * cos(4.0 * mean_h - 63.0);
    let l50_squared = (mean_l - 50.0).powi(2);
    let sl = 1.0 + 0.015 * l50_squared / (20.0 + l50_squared).sqrt();
    let sc = 1.0 + 0.045 * mean_c;
    let sh = 1.0 + 0.015 * mean_c * t;
    let theta = 30.0 * (-((mean_h - 275.0) / 25.0).powi(2)).exp();
    let rt = -2.0 * chroma_ratio(mean_c) * (2.0 * theta).to_radians().sin();
    let dl = (l2 - l1) / sl;
    let dc = (cp2 - cp1) / sc;
    let dh = delta_h / sh;
    // Protect the square root against a tiny negative roundoff at zero.
    (dl * dl + dc * dc + dh * dh + rt * dc * dh).max(0.0).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn published_sharma_vectors() {
        // Sharma et al. supplementary dataset: neutral and hue-wrap edge cases.
        let cases = [
            ([50.0, 2.6772, -79.7751], [50.0, 0.0, -82.7485], 2.0425),
            ([50.0, 3.1571, -77.2803], [50.0, 0.0, -82.7485], 2.8615),
            ([50.0, 2.8361, -74.0200], [50.0, 0.0, -82.7485], 3.4412),
            ([50.0, -1.3802, -84.2814], [50.0, 0.0, -82.7485], 1.0000),
            ([50.0, -1.1848, -84.8006], [50.0, 0.0, -82.7485], 1.0000),
            ([50.0, -0.9009, -85.5211], [50.0, 0.0, -82.7485], 1.0000),
            ([50.0, 0.0, 0.0], [50.0, -1.0, 2.0], 2.3669),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0009], 7.1792),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0010], 7.1792),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0011], 7.2195),
            ([50.0, 2.49, -0.001], [50.0, -2.49, 0.0012], 7.2195),
            ([50.0, -0.001, 2.49], [50.0, 0.0009, -2.49], 4.8045),
            ([50.0, -0.001, 2.49], [50.0, 0.0010, -2.49], 4.8045),
            ([50.0, -0.001, 2.49], [50.0, 0.0011, -2.49], 4.7461),
            ([50.0, 2.5, 0.0], [50.0, 0.0, -2.5], 4.3065),
            ([50.0, 2.5, 0.0], [73.0, 25.0, -18.0], 27.1492),
            (
                [2.0776, 0.0795, -1.1350],
                [0.9033, -0.0636, -0.5514],
                0.9082,
            ),
        ];
        for (index, (a, b, expected)) in cases.into_iter().enumerate() {
            for (left, right) in [(a, b), (b, a)] {
                let actual = delta_e_2000(left, right);
                assert!(
                    (actual - expected).abs() < 0.00005,
                    "case {index}: {actual} != {expected}"
                );
            }
            assert_eq!(delta_e_2000(a, a), 0.0);
        }
    }

    #[test]
    fn srgb_d65_lab_known_colors() {
        for (rgb, expected) in [
            ([0, 0, 0], [0.0, 0.0, 0.0]),
            ([255, 255, 255], [100.0, 0.0, 0.0]),
            ([255, 0, 0], [53.2408, 80.0925, 67.2032]),
            ([0, 255, 0], [87.7347, -86.1827, 83.1793]),
            ([0, 0, 255], [32.2970, 79.1875, -107.8602]),
            ([128, 128, 128], [53.5850, 0.0, 0.0]),
        ] {
            for (actual, expected) in srgb_to_lab(rgb).into_iter().zip(expected) {
                assert!(
                    (actual - expected).abs() < 0.0002,
                    "{rgb:?}: {actual} != {expected}"
                );
            }
        }
    }

    #[test]
    fn identical_native_render_synthetic_reference() {
        let planes = (0..3)
            .map(|c| (0..64).map(|i| ((i + c * 17) % 64) as f32 / 63.0).collect())
            .collect();
        let source = pipeline_cpu::Image::new(8, 8, planes).unwrap();
        let settings = engine_api::recipe::DevelopSettings::default();
        let rendered =
            pipeline_cpu::render(&settings, &pipeline_cpu::RenderSource::Rgb(&source)).unwrap();
        let reference = rendered.clone();
        let stats = compare(&rendered, &reference).unwrap();
        assert_eq!(
            stats,
            FidelityStats {
                mean: 0.0,
                p95: 0.0,
                samples: 64
            }
        );
    }

    #[test]
    fn rejects_empty_and_mismatched_dimensions() {
        for (a, b) in [
            ((0, 0), (0, 0)),
            ((0, 3), (0, 3)),
            ((3, 0), (3, 0)),
            ((2, 3), (3, 2)),
            ((1, 1), (2, 1)),
        ] {
            assert!(
                compare(
                    &image::RgbImage::new(a.0, a.1),
                    &image::RgbImage::new(b.0, b.1)
                )
                .is_err()
            );
        }
    }

    #[test]
    fn nearest_rank_and_mean_include_every_pixel() {
        let black = image::RgbImage::new(20, 1);
        let mut changed = black.clone();
        changed.put_pixel(19, 0, image::Rgb([255; 3]));
        let stats = compare(&changed, &black).unwrap();
        assert!((stats.mean - 5.0).abs() < 1e-5);
        assert_eq!(stats.p95, 0.0);
        changed.put_pixel(18, 0, image::Rgb([255; 3]));
        let stats = compare(&changed, &black).unwrap();
        assert!((stats.mean - 10.0).abs() < 1e-5);
        assert!((stats.p95 - 100.0).abs() < 1e-5);
        let single = compare(
            &image::RgbImage::from_pixel(1, 1, image::Rgb([255; 3])),
            &image::RgbImage::new(1, 1),
        )
        .unwrap();
        assert_eq!(single.mean, single.p95);
    }

    #[test]
    fn identical_rgb_is_zero() {
        let image = image::RgbImage::from_fn(8, 4, |x, y| {
            image::Rgb([(x * 31) as u8, (y * 63) as u8, ((x + y) * 23) as u8])
        });
        assert_eq!(
            compare(&image, &image).unwrap(),
            FidelityStats {
                mean: 0.0,
                p95: 0.0,
                samples: 32
            }
        );
    }
}
