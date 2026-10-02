//! Independent PV2010 reference branch. See ../LEGACY_PV2010.md.
use engine_api::{EngineError, EngineResult, recipe::settings::LegacyPv2010};

pub fn validate(s: &LegacyPv2010) -> EngineResult<()> {
    if [
        s.exposure,
        s.brightness,
        s.contrast,
        s.fill_light,
        s.recovery,
        s.blacks,
    ]
    .into_iter()
    .flatten()
    .any(|v| !v.is_finite())
    {
        return Err(EngineError::invalid("PV2010", "finite parameters required"));
    }
    Ok(())
}
/// EV exposure, highlight shoulder, black point, shadow fill, midtone brightness,
/// then contrast. Absent fields are neutral, with no inferred camera defaults.
pub fn apply(rgb: [f32; 3], s: &LegacyPv2010) -> [f32; 3] {
    let mut rgb = rgb.map(|v| v * s.exposure.unwrap_or(0.).clamp(-5., 5.).exp2());
    if let Some(v) = s.recovery {
        let y = super::luminance(rgb);
        let a = v.clamp(0., 100.) / 100.;
        if y > 0.75 {
            let recovered = 0.75 + (y - 0.75) / (1. + a * (y - 0.75));
            rgb = rgb.map(|v| v * (recovered / y));
        }
    }
    if let Some(v) = s.blacks
        && v != 0.
    {
        // Public DNG baseline shadow ramp, applied per channel. Camera
        // ShadowScale/Stage3Gain are assumed one; full PV2010 parity is unknown.
        let black = v.clamp(0., 100.) * 0.001;
        let slope = 1. / (1. - black);
        let radius = (0.5 * black).min((1. - black) / 16.);
        rgb = rgb.map(|x| {
            if x <= black - radius {
                0.
            } else if x >= black + radius {
                (x - black) * slope
            } else {
                let toe = x - black + radius;
                slope * toe * toe / (4. * radius)
            }
        });
    }
    let y = super::luminance(rgb);
    if y <= 0. {
        return rgb;
    }
    let mut out = y;
    if let Some(v) = s.fill_light
        && v != 0.
        && out < 1.
    {
        out += 2. * v.clamp(0., 100.) / 100. * out * (1. - out).powi(2);
    }
    if let Some(v) = s.brightness
        && v != 0.
        && out < 1.
    {
        let gain = (v.clamp(-150., 150.) / 100.).exp2();
        out = gain * out / (1. + (gain - 1.) * out);
    }
    if let Some(v) = s.contrast
        && v != 0.
        && out > 0.
        && out < 1.
    {
        let slope = (v.clamp(-50., 100.) / 100.).exp2();
        out = 1. / (1. + ((1. - out) / out).powf(slope));
    }
    rgb.map(|v| (v * (out / y)).clamp(-f32::MAX, f32::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lr2b_legacy_operator_goldens() {
        // Pinned scalar reference values evaluated independently in f64 from
        // LEGACY_PV2010.md; these are not Adobe-rendered calibration images.
        let cases = [
            (
                LegacyPv2010 {
                    exposure: Some(1.),
                    ..Default::default()
                },
                [0., 0.04, 0.36, 1., 2., 4.],
            ),
            (
                LegacyPv2010 {
                    brightness: Some(100.),
                    ..Default::default()
                },
                [0., 0.039215686, 0.305084746, 0.666666667, 1., 2.],
            ),
            (
                LegacyPv2010 {
                    contrast: Some(100.),
                    ..Default::default()
                },
                [0., 0.00041632, 0.045970488, 0.5, 1., 2.],
            ),
            (
                LegacyPv2010 {
                    fill_light: Some(100.),
                    ..Default::default()
                },
                [0., 0.058416, 0.422064, 0.75, 1., 2.],
            ),
            (
                LegacyPv2010 {
                    recovery: Some(100.),
                    ..Default::default()
                },
                [0., 0.02, 0.18, 0.5, 0.95, 1.305555556],
            ),
            (
                LegacyPv2010 {
                    blacks: Some(10.),
                    ..Default::default()
                },
                [
                    0.,
                    0.0101010101,
                    0.1717171717,
                    0.4949494949,
                    1.,
                    2.0101010101,
                ],
            ),
        ];
        for (s, expected) in cases {
            for (x, y) in [0., 0.02, 0.18, 0.5, 1., 2.].into_iter().zip(expected) {
                let actual = apply([x; 3], &s);
                assert!(
                    actual.iter().all(|v| (f64::from(*v) - y).abs() < 1e-6),
                    "{s:?} {x}: {actual:?} != {y}"
                );
            }
            let mut previous = 0.;
            for x in 0..=2000 {
                let y = apply([x as f32 / 1000.; 3], &s)[0];
                assert!(y >= previous, "{s:?}: ramp reversal at {x}");
                previous = y;
            }
        }
    }
    #[test]
    fn lr2b_legacy_validation_and_absence() {
        let rgb = [-0.1, 0.2, 1.2];
        assert_eq!(apply(rgb, &LegacyPv2010::default()), rgb);
        assert!(
            validate(&LegacyPv2010 {
                recovery: Some(f32::NAN),
                ..Default::default()
            })
            .is_err()
        );
    }
}

#[cfg(test)]
#[test]
fn lr2b_black_toe_uses_public_dng_shadow_scale() {
    // SDK's normalized black = Shadows * .001 at ShadowScale/Stage3Gain=1.
    // The quadratic toe spans black +/- black/2. No SDR clipping here.
    let s = LegacyPv2010 {
        blacks: Some(10.),
        ..Default::default()
    };
    for (input, expected) in [
        (0.005, 0.),
        (0.01, 0.0012626263),
        (0.015, 0.005050505),
        (0.02, 0.01010101),
    ] {
        let actual = apply([input; 3], &s)[0];
        assert!(
            (actual - expected).abs() < 1e-7,
            "{input}: {actual} != {expected}"
        );
    }
}

#[cfg(test)]
#[test]
fn lr2b_black_toe_maps_channels_independently() {
    let s = LegacyPv2010 {
        blacks: Some(10.),
        ..Default::default()
    };
    let actual = apply([0.01, 0.02, 0.18], &s);
    for (a, b) in actual
        .into_iter()
        .zip([0.0012626263, 0.01010101, 0.17171717])
    {
        assert!((a - b).abs() < 1e-7, "{actual:?}");
    }
}
