//! Lens point operators and composed geometry support. See LENS_M2.md.
use crate::Image;
use engine_api::{EngineError, EngineResult, recipe::settings::LensSettings};

pub(crate) fn validate(s: &LensSettings) -> EngineResult<()> {
    if [
        s.distortion_scale,
        s.vignetting_scale,
        s.chromatic_aberration_scale,
        s.manual_distortion,
        s.manual_vignetting,
        s.manual_vignetting_midpoint,
        s.defringe_purple.amount,
        s.defringe_green.amount,
        s.softness_correction,
    ]
    .iter()
    .chain(s.defringe_purple.hue_range.iter())
    .chain(s.defringe_green.hue_range.iter())
    .any(|v| !v.is_finite())
    {
        return Err(EngineError::invalid("lens", "finite parameters required"));
    }
    if s.softness_correction != 0. {
        return Err(EngineError::Unsupported {
            what: "lens PSF softness correction".into(),
        });
    }
    Ok(())
}
pub(crate) fn profile_vignette(
    image: &Image,
    s: &LensSettings,
    correction: &crate::ResolvedLens,
) -> EngineResult<Image> {
    let default = lens::CalibrationSample::default();
    let sample = correction.sample().unwrap_or(&default);
    if (sample.vignette == [0.; 3] && correction.embedded.gains.is_empty())
        || s.vignetting_scale == 0.
    {
        return Ok(image.clone());
    }
    let mut planes = image.planes().to_vec();
    let v = sample.vignette;
    for y in 0..image.height() {
        for x in 0..image.width() {
            let px = (2. * (x as f64 + 0.5) / image.width() as f64 - 1. - sample.distortion.cx)
                * sample.coordinate_scale[0];
            let py = (2. * (y as f64 + 0.5) / image.height() as f64 - 1. - sample.distortion.cy)
                * sample.coordinate_scale[1];
            let r = px * px + py * py;
            let illumination = (1. + r * (v[0] + r * (v[1] + r * v[2]))).clamp(0.125, 8.);
            let embedded = correction.embedded.gain([
                2. * (x as f64 + 0.5) / image.width() as f64 - 1.,
                2. * (y as f64 + 0.5) / image.height() as f64 - 1.,
            ]);
            let gain = (1.
                + (embedded / illumination - 1.) * s.vignetting_scale.clamp(0., 200.) as f64
                    / 100.)
                .clamp(0.125, 8.);
            for plane in &mut planes {
                let i = (y * image.width() + x) as usize;
                plane[i] = (plane[i] as f64 * gain).clamp(-f32::MAX as f64, f32::MAX as f64) as f32;
            }
        }
    }
    Image::new(image.width(), image.height(), planes)
}
/// Resample CA only, before camera-profile mixing. Bayer phases remain separate;
/// other inputs use their original linear RGB planes. Crop defines optical coordinates
/// without shifting the CFA phase or removing real demosaic neighbours.
pub(crate) fn lateral_ca(
    image: &Image,
    cfa: Option<raw_decode::CfaLayout>,
    crop: [u32; 4],
    s: &LensSettings,
    correction: &crate::ResolvedLens,
) -> EngineResult<Image> {
    if !correction.ca_active(s) {
        return Ok(image.clone());
    }
    let bayer = matches!(cfa, Some(raw_decode::CfaLayout::Bayer(_)));
    let mut planes = image.planes().to_vec();
    for (plane, dst) in planes.iter_mut().enumerate() {
        for y in 0..image.height() {
            for x in 0..image.width() {
                let channel = if bayer {
                    cfa.unwrap().channel_at(x, y)
                } else {
                    plane
                };
                if channel == 1 || channel == 3 {
                    continue;
                }
                let p = [
                    2. * (x as f64 + 0.5 - crop[0] as f64) / crop[2] as f64 - 1.,
                    2. * (y as f64 + 0.5 - crop[1] as f64) / crop[3] as f64 - 1.,
                ];
                let q = correction
                    .ca_map(p, channel, s)
                    .filter(|q| q.iter().all(|v| v.is_finite()))
                    .ok_or_else(|| {
                        EngineError::invalid("lateral CA", "noninvertible channel map")
                    })?;
                let sx = (q[0] + 1.) * crop[2] as f64 / 2. + crop[0] as f64 - 0.5;
                let sy = (q[1] + 1.) * crop[3] as f64 / 2. + crop[1] as f64 - 0.5;
                let step = if bayer { 2 } else { 1 };
                let (px, py) = (x % step, y % step);
                let (w, h) = (
                    (image.width() - 1 - px) / step,
                    (image.height() - 1 - py) / step,
                );
                let u = ((sx - px as f64) / step as f64).clamp(0., w as f64);
                let v = ((sy - py as f64) / step as f64).clamp(0., h as f64);
                let (a, b) = (u.floor() as u32, v.floor() as u32);
                let at = |xx: u32, yy: u32| {
                    image.planes()[plane]
                        [((yy.min(h) * step + py) * image.width() + xx.min(w) * step + px) as usize]
                        as f64
                };
                let value = (at(a, b) * (1. - u.fract()) + at(a + 1, b) * u.fract())
                    * (1. - v.fract())
                    + (at(a, b + 1) * (1. - u.fract()) + at(a + 1, b + 1) * u.fract()) * v.fract();
                dst[(y * image.width() + x) as usize] =
                    value.clamp(-f32::MAX as f64, f32::MAX as f64) as f32;
            }
        }
    }
    Image::new(image.width(), image.height(), planes)
}

