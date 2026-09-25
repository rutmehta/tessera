//! Scalar M2 colour and detail operators; formulas in COLOR_DETAIL_M2.md.
use engine_api::{
    EngineResult,
    recipe::settings::{ColorSettings, DetailSettings},
    tile::Tile,
};

/// Perceptual colour in linear Rec.2020/D65; processes interior and halo.
pub fn color(tile: &mut Tile, s: &ColorSettings) -> EngineResult<()> {
    validate_tile(tile)?;
    if !s.point_colors.is_empty() || s.lut.is_some() {
        return Err(engine_api::EngineError::invalid(
            "color",
            "Point Color and LUT are not implemented in M2",
        ));
    }
    finite(&[
        s.vibrance,
        s.saturation,
        s.grading.balance,
        s.grading.blending,
    ])?;
    for b in [&s.hsl.hue, &s.hsl.saturation, &s.hsl.luminance] {
        finite(&[
            b.red, b.orange, b.yellow, b.green, b.aqua, b.blue, b.purple, b.magenta,
        ])?;
    }
    for w in [
        &s.grading.shadows,
        &s.grading.midtones,
        &s.grading.highlights,
        &s.grading.global,
    ] {
        finite(&[w.hue, w.saturation, w.luminance])?;
    }
    if s == &ColorSettings::default() {
        return Ok(());
    }
    crate::map_rgb(tile, |rgb| {
        let [mut l, a, b] = to_lab(rgb);
        let mut c = a.hypot(b);
        let mut h = b.atan2(a).to_degrees().rem_euclid(360.0);
        let skin = (-0.5 * (hue_distance(h, 50.0) / 25.0).powi(2)).exp();
        let muted = 1.0 / (1.0 + c / (0.25 * l.abs().max(0.05)));
        c *= (1.0 + unit(s.vibrance) * muted * (1.0 - 0.7 * skin)) * (1.0 + unit(s.saturation));
        // Membership uses the original hue, never the already-shifted hue.
        let weights = hue_weights(h);
        if c > 1e-6 {
            h += 30.0 * weighted(&s.hsl.hue, weights);
            c *= 1.0 + weighted(&s.hsl.saturation, weights);
            l *= 1.0 + 0.5 * weighted(&s.hsl.luminance, weights);
        }
        let mut lab = [l, c * h.to_radians().cos(), c * h.to_radians().sin()];
        let g = &s.grading;
        let t = (l + 0.25 * unit(g.balance)).clamp(0.0, 1.0);
        let width = 0.15 + 0.5 * g.blending.clamp(0.0, 100.0) / 100.0;
        let raw = [0.0, 0.5, 1.0].map(|center| (-0.5 * ((t - center) / width).powi(2)).exp());
        let sum: f32 = raw.iter().sum();
        for (wheel, weight) in [&g.shadows, &g.midtones, &g.highlights, &g.global]
            .into_iter()
            .zip([raw[0] / sum, raw[1] / sum, raw[2] / sum, 1.0])
        {
            let angle = wheel.hue.rem_euclid(360.0).to_radians();
            let chroma =
                0.2 * wheel.saturation.clamp(0.0, 100.0) / 100.0 * weight * l.abs().min(1.0);
            lab[1] += chroma * angle.cos();
            lab[2] += chroma * angle.sin();
            lab[0] += 0.25 * unit(wheel.luminance) * weight;
        }
        from_lab(lab)
    })
}

fn finite(values: &[f32]) -> EngineResult<()> {
    if values.iter().any(|v| !v.is_finite()) {
        return Err(engine_api::EngineError::invalid(
            "color/detail",
            "finite values required",
        ));
    }
    Ok(())
}
fn validate_tile(tile: &Tile) -> EngineResult<()> {
    if tile.layout().channels != 3 {
        return Err(engine_api::EngineError::invalid(
            "tile",
            "expected three RGB planes",
        ));
    }
    finite(tile.samples::<f32>()?)
}

fn unit(v: f32) -> f32 {
    v.clamp(-100.0, 100.0) / 100.0
}
fn hue_distance(a: f32, b: f32) -> f32 {
    (a - b + 180.0).rem_euclid(360.0) - 180.0
}

