//! Dependency-free color difference helpers for GPU regression tests.

/// CIEDE2000 for CIELab values, with unit parametric weights.
pub fn delta_e_2000(a: [f64; 3], b: [f64; 3]) -> f64 {
    let [l1, a1, b1] = a;
    let [l2, a2, b2] = b;
    let c_bar = (a1.hypot(b1) + a2.hypot(b2)) / 2.0;
    let chroma_ratio = |c: f64| c.powi(7) / (c.powi(7) + 25_f64.powi(7));
    let g = 0.5 * (1.0 - chroma_ratio(c_bar).sqrt());
    let ap1 = (1.0 + g) * a1;
    let ap2 = (1.0 + g) * a2;
    let cp1 = ap1.hypot(b1);
    let cp2 = ap2.hypot(b2);
    let hue = |ap: f64, bv: f64, cp: f64| {
        if cp == 0.0 {
            0.0
        } else {
            bv.atan2(ap).to_degrees().rem_euclid(360.0)
        }
    };
    let h1 = hue(ap1, b1, cp1);
    let h2 = hue(ap2, b2, cp2);
    let achromatic = cp1 == 0.0 || cp2 == 0.0;
    let mut dh = h2 - h1;
    if achromatic {
        dh = 0.0;
    } else if dh > 180.0 {
        dh -= 360.0;
    } else if dh < -180.0 {
        dh += 360.0;
    }
    let dl = l2 - l1;
    let dc = cp2 - cp1;
    let d_h = 2.0 * (cp1 * cp2).sqrt() * (dh / 2.0).to_radians().sin();
    let l_bar = (l1 + l2) / 2.0;
    let cp_bar = (cp1 + cp2) / 2.0;
    let h_bar = if achromatic {
        h1 + h2
    } else if (h1 - h2).abs() <= 180.0 {
        (h1 + h2) / 2.0
    } else if h1 + h2 < 360.0 {
        (h1 + h2 + 360.0) / 2.0
    } else {
        (h1 + h2 - 360.0) / 2.0
    };
    let cos = |degrees: f64| degrees.to_radians().cos();
    let t =
        1.0 - 0.17 * cos(h_bar - 30.0) + 0.24 * cos(2.0 * h_bar) + 0.32 * cos(3.0 * h_bar + 6.0)
            - 0.20 * cos(4.0 * h_bar - 63.0);
    let l_offset_sq = (l_bar - 50.0).powi(2);
    let sl = 1.0 + 0.015 * l_offset_sq / (20.0 + l_offset_sq).sqrt();
    let sc = 1.0 + 0.045 * cp_bar;
    let sh = 1.0 + 0.015 * cp_bar * t;
    let theta = 30.0 * (-((h_bar - 275.0) / 25.0).powi(2)).exp();
    let rt = -2.0 * chroma_ratio(cp_bar).sqrt() * (2.0 * theta).to_radians().sin();
    let (vl, vc, vh) = (dl / sl, dc / sc, d_h / sh);
    (vl * vl + vc * vc + vh * vh + rt * vc * vh).sqrt()
}