pub(crate) fn point_corrections(image: &Image, s: &LensSettings) -> EngineResult<Image> {
    if s.manual_vignetting == 0. && s.defringe_purple.amount == 0. && s.defringe_green.amount == 0.
    {
        return Ok(image.clone());
    }
    let mut planes = image.planes().to_vec();
    for y in 0..image.height() {
        for x in 0..image.width() {
            let p = [
                2. * (x as f64 + 0.5) / image.width() as f64 - 1.,
                2. * (y as f64 + 0.5) / image.height() as f64 - 1.,
            ];
            let r = (p[0] * p[0] + p[1] * p[1]) / 2.;
            let midpoint = s.manual_vignetting_midpoint.clamp(0., 100.) as f64 / 100.;
            let gain = (s.manual_vignetting.clamp(-100., 100.) as f64 / 50.
                * r.powf(0.25 + 3.75 * midpoint))
            .exp2();
            for plane in &mut planes {
                let i = (y * image.width() + x) as usize;
                plane[i] = (plane[i] as f64 * gain).clamp(-f32::MAX as f64, f32::MAX as f64) as f32;
            }
        }
    }
    if s.defringe_purple.amount > 0. || s.defringe_green.amount > 0. {
        let w = image.width() as usize;
        let h = image.height() as usize;
        for y in 0..h {
            for x in 0..w {
                let i = y * w + x;
                let rgb = std::array::from_fn(|c| image.planes()[c][i]);
                let lum = crate::luminance(rgb);
                let max = rgb.iter().copied().fold(f32::NEG_INFINITY, f32::max);
                let min = rgb.iter().copied().fold(f32::INFINITY, f32::min);
                let chroma = max - min;
                if chroma <= 0.15 * max.max(1e-6) {
                    continue;
                }
                let hue = (60.
                    * if max == rgb[0] {
                        (rgb[1] - rgb[2]) / chroma
                    } else if max == rgb[1] {
                        2. + (rgb[2] - rgb[0]) / chroma
                    } else {
                        4. + (rgb[0] - rgb[1]) / chroma
                    })
                .rem_euclid(360.);
                let mut edge = 0.0_f32;
                for (nx, ny) in [
                    (x.saturating_sub(1), y),
                    ((x + 1).min(w - 1), y),
                    (x, y.saturating_sub(1)),
                    (x, (y + 1).min(h - 1)),
                ] {
                    let near =
                        crate::luminance(std::array::from_fn(|c| image.planes()[c][ny * w + nx]));
                    edge = edge.max((near - lum).abs() / near.abs().max(lum.abs()).max(0.02));
                }
                if edge < 0.2 {
                    continue;
                }
                let mut amount = 0.0_f32;
                for band in [&s.defringe_purple, &s.defringe_green] {
                    let lo = band.hue_range[0].rem_euclid(360.);
                    let hi = band.hue_range[1].rem_euclid(360.);
                    if (lo <= hi && hue >= lo && hue <= hi) || (lo > hi && (hue >= lo || hue <= hi))
                    {
                        amount = amount.max(band.amount.clamp(0., 20.) / 20.);
                    }
                }
                let target = crate::luminance(std::array::from_fn(|c| planes[c][i]));
                for plane in &mut planes {
                    plane[i] += amount * (target - plane[i]);
                }
            }
        }
    }
    Image::new(image.width(), image.height(), planes)
}