fn hue_weights(h: f32) -> [f32; 8] {
    const CENTERS: [f32; 9] = [25.0, 55.0, 95.0, 145.0, 195.0, 255.0, 295.0, 335.0, 385.0];
    let h = (h - 25.0).rem_euclid(360.0) + 25.0;
    let mut weights = [0.0; 8];
    for i in 0..8 {
        if h >= CENTERS[i] && h <= CENTERS[i + 1] {
            let t = (h - CENTERS[i]) / (CENTERS[i + 1] - CENTERS[i]);
            let w = 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
            weights[i] = 1.0 - w;
            weights[(i + 1) % 8] = w;
            break;
        }
    }
    weights
}
fn weighted(b: &engine_api::recipe::settings::HueBands, w: [f32; 8]) -> f32 {
    [
        b.red, b.orange, b.yellow, b.green, b.aqua, b.blue, b.purple, b.magenta,
    ]
    .into_iter()
    .zip(w)
    .map(|(v, w)| unit(v) * w)
    .sum()
}

fn mul(m: [[f32; 3]; 3], v: [f32; 3]) -> [f32; 3] {
    m.map(|r| r[0] * v[0] + r[1] * v[1] + r[2] * v[2])
}

// Rec.2020 -> Oklab LMS, D65. Signed cube roots retain negative values.
fn to_lab(rgb: [f32; 3]) -> [f32; 3] {
    let lms = mul(
        [
            [0.6167558, 0.3601984, 0.0230458],
            [0.265133, 0.6358394, 0.0990276],
            [0.1001026, 0.2039065, 0.6959909],
        ],
        rgb,
    )
    .map(f32::cbrt);
    mul(
        [
            [0.21045426, 0.7936178, -0.004072047],
            [1.9779985, -2.4285922, 0.4505937],
            [0.025904037, 0.78277177, -0.80867577],
        ],
        lms,
    )
}

fn from_lab(lab: [f32; 3]) -> [f32; 3] {
    let lms = mul(
        [
            [1.0, 0.39633778, 0.21580376],
            [1.0, -0.105561346, -0.06385417],
            [1.0, -0.08948418, -1.2914855],
        ],
        lab,
    )
    .map(|v| v * v * v);
    mul(
        [
            [2.1399066, -1.2463895, 0.1064829],
            [-0.8847359, 2.163231, -0.2784951],
            [-0.0485738, -0.4545031, 1.5030769],
        ],
        lms,
    )
}
/// Maximum support in pixels. Gather real neighbours; clamp only at image edges.
pub const DETAIL_HALO: u16 = 9;

fn active_detail(s: &DetailSettings) -> (bool, bool, bool) {
    let d = DetailSettings::default();
    let n = &s.noise_reduction;
    (
        s.sharpening != d.sharpening && s.sharpening.amount > 0.0,
        n.luminance > 0.0,
        n.color > 0.0
            && (n.color != d.noise_reduction.color
                || n.color_detail != d.noise_reduction.color_detail
                || n.color_smoothness != d.noise_reduction.color_smoothness),
    )
}

/// Required input halo; output interior is valid, output halo is unchanged.
pub fn detail_halo(s: &DetailSettings) -> u16 {
    let (sharp, lum, chroma) = active_detail(s);
    let mut r = if sharp {
        (3.0 * s.sharpening.radius.clamp(0.5, 3.0)).ceil() as u16
    } else {
        0
    };
    if sharp || lum {
        r = r.max(2);
    }
    if chroma {
        r = r.max(chroma_radius(s));
    }
    r
}

fn chroma_radius(s: &DetailSettings) -> u16 {
    (1.0 + 4.0 * s.noise_reduction.color_smoothness.clamp(0.0, 100.0) / 100.0).ceil() as u16
}

fn kernel(radius: u16, sigma: f32, stride: usize) -> Vec<(isize, f32)> {
    let r = i32::from(radius);
    (-r..=r)
        .flat_map(|y| {
            (-r..=r).map(move |x| {
                (
                    y as isize * stride as isize + x as isize,
                    (-0.5 * (x * x + y * y) as f32 / (sigma * sigma)).exp(),
                )
            })
        })
        .collect()
}