/// Compare raw scene-linear Rec.2020 RGB via XYZ and CIELab with a D65 white.
///
/// Both inputs use the same relative scale: RGB [1, 1, 1] is D65 with Y = 1.
/// No tone mapper, display mapping, transfer function, or chromatic adaptation
/// is applied. Negative XYZ components are clamped to zero *after* the matrix;
/// RGB and highlights above one are not clipped. This is a regression metric,
/// not a claim about perceptual differences after HDR display rendering.
/// Callers may enforce a golden-suite tolerance such as delta E <= 0.5.
/// Inputs must be finite (NaN/infinity are invalid test samples).
pub fn linear_rec2020_delta_e(a: [f32; 3], b: [f32; 3]) -> f64 {
    fn lab(rgb: [f32; 3]) -> [f64; 3] {
        // Rec.2020 primaries, D65 xy = (0.3127, 0.3290), Y normalized to 1.
        // Matrix and white use the same chromaticities, avoiding neutral bias
        // from mixing rounded ICC D65 tristimulus constants with this matrix.
        let [r, g, b] = rgb.map(f64::from);
        let xyz = [
            0.6369580483012914 * r + 0.1446169035862083 * g + 0.1688809751641721 * b,
            0.2627002120112671 * r + 0.6779980715188708 * g + 0.0593017164698620 * b,
            0.0280726930490874 * g + 1.060_985_057_710_791 * b,
        ];
        let white = [0.3127 / 0.3290, 1.0, (1.0 - 0.3127 - 0.3290) / 0.3290];
        let f = |t: f64| {
            if t > 216.0 / 24389.0 {
                t.cbrt()
            } else {
                (24389.0 / 27.0 * t + 16.0) / 116.0
            }
        };
        let [x, y, z] = std::array::from_fn(|i| f(xyz[i].max(0.0) / white[i]));
        [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
    }
    // Do not let XYZ's negative clamp silently turn NaN into a valid sample.
    if a.into_iter().chain(b).any(|v| !v.is_finite()) {
        return f64::NAN;
    }
    delta_e_2000(lab(a), lab(b))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_linear_neutrals_and_identity() {
        assert!((linear_rec2020_delta_e([0.0; 3], [1.0; 3]) - 100.0).abs() < 1e-10);
        for rgb in [[0.0; 3], [0.18; 3], [4.0; 3], [1.0, 0.2, 0.0]] {
            assert_eq!(linear_rec2020_delta_e(rgb, rgb), 0.0);
        }
        // Neutral cube roots are exact: no gamma decoding or tone mapping.
        let expected = delta_e_2000([42.0, 0.0, 0.0], [216.0, 0.0, 0.0]);
        assert!((linear_rec2020_delta_e([0.125; 3], [8.0; 3]) - expected).abs() < 1e-10);
        // The low-light linear Lab branch: L* = (24389/27) * Y.
        let y = 0.001_f32;
        let expected = delta_e_2000([0.0; 3], [(24389.0 / 27.0) * f64::from(y), 0.0, 0.0]);
        assert!((linear_rec2020_delta_e([0.0; 3], [y; 3]) - expected).abs() < 1e-10);
    }

    #[test]
    fn negatives_are_clamped_in_xyz_not_rgb() {
        assert_eq!(linear_rec2020_delta_e([-1.0; 3], [0.0; 3]), 0.0);
        assert!(linear_rec2020_delta_e([-0.1, 1.0, 0.0], [0.0, 1.0, 0.0]) > 0.5);
    }

    #[test]
    fn sharma_reference_vectors_in_both_directions() {
        // All 34 supplementary pairs from Sharma, Wu & Dalal (2005).
        // https://hajim.rochester.edu/ece/sites/gsharma/ciede2000/dataNprograms/ciede2000testdata.txt
        // Published results have four decimal places (rounding error <= 0.00005).
        let vectors: [([f64; 3], [f64; 3], f64); 34] = [
            (
                [50.0000, 2.6772, -79.7751],
                [50.0000, 0.0000, -82.7485],
                2.0425,
            ),
            (
                [50.0000, 3.1571, -77.2803],
                [50.0000, 0.0000, -82.7485],
                2.8615,
            ),
            (
                [50.0000, 2.8361, -74.0200],
                [50.0000, 0.0000, -82.7485],
                3.4412,
            ),
            (
                [50.0000, -1.3802, -84.2814],
                [50.0000, 0.0000, -82.7485],
                1.0000,
            ),
            (
                [50.0000, -1.1848, -84.8006],
                [50.0000, 0.0000, -82.7485],
                1.0000,
            ),
            (
                [50.0000, -0.9009, -85.5211],
                [50.0000, 0.0000, -82.7485],
                1.0000,
            ),
            (
                [50.0000, 0.0000, 0.0000],
                [50.0000, -1.0000, 2.0000],
                2.3669,
            ),
            (
                [50.0000, -1.0000, 2.0000],
                [50.0000, 0.0000, 0.0000],
                2.3669,
            ),
            (
                [50.0000, 2.4900, -0.0010],
                [50.0000, -2.4900, 0.0009],
                7.1792,
            ),
            (
                [50.0000, 2.4900, -0.0010],
                [50.0000, -2.4900, 0.0010],
                7.1792,
            ),
            (
                [50.0000, 2.4900, -0.0010],
                [50.0000, -2.4900, 0.0011],
                7.2195,
            ),
            (
                [50.0000, 2.4900, -0.0010],
                [50.0000, -2.4900, 0.0012],
                7.2195,
            ),
            (
                [50.0000, -0.0010, 2.4900],
                [50.0000, 0.0009, -2.4900],
                4.8045,
            ),
            (
                [50.0000, -0.0010, 2.4900],
                [50.0000, 0.0010, -2.4900],
                4.8045,
            ),
            (
                [50.0000, -0.0010, 2.4900],
                [50.0000, 0.0011, -2.4900],
                4.7461,
            ),
            (
                [50.0000, 2.5000, 0.0000],
                [50.0000, 0.0000, -2.5000],
                4.3065,
            ),
            (
                [50.0000, 2.5000, 0.0000],
                [73.0000, 25.0000, -18.0000],
                27.1492,
            ),
            (
                [50.0000, 2.5000, 0.0000],
                [61.0000, -5.0000, 29.0000],
                22.8977,
            ),
            (
                [50.0000, 2.5000, 0.0000],
                [56.0000, -27.0000, -3.0000],
                31.9030,
            ),
            (
                [50.0000, 2.5000, 0.0000],
                [58.0000, 24.0000, 15.0000],
                19.4535,
            ),
            ([50.0000, 2.5000, 0.0000], [50.0000, 3.1736, 0.5854], 1.0000),
            ([50.0000, 2.5000, 0.0000], [50.0000, 3.2972, 0.0000], 1.0000),
            ([50.0000, 2.5000, 0.0000], [50.0000, 1.8634, 0.5757], 1.0000),
            ([50.0000, 2.5000, 0.0000], [50.0000, 3.2592, 0.3350], 1.0000),
            (
                [60.2574, -34.0099, 36.2677],
                [60.4626, -34.1751, 39.4387],
                1.2644,
            ),
            (
                [63.0109, -31.0961, -5.8663],
                [62.8187, -29.7946, -4.0864],
                1.2630,
            ),
            (
                [61.2901, 3.7196, -5.3901],
                [61.4292, 2.2480, -4.9620],
                1.8731,
            ),
            (
                [35.0831, -44.1164, 3.7933],
                [35.0232, -40.0716, 1.5901],
                1.8645,
            ),
            (
                [22.7233, 20.0904, -46.6940],
                [23.0331, 14.9730, -42.5619],
                2.0373,
            ),
            (
                [36.4612, 47.8580, 18.3852],
                [36.2715, 50.5065, 21.2231],
                1.4146,
            ),
            (
                [90.8027, -2.0831, 1.4410],
                [91.1528, -1.6435, 0.0447],
                1.4441,
            ),
            (
                [90.9257, -0.5406, -0.9208],
                [88.6381, -0.8985, -0.7239],
                1.5381,
            ),
            (
                [6.7747, -0.2908, -2.4247],
                [5.8714, -0.0985, -2.2286],
                0.6377,
            ),
            (
                [2.0776, 0.0795, -1.1350],
                [0.9033, -0.0636, -0.5514],
                0.9082,
            ),
        ];
        for (index, (a, b, expected)) in vectors.into_iter().enumerate() {
            for (left, right) in [(a, b), (b, a)] {
                let actual = delta_e_2000(left, right);
                assert!(
                    (actual - expected).abs() <= 0.00005,
                    "pair {}: {left:?} -> {right:?}: expected {expected}, got {actual}",
                    index + 1
                );
            }
        }
    }
}
