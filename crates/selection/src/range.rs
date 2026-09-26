//! Colour range: sampled colours with fuzziness, tonal ranges, skin tones.

use crate::mask::{Image, Mask};

/// sRGB (display-referred, `0..=1`) to CIE L*a*b* (D65).
pub fn srgb_to_lab(c: [f32; 3]) -> [f32; 3] {
    let lin = |v: f32| {
        let v = v.clamp(0.0, 1.0);
        if v <= 0.04045 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    let (r, g, b) = (lin(c[0]), lin(c[1]), lin(c[2]));
    let x = (0.4124 * r + 0.3576 * g + 0.1805 * b) / 0.95047;
    let y = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    let z = (0.0193 * r + 0.1192 * g + 0.9505 * b) / 1.08883;
    let f = |t: f32| {
        if t > 216.0 / 24389.0 {
            t.cbrt()
        } else {
            (24389.0 / 27.0 * t + 16.0) / 116.0
        }
    };
    let (fx, fy, fz) = (f(x), f(y), f(z));
    [116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz)]
}

/// Membership for a colour distance `d` (ΔE*ab) at `fuzziness`: 1 within
/// half the fuzziness, linear falloff to 0 at the fuzziness.
pub fn falloff(d: f32, fuzziness: f32) -> f32 {
    if fuzziness <= 0.0 {
        return f32::from(u8::from(d <= 1e-3));
    }
    let half = fuzziness * 0.5;
    if d <= half {
        1.0
    } else {
        ((fuzziness - d) / half).clamp(0.0, 1.0)
    }
}

/// Selects colours near any of `samples` (sRGB), `fuzziness` in ΔE*ab.
pub fn color_range(img: &Image, samples: &[[f32; 3]], fuzziness: f32) -> Mask {
    let labs: Vec<[f32; 3]> = samples.iter().map(|s| srgb_to_lab(*s)).collect();
    let data = img
        .data
        .iter()
        .map(|p| {
            let l = srgb_to_lab([p[0], p[1], p[2]]);
            labs.iter()
                .map(|s| {
                    falloff(
                        ((l[0] - s[0]).powi(2) + (l[1] - s[1]).powi(2) + (l[2] - s[2]).powi(2))
                            .sqrt(),
                        fuzziness,
                    )
                })
                .fold(0.0f32, f32::max)
        })
        .collect();
    Mask::from_vec(img.width, img.height, data).unwrap_or_else(|_| Mask::new(img.width, img.height))
}

/// Predefined colour-range selections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tonal {
    /// L* above 75 (falloff to 55).
    Highlights,
    /// L* 40..60 (falloff to 20/80).
    Midtones,
    /// L* below 25 (falloff to 45).
    Shadows,
    /// YCbCr skin-tone box with a soft margin (heuristic, not a learned
    /// classifier).
    SkinTones,
}

/// Tonal / skin-tone selection.
pub fn tonal_range(img: &Image, t: Tonal) -> Mask {
    let ramp = |v: f32, a: f32, b: f32| ((v - a) / (b - a)).clamp(0.0, 1.0);
    let data = img
        .data
        .iter()
        .map(|p| match t {
            Tonal::SkinTones => {
                let (r, g, b) = (p[0] * 255.0, p[1] * 255.0, p[2] * 255.0);
                let cb = 128.0 - 0.168_736 * r - 0.331_264 * g + 0.5 * b;
                let cr = 128.0 + 0.5 * r - 0.418_688 * g - 0.081_312 * b;
                let m = 6.0;
                ramp(cb, 77.0 - m, 77.0)
                    .min(1.0 - ramp(cb, 127.0, 127.0 + m))
                    .min(ramp(cr, 133.0 - m, 133.0))
                    .min(1.0 - ramp(cr, 173.0, 173.0 + m))
            }
            _ => {
                let l = srgb_to_lab([p[0], p[1], p[2]])[0];
                match t {
                    Tonal::Highlights => ramp(l, 55.0, 75.0),
                    Tonal::Shadows => 1.0 - ramp(l, 25.0, 45.0),
                    _ => ramp(l, 20.0, 40.0).min(1.0 - ramp(l, 60.0, 80.0)),
                }
            }
        })
        .collect();
    Mask::from_vec(img.width, img.height, data).unwrap_or_else(|_| Mask::new(img.width, img.height))
}
