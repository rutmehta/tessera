//! Independent, approximate Adobe PV1–PV6 rendering. See ADOBE_COMPAT.md.
pub mod curves;
pub mod dcp;
mod embedded_profile;
pub mod fidelity;
pub use embedded_profile::{
    SUBSTITUTED_PROFILE_NOTICE, UNAVAILABLE_PROFILE_NOTICE, embedded_profile_fallback,
};
mod render;
use engine_api::recipe::settings::ToneSettings;
pub use pipeline_cpu::{Image, RenderSource, Rgb8Image};
pub use render::{
    render_linear_scaled, render_linear_scaled_with_denoiser, render_linear_scaled_with_profile,
    render_linear_scaled_with_profile_and_locals, render_linear_scaled_with_resources,
    render_scaled, render_scaled_with_profile,
};

/// Recognize imported Adobe profile identities; this never selects a process family.
pub fn names_adobe_profile(settings: &engine_api::recipe::DevelopSettings) -> bool {
    settings
        .camera_profile
        .profile
        .name
        .0
        .split_whitespace()
        .next()
        .is_some_and(|word| word.eq_ignore_ascii_case("Adobe"))
}

/// Scene-linear basic tone operator, before profile/user curves.
pub fn basic_tone(rgb: [f32; 3], s: &ToneSettings) -> [f32; 3] {
    let rgb = if let Some(legacy) = &s.legacy_pv2010 {
        pipeline_cpu::legacy_pv2010::apply(rgb, legacy)
    } else {
        rgb
    };
    let rgb = rgb.map(|v| v * s.exposure.clamp(-10., 10.).exp2());
    let y = luminance(rgb);
    if y <= 0. {
        return rgb;
    }
    // Contrast pivots around 18% scene grey in stops, not encoded RGB.
    let c = s.contrast.clamp(-100., 100.) / 100.;
    let mut out = if c == 0. {
        y
    } else {
        0.18 * (y / 0.18).powf((0.5 * c).exp2())
    };
    let smooth = |a: f32, b: f32, x: f32| {
        let u = ((x - a) / (b - a)).clamp(0., 1.);
        u * u * (3. - 2. * u)
    };
    // Feathered log gains retain channel ratios and headroom.
    out *= (s.highlights.clamp(-100., 100.) / 100. * smooth(0.25, 1., out)).exp2();
    out *= (s.shadows.clamp(-100., 100.) / 100. * (1. - smooth(0.02, 0.25, out))).exp2();
    // White-point gain stretches the histogram, with black fixed. A narrow
    // brightness-dependent negative gain can reverse a ramp; this cannot.
    out *= (0.5 * s.whites.clamp(-100., 100.) / 100.).exp2();
    out *= (s.blacks.clamp(-100., 100.) / 100. * (1. - smooth(0., 0.08, out))).exp2();
    rgb.map(|v| v * (out / y))
}

fn luminance(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}

/// Validate source BaselineExposure only at the Adobe boundary. Native retains
/// the metadata as data and never applies or validates an exposure gain.
pub fn validate_baseline_exposure(baseline_exposure: f32) -> engine_api::EngineResult<()> {
    let gain = baseline_exposure.exp2();
    if !baseline_exposure.is_finite() || !gain.is_finite() || gain <= 0. {
        return Err(engine_api::EngineError::invalid(
            "BaselineExposure",
            "finite positive gain required",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn individual_tone_controls_preserve_ramp_order_at_extremes() {
        for control in 0..5 {
            for amount in [-100., 100.] {
                let mut s = ToneSettings::default();
                *match control {
                    0 => &mut s.contrast,
                    1 => &mut s.highlights,
                    2 => &mut s.shadows,
                    3 => &mut s.whites,
                    _ => &mut s.blacks,
                } = amount;
                let mut previous = 0.;
                for i in 0..=2000 {
                    let y = basic_tone([i as f32 / 1000.; 3], &s)[0];
                    assert!(y >= previous, "control={control} amount={amount} index={i}");
                    previous = y;
                }
            }
        }
    }
    #[test]
    fn tone_controls_are_directional_and_region_limited() {
        for index in 0..5 {
            let mut s = ToneSettings::default();
            let value = match index {
                0 => &mut s.contrast,
                1 => &mut s.highlights,
                2 => &mut s.shadows,
                3 => &mut s.whites,
                _ => &mut s.blacks,
            };
            *value = 100.;
            let x = if index == 2 || index == 4 { 0.02 } else { 0.9 };
            assert!(basic_tone([x; 3], &s)[0] > x, "control {index}");
        }
        let s = ToneSettings {
            highlights: -100.,
            ..Default::default()
        };
        assert!(basic_tone([0.95; 3], &s)[0] < 0.95);
        assert_eq!(basic_tone([0.02; 3], &s), [0.02; 3]);
    }
    #[test]
    fn exposure_doubles_linear_midtones() {
        let s = ToneSettings {
            exposure: 1.,
            ..Default::default()
        };
        assert_eq!(basic_tone([0.18; 3], &s), [0.36; 3]);
    }
}