fn y_of(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}

fn edge(y: &[f32], i: usize, stride: usize) -> f32 {
    let mut sum = 0.0;
    for dy in -1isize..=1 {
        for dx in -1isize..=1 {
            let j = (i as isize + dy * stride as isize + dx) as usize;
            let gx = y[j - stride + 1] + 2.0 * y[j + 1] + y[j + stride + 1]
                - y[j - stride - 1]
                - 2.0 * y[j - 1]
                - y[j + stride - 1];
            let gy = y[j + stride - 1] + 2.0 * y[j + stride] + y[j + stride + 1]
                - y[j - stride - 1]
                - 2.0 * y[j - stride]
                - y[j - stride + 1];
            sum += gx.hypot(gy) / 8.0;
        }
    }
    sum / 9.0
}

/// Interleaved linear-luminance detail and Oklab chroma NR from one immutable
/// source decomposition. Only the interior is written; regather before reuse.
pub fn detail(tile: &mut Tile, s: &DetailSettings) -> EngineResult<()> {
    validate_tile(tile)?;
    let sh = &s.sharpening;
    let nr = &s.noise_reduction;
    finite(&[
        sh.amount,
        sh.radius,
        sh.detail,
        sh.masking,
        nr.luminance,
        nr.luminance_detail,
        nr.luminance_contrast,
        nr.color,
        nr.color_detail,
        nr.color_smoothness,
    ])?;
    if !(0.0..=150.0).contains(&sh.amount)
        || !(0.5..=3.0).contains(&sh.radius)
        || [
            sh.detail,
            sh.masking,
            nr.luminance,
            nr.luminance_detail,
            nr.luminance_contrast,
            nr.color,
            nr.color_detail,
            nr.color_smoothness,
        ]
        .iter()
        .any(|v| !(0.0..=100.0).contains(v))
    {
        return Err(engine_api::EngineError::invalid(
            "detail",
            "controls outside documented settings ranges",
        ));
    }
    let (sharp, lum, chroma) = active_detail(s);
    if !(sharp || lum || chroma) {
        return Ok(());
    }
    let l = tile.layout();
    if l.channels != 3 || l.halo < detail_halo(s) {
        return Err(engine_api::EngineError::invalid(
            "detail",
            "three RGB planes and sufficient real-neighbour halo required",
        ));
    }
    let n = l.plane_len();
    let src = tile.samples::<f32>()?;
    let rgb: Vec<[f32; 3]> = (0..n)
        .map(|i| [src[i], src[n + i], src[2 * n + i]])
        .collect();
    let ys: Vec<_> = rgb.iter().copied().map(y_of).collect();
    let labs: Vec<_> = if chroma {
        rgb.iter().copied().map(to_lab).collect()
    } else {
        Vec::new()
    };
    let sh = &s.sharpening;
    let nr = &s.noise_reduction;
    let sigma = sh.radius.clamp(0.5, 3.0);
    let ks = if sharp {
        kernel((3.0 * sigma).ceil() as u16, sigma, l.stride())
    } else {
        Vec::new()
    };
    let kl = if lum {
        kernel(2, 1.2, l.stride())
    } else {
        Vec::new()
    };
    let kc = if chroma {
        kernel(
            chroma_radius(s),
            0.7 + 2.0 * unit(nr.color_smoothness),
            l.stride(),
        )
    } else {
        Vec::new()
    };
    let dst = tile.samples_mut::<f32>()?;
    for y in 0..l.extent.height as i32 {
        for x in 0..l.extent.width as i32 {
            let i = l.index(0, x, y).unwrap();
            let base = ys[i];
            let mut target = base;
            if sharp {
                let mut delta = 0.0;
                let mut total = 0.0;
                for &(offset, w) in &ks {
                    delta += w * (ys[(i as isize + offset) as usize] - base);
                    total += w;
                }
                let residual = -delta / total;
                let d = unit(sh.detail);
                let limit = 0.05 * (base.abs() + 0.1);
                let boost = (1.0 - d) * residual.clamp(-limit, limit) + d * residual * 1.5;
                let masking = unit(sh.masking);
                let gate = if masking <= 0.0 {
                    1.0
                } else {
                    let t = ((edge(&ys, i, l.stride()) / (base.abs() + 0.1) - 0.15 * masking)
                        / 0.05)
                        .clamp(0.0, 1.0);
                    t * t * (3.0 - 2.0 * t)
                };
                target += sh.amount.clamp(0.0, 150.0) / 100.0 * gate * boost;
            }
            if lum {
                let range = (0.02 + 0.18 * (1.0 - unit(nr.luminance_detail))) * (base.abs() + 0.1);
                let mut delta = 0.0;
                let mut total = 0.0;
                let mut variance = 0.0;
                let mut spatial = 0.0;
                for &(offset, w) in &kl {
                    let d = ys[(i as isize + offset) as usize] - base;
                    let weight = w * (-0.5 * (d / range).powi(2)).exp();
                    delta += weight * d;
                    total += weight;
                    variance += w * d * d;
                    spatial += w;
                }
                let protect = 1.0
                    / (1.0
                        + 8.0 * unit(nr.luminance_contrast) * variance / spatial / (range * range));
                target += unit(nr.luminance) * protect * delta / total;
            }
            let mut out = rgb[i];
            if chroma {
                let lab = labs[i];
                let range = 0.02 + 0.18 * (1.0 - unit(nr.color_detail));
                let mut delta = [0.0; 2];
                let mut total = 0.0;
                for &(offset, w) in &kc {
                    let q = labs[(i as isize + offset) as usize];
                    let da = q[1] - lab[1];
                    let db = q[2] - lab[2];
                    let weight = w
                        * (-0.5
                            * ((da * da + db * db) / (range * range)
                                + ((q[0] - lab[0]) / 0.08).powi(2)))
                        .exp();
                    delta[0] += weight * da;
                    delta[1] += weight * db;
                    total += weight;
                }
                let amount = unit(nr.color) / total;
                if delta != [0.0; 2] {
                    out = from_lab([
                        lab[0],
                        lab[1] + amount * delta[0],
                        lab[2] + amount * delta[1],
                    ]);
                }
            }
            // Equal RGB offset changes Y without an unstable division near black.
            let shift = target - y_of(out);
            for c in 0..3 {
                dst[c * n + i] = out[c] + shift;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Image;
    use engine_api::tile::TileCoord;

    fn sample(rgb: [f32; 3]) -> Tile {
        Image::new(1, 1, rgb.map(|v| vec![v]).to_vec())
            .unwrap()
            .tile(TileCoord::new(0, 0, 0), 0, 1)
            .unwrap()
    }

    fn output(lab: [f32; 3], s: &ColorSettings) -> [f32; 3] {
        let mut t = sample(from_lab(lab));
        color(&mut t, s).unwrap();
        to_lab(t.samples::<f32>().unwrap().try_into().unwrap())
    }

    #[test]
    fn vibrance_boosts_muted_colors_and_protects_skin() {
        let s = ColorSettings {
            vibrance: 100.0,
            ..Default::default()
        };
        let chroma = |h: f32, c: f32| {
            let h = h.to_radians();
            let p = output([0.6, c * h.cos(), c * h.sin()], &s);
            p[1].hypot(p[2]) / c
        };
        assert!(chroma(220.0, 0.04) > 1.1);
        assert!(chroma(220.0, 0.04) > chroma(220.0, 0.25));
        assert!(chroma(220.0, 0.04) > chroma(50.0, 0.04));
    }

    #[test]
    fn eight_hsl_bands_change_their_centers_smoothly() {
        for (i, h) in [25.0f32, 55.0, 95.0, 145.0, 195.0, 255.0, 295.0, 335.0]
            .into_iter()
            .enumerate()
        {
            let mut s = ColorSettings::default();
            let bands = &mut s.hsl.hue;
            *[
                &mut bands.red,
                &mut bands.orange,
                &mut bands.yellow,
                &mut bands.green,
                &mut bands.aqua,
                &mut bands.blue,
                &mut bands.purple,
                &mut bands.magenta,
            ][i] = 100.0;
            let p = output(
                [0.6, 0.1 * h.to_radians().cos(), 0.1 * h.to_radians().sin()],
                &s,
            );
            let delta = (p[2].atan2(p[1]).to_degrees() - h + 180.0).rem_euclid(360.0) - 180.0;
            assert!((delta - 30.0).abs() < 0.01, "band {i}: {delta}");
        }
    }

    #[test]
    fn grading_wheels_balance_and_blending_are_effective() {
        let mut s = ColorSettings::default();
        s.grading.shadows.saturation = 70.0;
        s.grading.shadows.hue = 30.0;
        s.grading.highlights.saturation = 70.0;
        s.grading.highlights.hue = 210.0;
        let p = output([0.4, 0.0, 0.0], &s);
        assert!(p[1].abs() > 0.001);
        s.grading.balance = 80.0;
        let q = output([0.4, 0.0, 0.0], &s);
        assert!((p[1] - q[1]).abs() > 0.001);
        s.grading.blending = 0.0;
        let r = output([0.4, 0.0, 0.0], &s);
        assert!((q[1] - r[1]).abs() > 0.001);
        s = ColorSettings::default();
        s.grading.global.luminance = 50.0;
        assert!(output([0.4, 0.0, 0.0], &s)[0] > 0.4);
    }

    fn noisy_tile(halo: u16) -> Tile {
        let p: Vec<f32> = (0..81)
            .map(|i| 0.3 + if i % 2 == 0 { 0.02 } else { -0.02 })
            .collect();
        Image::new(9, 9, vec![p.clone(), p.clone(), p])
            .unwrap()
            .tile(TileCoord::new(0, 0, 0), halo, 1)
            .unwrap()
    }

    #[test]
    fn defaults_are_bit_neutral_including_halos() {
        let mut t = noisy_tile(9);
        let before = t.samples::<f32>().unwrap().to_vec();
        color(&mut t, &ColorSettings::default()).unwrap();
        detail(&mut t, &DetailSettings::default()).unwrap();
        assert_eq!(before, t.samples::<f32>().unwrap());
    }

    #[test]
    fn sharpening_controls_boost_and_mask_texture() {
        let original = noisy_tile(9);
        let l = original.layout();
        let i = l.index(0, 4, 4).unwrap();
        let mut s = DetailSettings::default();
        s.sharpening.amount = 100.0;
        let mut t = original.clone();
        detail(&mut t, &s).unwrap();
        let boosted = t.samples::<f32>().unwrap()[i];
        assert!(boosted > 0.325);
        s.sharpening.masking = 100.0;
        let mut masked = original.clone();
        detail(&mut masked, &s).unwrap();
        assert!(masked.samples::<f32>().unwrap()[i] < boosted - 0.001);
        s.sharpening.masking = 0.0;
        s.sharpening.detail = 100.0;
        let mut fine = original.clone();
        detail(&mut fine, &s).unwrap();
        assert!((fine.samples::<f32>().unwrap()[i] - boosted).abs() > 0.0001);
        s.sharpening.radius = 0.5;
        let mut small = original;
        detail(&mut small, &s).unwrap();
        assert!(
            (small.samples::<f32>().unwrap()[i] - fine.samples::<f32>().unwrap()[i]).abs() > 0.0001
        );
    }

    #[test]
    fn luminance_nr_reduces_noise_without_activating_default_sharpening() {
        let mut t = noisy_tile(9);
        let i = t.layout().index(0, 4, 4).unwrap();
        let mut s = DetailSettings::default();
        s.noise_reduction.luminance = 100.0;
        detail(&mut t, &s).unwrap();
        assert!((t.samples::<f32>().unwrap()[i] - 0.3).abs() < 0.015);
    }

    #[test]
    fn chroma_nr_smooths_opponents_preserving_linear_luminance() {
        let rgb: Vec<_> = (0..81)
            .map(|i| from_lab([0.65, if i % 2 == 0 { 0.025 } else { -0.025 }, 0.02]))
            .collect();
        let image = Image::new(
            9,
            9,
            (0..3).map(|c| rgb.iter().map(|p| p[c]).collect()).collect(),
        )
        .unwrap();
        let mut t = image.tile(TileCoord::new(0, 0, 0), 9, 1).unwrap();
        let l = t.layout();
        let mut s = DetailSettings::default();
        s.noise_reduction.color = 100.0;
        detail(&mut t, &s).unwrap();
        let p =
            std::array::from_fn(|c| t.samples::<f32>().unwrap()[l.index(c as u8, 4, 4).unwrap()]);
        assert!(to_lab(p)[1].abs() < 0.02);
        let y = |p: [f32; 3]| 0.2627 * p[0] + 0.678 * p[1] + 0.0593 * p[2];
        assert!((y(p) - y(rgb[40])).abs() < 2e-6);
    }

    #[test]
    fn active_detail_requires_real_halo_without_partial_write() {
        let mut t = noisy_tile(0);
        let before = t.samples::<f32>().unwrap().to_vec();
        let mut s = DetailSettings::default();
        s.sharpening.amount = 80.0;
        assert!(detail(&mut t, &s).is_err());
        assert_eq!(before, t.samples::<f32>().unwrap());
    }

    #[test]
    fn invalid_inputs_and_unsupported_controls_are_rejected_atomically() {
        let mut t = noisy_tile(9);
        let before = t.samples::<f32>().unwrap().to_vec();
        let mut c = ColorSettings {
            vibrance: f32::NAN,
            ..Default::default()
        };
        assert!(color(&mut t, &c).is_err());
        c = ColorSettings::default();
        c.point_colors.push(Default::default());
        assert!(color(&mut t, &c).is_err());
        c = ColorSettings::default();
        c.lut = Some(Default::default());
        assert!(color(&mut t, &c).is_err());
        let mut d = DetailSettings::default();
        d.noise_reduction.color_detail = f32::INFINITY;
        assert!(detail(&mut t, &d).is_err());
        d = DetailSettings::default();
        d.noise_reduction.luminance_contrast = -100.0;
        assert!(detail(&mut t, &d).is_err());
        assert_eq!(before, t.samples::<f32>().unwrap());
        let mut mono = Image::new(1, 1, vec![vec![0.2]])
            .unwrap()
            .tile(TileCoord::new(0, 0, 0), 0, 1)
            .unwrap();
        assert!(color(&mut mono, &ColorSettings::default()).is_err());
        assert!(detail(&mut mono, &DetailSettings::default()).is_err());
        t.samples_mut::<f32>().unwrap()[0] = f32::NAN;
        assert!(detail(&mut t, &DetailSettings::default()).is_err());
    }

    #[test]
    fn signed_hdr_roundtrip_and_neutral_bit_patterns() {
        for rgb in [
            [-0.1, 0.4, 3.0],
            [10.0, 2.0, 0.5],
            [-1.0, -0.3, -0.2],
            [0.0; 3],
        ] {
            let back = from_lab(to_lab(rgb));
            for c in 0..3 {
                assert!((back[c] - rgb[c]).abs() < 2e-5);
            }
        }
        let mut t = sample([-0.0, -0.1, 4.0]);
        let bits = t
            .samples::<f32>()
            .unwrap()
            .iter()
            .map(|v| v.to_bits())
            .collect::<Vec<_>>();
        color(&mut t, &ColorSettings::default()).unwrap();
        detail(&mut t, &DetailSettings::default()).unwrap();
        assert_eq!(
            bits,
            t.samples::<f32>()
                .unwrap()
                .iter()
                .map(|v| v.to_bits())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn hsl_windows_partition_and_wrap_and_lightness_is_independent() {
        for h in -360..720 {
            let w = hue_weights(h as f32);
            assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-6);
            assert!(w.iter().all(|&v| (0.0..=1.0).contains(&v)));
        }
        for (a, b) in hue_weights(359.999).into_iter().zip(hue_weights(0.001)) {
            assert!((a - b).abs() < 0.001);
        }
        let mut s = ColorSettings::default();
        s.hsl.luminance.red = 100.0;
        let h = 25f32.to_radians();
        let p = output([0.4, 0.1 * h.cos(), 0.1 * h.sin()], &s);
        assert!((p[0] - 0.6).abs() < 1e-5);
        assert!((p[1].hypot(p[2]) - 0.1).abs() < 1e-5);
        s.hsl.saturation.red = -100.0;
        let p = output([0.4, 0.1 * h.cos(), 0.1 * h.sin()], &s);
        assert!(p[1].hypot(p[2]) < 1e-5);
    }

    #[test]
    fn detail_subsettings_are_independent_and_halo_is_bounded() {
        let mut s = DetailSettings::default();
        assert_eq!(detail_halo(&s), 0);
        s.sharpening.radius = 3.0;
        assert_eq!(detail_halo(&s), DETAIL_HALO);
        s = DetailSettings::default();
        s.noise_reduction.luminance = 100.0;
        assert_eq!(detail_halo(&s), 2);
        let run = |s: &DetailSettings| {
            let mut t = noisy_tile(DETAIL_HALO);
            detail(&mut t, s).unwrap();
            t.samples::<f32>().unwrap()[t.layout().index(0, 4, 4).unwrap()]
        };
        let base = run(&s);
        s.noise_reduction.luminance_detail = 100.0;
        assert!(run(&s) > base + 0.001);
        s.noise_reduction.luminance_detail = 50.0;
        s.noise_reduction.luminance_contrast = 100.0;
        assert!(run(&s) > base + 0.001);
        // Explicitly disable other suboperators: identical to default bypass.
        s.noise_reduction.luminance_contrast = 0.0;
        s.sharpening.amount = 0.0;
        s.noise_reduction.color = 0.0;
        assert_eq!(run(&s).to_bits(), base.to_bits());
    }

    #[test]
    fn detail_is_partition_invariant_and_keeps_input_halo() {
        use engine_api::tile::{Extent, TileLayout};
        let pixel = |x: i32, y: i32| {
            [
                0.2 + 0.01 * ((x * 7 + y * 3).rem_euclid(11)) as f32,
                0.3,
                0.4,
            ]
        };
        let make = |ox: i32, width: u32| {
            let l = TileLayout {
                extent: Extent::new(width, 4),
                halo: DETAIL_HALO,
                channels: 3,
            };
            let mut data = Vec::new();
            for c in 0..3 {
                for y in -(DETAIL_HALO as i32)..4 + DETAIL_HALO as i32 {
                    for x in -(DETAIL_HALO as i32)..width as i32 + DETAIL_HALO as i32 {
                        data.push(pixel(ox + x, y)[c]);
                    }
                }
            }
            Tile::from_samples(TileCoord::new(0, 0, 0), l, data).unwrap()
        };
        let mut s = DetailSettings::default();
        s.sharpening.amount = 80.0;
        s.sharpening.radius = 3.0;
        s.sharpening.masking = 10.0;
        s.noise_reduction.luminance = 40.0;
        s.noise_reduction.color = 60.0;
        let mut full = make(0, 16);
        detail(&mut full, &s).unwrap();
        for ox in [0, 8] {
            let mut part = make(ox, 8);
            let before = part.samples::<f32>().unwrap().to_vec();
            detail(&mut part, &s).unwrap();
            let l = part.layout();
            assert_eq!(
                before[0].to_bits(),
                part.samples::<f32>().unwrap()[0].to_bits()
            );
            for c in 0..3 {
                for y in 0..4 {
                    for x in 0..8 {
                        let a = part.samples::<f32>().unwrap()[l.index(c, x, y).unwrap()];
                        let b = full.samples::<f32>().unwrap()
                            [full.layout().index(c, ox + x, y).unwrap()];
                        assert_eq!(a.to_bits(), b.to_bits());
                    }
                }
            }
        }
    }

    #[test]
    fn saturation_minus_100_removes_chroma() {
        let mut tile = sample([0.7, 0.2, 0.1]);
        let s = ColorSettings {
            saturation: -100.0,
            ..Default::default()
        };
        color(&mut tile, &s).unwrap();
        let p = tile.samples::<f32>().unwrap();
        assert!((p[0] - p[1]).abs() < 2e-5 && (p[1] - p[2]).abs() < 2e-5);
    }
}
