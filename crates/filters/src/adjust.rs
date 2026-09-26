//! Straight-alpha, scene-linear Rec.2020 image adjustments.
use engine_api::EngineResult;

/// CPU-reference image adjustments. Values are normalized unless documented
/// otherwise; alpha is metadata, never premultiplied into the colour math.
/// Identity parameter sets preserve out-of-range scene-linear values exactly.
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
#[serde(rename_all = "snake_case", deny_unknown_fields)]
pub enum Adjustment {
    /// Per-channel levels; non-neutral normalized input is clamped to [0,1].
    Levels {
        input_black: [f32; 3],
        input_white: [f32; 3],
        gamma: [f32; 3],
        output_black: [f32; 3],
        output_white: [f32; 3],
    },
    /// Monotone cubic Hermite curves, one per channel. Y is nondecreasing,
    /// X is strictly increasing, and points span x=0..1;
    /// outside that domain the endpoint tangent is used (HDR is not clipped).
    Curves { points: [Vec<[f32; 2]>; 3] },
    /// Brightness [-150,150], contrast [-100,100]. Non-legacy uses a
    /// bounded midtone curve; legacy uses an affine transform around 0.5.
    BrightnessContrast {
        brightness: f32,
        contrast: f32,
        legacy: bool,
    },
    /// Stops [-32,32], finite offset, positive gamma; sign-preserving power.
    Exposure { stops: f32, offset: f32, gamma: f32 },
    /// Rec.2020 luminance >= level maps to white; level is in [0,1].
    Threshold { level: f32 },
    /// Uniform quantization of clamped RGB to 2..=65535 levels.
    Posterize { levels: u16 },
    /// Master hue in degrees; saturation and lightness shifts in [-1,1].
    /// HSL is defined on RGB clamped to [0,1] in linear Rec.2020, not encoded
    /// sRGB. Neutral controls are an exact no-op, including for HDR inputs.
    Hsl {
        hue_degrees: f32,
        saturation: f32,
        lightness: f32,
    },
    /// Chroma boosts in [-1,1]; vibrance preferentially boosts muted colours.
    Vibrance { vibrance: f32, saturation: f32 },
    /// Linear filter colour [0,1], density [0,1].
    PhotoFilter {
        colour: [f32; 3],
        density: f32,
        preserve_luminosity: bool,
    },
    /// Row-major RGB matrix, followed by per-channel constants.
    ChannelMixer {
        matrix: [[f32; 3]; 3],
        constant: [f32; 3],
    },
    /// Sorted (position, linear RGB) stops spanning 0..1; linear interpolation.
    GradientMap {
        stops: Vec<(f32, [f32; 3])>,
        reverse: bool,
    },
    /// CMYK offsets [-1,1] for red, yellow, green, cyan, blue, magenta,
    /// white, neutral, black. Relative scales each correction by available ink.
    SelectiveColour {
        corrections: [[f32; 4]; 9],
        relative: bool,
    },
    /// Nonnegative luminance multipliers for R,Y,G,C,B,M hue bands.
    /// Optional linear RGB tint; None produces neutral grey.
    BlackWhite {
        weights: [f32; 6],
        tint: Option<[f32; 3]>,
    },
    /// Reinhard mean/population-standard-deviation transfer in Oklab over the
    /// entire slice (not independently per tile). Target samples are linear
    /// Rec.2020 RGB. Every sample participates regardless of alpha. Amount
    /// is [0,1]. Constant source axes use a mean shift.
    MatchColour { target: Vec<[f32; 3]>, amount: f32 },
    /// Component-wise 1-RGB, without clamping.
    Invert,
    /// Neutral grey at Rec.2020 luminance, without clamping.
    Desaturate,
}
impl Default for Adjustment {
    fn default() -> Self {
        Self::Exposure {
            stops: 0.0,
            offset: 0.0,
            gamma: 1.0,
        }
    }
}
impl Adjustment {
    /// Whether valid parameters are a no-op for every RGB value, including HDR.
    /// Invalid parameter sets are never considered identities.
    pub fn is_identity(&self) -> bool {
        if self.validate().is_err() {
            return false;
        }
        match self {
            Self::Exposure {
                stops,
                offset,
                gamma,
            } => *stops == 0.0 && *offset == 0.0 && *gamma == 1.0,
            Self::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            } => {
                *input_black == [0.0; 3]
                    && *input_white == [1.0; 3]
                    && *gamma == [1.0; 3]
                    && *output_black == [0.0; 3]
                    && *output_white == [1.0; 3]
            }
            Self::Curves { points } => points.iter().flatten().all(|p| p[0] == p[1]),
            Self::Hsl {
                hue_degrees,
                saturation,
                lightness,
            } => hue_degrees.rem_euclid(360.0) == 0.0 && *saturation == 0.0 && *lightness == 0.0,
            Self::BrightnessContrast {
                brightness,
                contrast,
                ..
            } => *brightness == 0.0 && *contrast == 0.0,
            Self::Vibrance {
                vibrance,
                saturation,
            } => *vibrance == 0.0 && *saturation == 0.0,
            Self::PhotoFilter {
                colour, density, ..
            } => *density == 0.0 || *colour == [1.0; 3],
            Self::ChannelMixer { matrix, constant } => {
                *matrix == [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
                    && *constant == [0.0; 3]
            }
            Self::SelectiveColour { corrections, .. } => {
                corrections.iter().flatten().all(|v| *v == 0.0)
            }
            Self::MatchColour { amount, .. } => *amount == 0.0,
            _ => false,
        }
    }
    /// Validate all parameters before any pixels are modified.
    pub fn validate(&self) -> EngineResult<()> {
        let valid = match self {
            Self::Exposure {
                stops,
                offset,
                gamma,
            } => finite(&[*stops, *offset, *gamma]) && stops.abs() <= 32.0 && *gamma > 0.0,
            Self::Levels {
                input_black,
                input_white,
                gamma,
                output_black,
                output_white,
            } => {
                finite(input_black)
                    && finite(input_white)
                    && finite(gamma)
                    && finite(output_black)
                    && finite(output_white)
                    && (0..3).all(|c| {
                        input_white[c] > input_black[c]
                            && gamma[c] > 0.0
                            && output_white[c] >= output_black[c]
                    })
            }
            Self::Curves { points } => points.iter().all(|p| {
                p.len() >= 2
                    && p[0][0] == 0.0
                    && p[p.len() - 1][0] == 1.0
                    && p.iter().all(|p| finite(p))
                    && p.windows(2)
                        .all(|w| w[1][0] > w[0][0] && w[1][1] >= w[0][1])
            }),
            Self::BrightnessContrast {
                brightness,
                contrast,
                ..
            } => range(*brightness, -150.0, 150.0) && range(*contrast, -100.0, 100.0),
            Self::Threshold { level } => range(*level, 0.0, 1.0),
            Self::Posterize { levels } => *levels >= 2,
            Self::MatchColour { target, amount } => {
                !target.is_empty() && target.iter().all(|c| finite(c)) && range(*amount, 0.0, 1.0)
            }
            Self::Invert | Self::Desaturate => true,
            Self::Hsl {
                hue_degrees,
                saturation,
                lightness,
            } => {
                hue_degrees.is_finite()
                    && range(*saturation, -1.0, 1.0)
                    && range(*lightness, -1.0, 1.0)
            }
            Self::Vibrance {
                vibrance,
                saturation,
            } => range(*vibrance, -1.0, 1.0) && range(*saturation, -1.0, 1.0),
            Self::PhotoFilter {
                colour, density, ..
            } => colour.iter().all(|v| range(*v, 0.0, 1.0)) && range(*density, 0.0, 1.0),
            Self::ChannelMixer { matrix, constant } => {
                matrix.iter().all(|r| finite(r)) && finite(constant)
            }
            Self::GradientMap { stops, .. } => {
                stops.len() >= 2
                    && stops[0].0 == 0.0
                    && stops[stops.len() - 1].0 == 1.0
                    && stops.iter().all(|(x, c)| x.is_finite() && finite(c))
                    && stops.windows(2).all(|w| w[1].0 > w[0].0)
            }
            Self::SelectiveColour { corrections, .. } => {
                corrections.iter().flatten().all(|v| range(*v, -1.0, 1.0))
            }
            Self::BlackWhite { weights, tint } => {
                weights.iter().all(|v| v.is_finite() && *v >= 0.0)
                    && tint
                        .as_ref()
                        .is_none_or(|c| c.iter().all(|v| range(*v, 0.0, 1.0)))
            }
        };
        if valid {
            Ok(())
        } else {
            Err(engine_api::EngineError::invalid(
                "adjustment",
                "non-finite or out-of-domain parameter",
            ))
        }
    }
    /// Apply to straight-alpha RGB without clipping HDR unless an operator's
    /// definition requires a bounded domain. Alpha bits are preserved exactly,
    /// including at zero coverage. Nonfinite RGB or output overflow returns an
    /// error without modifying any pixels. Empty slices are accepted.
    pub fn apply(&self, pixels: &mut [[f32; 4]]) -> EngineResult<()> {
        self.validate()?;
        if pixels.iter().any(|p| !finite(&p[..3])) {
            return Err(engine_api::EngineError::invalid(
                "pixels",
                "RGB must be finite",
            ));
        }
        if pixels.is_empty() || self.is_identity() {
            return Ok(());
        }
        let mut adjusted = pixels.to_vec();
        self.apply_validated(&mut adjusted)?;
        if adjusted.iter().any(|p| !finite(&p[..3])) {
            return Err(engine_api::EngineError::invalid(
                "adjustment",
                "RGB output overflow",
            ));
        }
        for (pixel, out) in pixels.iter_mut().zip(adjusted) {
            pixel[..3].copy_from_slice(&out[..3]);
        }
        Ok(())
    }
    fn apply_validated(&self, pixels: &mut [[f32; 4]]) -> EngineResult<()> {
        if let Self::MatchColour { target, amount } = self {
            return match_colour(pixels, target, *amount);
        }
        // Precompute spline derivatives once, rather than once per pixel.
        let slopes = if let Self::Curves { points } = self {
            Some(points.each_ref().map(|p| curve_slopes(p)))
        } else {
            None
        };
        for pixel in pixels {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            let out = match self {
                Self::Exposure {
                    stops,
                    offset,
                    gamma,
                } => rgb.map(|v| signed_power(v * stops.exp2() + offset, 1.0 / gamma)),
                Self::Levels {
                    input_black,
                    input_white,
                    gamma,
                    output_black,
                    output_white,
                } => std::array::from_fn(|c| {
                    let t = ((rgb[c] - input_black[c]) / (input_white[c] - input_black[c]))
                        .clamp(0.0, 1.0);
                    t.powf(1.0 / gamma[c]) * (output_white[c] - output_black[c]) + output_black[c]
                }),
                Self::Curves { points } => std::array::from_fn(|c| {
                    curve_value(&points[c], &slopes.as_ref().unwrap()[c], rgb[c])
                }),
                Self::BrightnessContrast {
                    brightness,
                    contrast,
                    legacy,
                } => rgb.map(|v| {
                    if *legacy {
                        (v - 0.5) * (1.0 + contrast / 100.0) + 0.5 + brightness / 150.0
                    } else if !(0.0..=1.0).contains(&v) {
                        v
                    } else {
                        let b = brightness / 150.0;
                        let v = v + b * v * (1.0 - v);
                        // Smooth monotone S curve, fixed black, white and midpoint.
                        let k = contrast / 100.0;
                        v + k * (v - 0.5) * 2.0 * v * (1.0 - v)
                    }
                }),
                Self::Threshold { level } => [if luminance(rgb) >= *level { 1.0 } else { 0.0 }; 3],
                Self::Posterize { levels } => {
                    let n = f32::from(*levels - 1);
                    rgb.map(|v| (v.clamp(0.0, 1.0) * n).round() / n)
                }
                Self::MatchColour { .. } => unreachable!(),
                Self::Invert => rgb.map(|v| 1.0 - v),
                Self::Desaturate => [luminance(rgb); 3],
                Self::Hsl {
                    hue_degrees,
                    saturation,
                    lightness,
                } => {
                    if *hue_degrees == 0.0 && *saturation == 0.0 && *lightness == 0.0 {
                        rgb
                    } else {
                        let [h, s, l] = rgb_to_hsl(rgb.map(|v| v.clamp(0.0, 1.0)));
                        let s = shift_unit(s, *saturation);
                        let l = shift_unit(l, *lightness);
                        hsl_to_rgb([(h + hue_degrees / 360.0).rem_euclid(1.0), s, l])
                    }
                }
                Self::Vibrance {
                    vibrance,
                    saturation,
                } => {
                    let y = luminance(rgb);
                    let max = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
                    let min = rgb.into_iter().fold(f32::INFINITY, f32::min);
                    let chroma = if max > 0.0 {
                        ((max - min) / max).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let gain = (1.0 + saturation) * (1.0 + vibrance * (1.0 - chroma));
                    rgb.map(|v| y + (v - y) * gain)
                }
                Self::PhotoFilter {
                    colour,
                    density,
                    preserve_luminosity,
                } => {
                    let filtered =
                        std::array::from_fn(|c| rgb[c] * (1.0 - density + density * colour[c]));
                    let y = luminance(filtered);
                    if *preserve_luminosity {
                        if y.abs() > 1e-10 {
                            filtered.map(|v| v * luminance(rgb) / y)
                        } else {
                            rgb
                        }
                    } else {
                        filtered
                    }
                }
                Self::ChannelMixer { matrix, constant } => std::array::from_fn(|c| {
                    matrix[c][0] * rgb[0]
                        + matrix[c][1] * rgb[1]
                        + matrix[c][2] * rgb[2]
                        + constant[c]
                }),
                Self::GradientMap { stops, reverse } => {
                    let mut t = luminance(rgb).clamp(0.0, 1.0);
                    if *reverse {
                        t = 1.0 - t;
                    }
                    let i = stops
                        .partition_point(|p| p.0 <= t)
                        .saturating_sub(1)
                        .min(stops.len() - 2);
                    let u = (t - stops[i].0) / (stops[i + 1].0 - stops[i].0);
                    std::array::from_fn(|c| stops[i].1[c] * (1.0 - u) + stops[i + 1].1[c] * u)
                }
                Self::SelectiveColour {
                    corrections,
                    relative,
                } => {
                    let bounded = rgb.map(|v| v.clamp(0.0, 1.0));
                    let max = bounded.into_iter().fold(0.0, f32::max);
                    let min = bounded.into_iter().fold(1.0, f32::min);
                    let chroma = max - min;
                    let hue = rgb_to_hsl(bounded)[0];
                    let mut w = [0.0; 9];
                    for (i, weight) in w[..6].iter_mut().enumerate() {
                        *weight = hue_weight(hue, i) * chroma;
                    }
                    w[6] = (2.0 * min - 1.0).max(0.0);
                    w[8] = (1.0 - 2.0 * max).max(0.0);
                    w[7] = (1.0 - chroma - w[6] - w[8]).max(0.0);
                    let correction: [f32; 4] =
                        std::array::from_fn(|c| (0..9).map(|i| w[i] * corrections[i][c]).sum());
                    std::array::from_fn(|c| {
                        let scale = if *relative { 1.0 - bounded[c] } else { 1.0 };
                        let black_scale = if *relative { 1.0 - max } else { 1.0 };
                        rgb[c] - correction[c] * scale - correction[3] * black_scale
                    })
                }
                Self::BlackWhite { weights, tint } => {
                    let [h, s, _] = rgb_to_hsl(rgb.map(|v| v.clamp(0.0, 1.0)));
                    let gain: f32 = weights
                        .iter()
                        .enumerate()
                        .map(|(i, w)| hue_weight(h, i) * w)
                        .sum();
                    let y = luminance(rgb) * (1.0 + s * (gain - 1.0));
                    tint.unwrap_or([1.0; 3]).map(|v| v * y)
                }
            };
            pixel[..3].copy_from_slice(&out);
        }
        Ok(())
    }
}
// Same linear Rec.2020 -> LMS coefficients as the existing colour-range
// operator. That operator and the FFI's Oklab conversion are private; reuse
// the public engine matrix implementation for the inverse, not private APIs.
const RGB_TO_LMS: engine_api::color::ColorMatrix3 = engine_api::color::ColorMatrix3([
    [0.6167558, 0.3601984, 0.0230458],
    [0.265133, 0.6358394, 0.0990276],
    [0.1001026, 0.2039065, 0.6959909],
]);
const LMS_TO_LAB: engine_api::color::ColorMatrix3 = engine_api::color::ColorMatrix3([
    [0.21045426, 0.7936178, -0.004072047],
    [1.9779985, -2.4285922, 0.4505937],
    [0.025904037, 0.78277177, -0.80867577],
]);
fn to_oklab(rgb: [f32; 3]) -> [f64; 3] {
    LMS_TO_LAB.apply(RGB_TO_LMS.apply(rgb.map(f64::from)).map(f64::cbrt))
}
fn lab_statistics(samples: impl Iterator<Item = [f32; 3]>) -> ([f64; 3], [f64; 3]) {
    let mut mean = [0.0; 3];
    let mut m2 = [0.0; 3];
    let mut n = 0.0;
    for rgb in samples {
        n += 1.0;
        let lab = to_oklab(rgb);
        for c in 0..3 {
            let delta = lab[c] - mean[c];
            mean[c] += delta / n;
            m2[c] += delta * (lab[c] - mean[c]);
        }
    }
    (mean, m2.map(|v| (v / n.max(1.0)).max(0.0).sqrt()))
}
fn match_colour(pixels: &mut [[f32; 4]], target: &[[f32; 3]], amount: f32) -> EngineResult<()> {
    if pixels.is_empty() || amount == 0.0 {
        return Ok(());
    }
    let (source_mean, source_std) = lab_statistics(pixels.iter().map(|p| [p[0], p[1], p[2]]));
    let (target_mean, target_std) = lab_statistics(target.iter().copied());
    let lab_to_lms = LMS_TO_LAB.inverse()?;
    let lms_to_rgb = RGB_TO_LMS.inverse()?;
    for p in pixels {
        let lab = to_oklab([p[0], p[1], p[2]]);
        let matched = std::array::from_fn(|c| {
            let ratio = if source_std[c] > 1e-8 {
                target_std[c] / source_std[c]
            } else {
                1.0
            };
            let transferred = (lab[c] - source_mean[c]) * ratio + target_mean[c];
            lab[c] + f64::from(amount) * (transferred - lab[c])
        });
        let out = lms_to_rgb.apply(lab_to_lms.apply(matched).map(|v| v * v * v));
        for c in 0..3 {
            p[c] = out[c] as f32;
        }
    }
    Ok(())
}

fn finite(values: &[f32]) -> bool {
    values.iter().all(|v| v.is_finite())
}
fn range(v: f32, lo: f32, hi: f32) -> bool {
    v.is_finite() && (lo..=hi).contains(&v)
}
fn signed_power(v: f32, p: f32) -> f32 {
    if p == 1.0 {
        v
    } else {
        v.signum() * v.abs().powf(p)
    }
}
fn luminance(rgb: [f32; 3]) -> f32 {
    0.2627 * rgb[0] + 0.6780 * rgb[1] + 0.0593 * rgb[2]
}

fn shift_unit(v: f32, shift: f32) -> f32 {
    if shift >= 0.0 {
        v + (1.0 - v) * shift
    } else {
        v * (1.0 + shift)
    }
}
fn rgb_to_hsl(rgb: [f32; 3]) -> [f32; 3] {
    let max = rgb.into_iter().fold(f32::NEG_INFINITY, f32::max);
    let min = rgb.into_iter().fold(f32::INFINITY, f32::min);
    let d = max - min;
    let l = (max + min) * 0.5;
    if d <= 1e-10 {
        return [0.0, 0.0, l];
    }
    let h = if max == rgb[0] {
        ((rgb[1] - rgb[2]) / d).rem_euclid(6.0)
    } else if max == rgb[1] {
        (rgb[2] - rgb[0]) / d + 2.0
    } else {
        (rgb[0] - rgb[1]) / d + 4.0
    };
    [h / 6.0, d / (1.0 - (2.0 * l - 1.0).abs()).max(1e-10), l]
}
fn hsl_to_rgb([h, s, l]: [f32; 3]) -> [f32; 3] {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let h = h * 6.0;
    let x = c * (1.0 - (h.rem_euclid(2.0) - 1.0).abs());
    let rgb = match h as u8 {
        0 => [c, x, 0.0],
        1 => [x, c, 0.0],
        2 => [0.0, c, x],
        3 => [0.0, x, c],
        4 => [x, 0.0, c],
        _ => [c, 0.0, x],
    };
    rgb.map(|v| v + l - c * 0.5)
}
fn hue_weight(h: f32, band: usize) -> f32 {
    let d = (h * 6.0 - band as f32).abs();
    (1.0 - d.min(6.0 - d)).max(0.0)
}

// Fritsch-Carlson slope limiter. Each segment stays inside its knot values.
fn curve_slopes(p: &[[f32; 2]]) -> Vec<f32> {
    let d: Vec<f32> = p
        .windows(2)
        .map(|w| (w[1][1] - w[0][1]) / (w[1][0] - w[0][0]))
        .collect();
    let mut m = vec![0.0; p.len()];
    m[0] = d[0];
    m[p.len() - 1] = d[d.len() - 1];
    for i in 1..p.len() - 1 {
        m[i] = (d[i - 1] + d[i]) * 0.5;
    }
    for i in 0..d.len() {
        if d[i] == 0.0 {
            m[i] = 0.0;
            m[i + 1] = 0.0;
        } else {
            let a = m[i] / d[i];
            let b = m[i + 1] / d[i];
            let norm = a.hypot(b);
            if norm > 3.0 {
                let t = 3.0 / norm;
                m[i] = t * a * d[i];
                m[i + 1] = t * b * d[i];
            }
        }
    }
    m
}
fn curve_value(p: &[[f32; 2]], m: &[f32], x: f32) -> f32 {
    if x <= p[0][0] {
        return p[0][1] + (x - p[0][0]) * m[0];
    }
    let last = p.len() - 1;
    if x >= p[last][0] {
        return p[last][1] + (x - p[last][0]) * m[last];
    }
    let i = p
        .partition_point(|p| p[0] <= x)
        .saturating_sub(1)
        .min(last - 1);
    let h = p[i + 1][0] - p[i][0];
    let t = (x - p[i][0]) / h;
    let t2 = t * t;
    let t3 = t2 * t;
    (2.0 * t3 - 3.0 * t2 + 1.0) * p[i][1]
        + (t3 - 2.0 * t2 + t) * h * m[i]
        + (-2.0 * t3 + 3.0 * t2) * p[i + 1][1]
        + (t3 - t2) * h * m[i + 1]
}

#[cfg(test)]
mod tests {
    use super::*;
    fn run(a: Adjustment, rgb: [f32; 3]) -> [f32; 3] {
        let mut p = [[rgb[0], rgb[1], rgb[2], 0.37]];
        a.apply(&mut p).unwrap();
        assert_eq!(p[0][3], 0.37);
        [p[0][0], p[0][1], p[0][2]]
    }
    #[test]
    fn tonal_adjustments_known_values() {
        let levels = Adjustment::Levels {
            input_black: [0.1; 3],
            input_white: [0.9; 3],
            gamma: [2.0; 3],
            output_black: [0.2; 3],
            output_white: [0.8; 3],
        };
        let r = run(levels, [0.1, 0.3, 0.9]);
        for (x, y) in r.into_iter().zip([0.2, 0.5, 0.8]) {
            close(x, y);
        }
        assert_eq!(
            run(Adjustment::Threshold { level: 0.5 }, [0.0, 1.0, 0.0]),
            [1.0; 3]
        );
        assert_eq!(
            run(Adjustment::Posterize { levels: 3 }, [0.1, 0.4, 0.9]),
            [0.0, 0.5, 1.0]
        );
        assert_eq!(run(Adjustment::Invert, [0.0, 0.25, 1.0]), [1.0, 0.75, 0.0]);
        for v in run(Adjustment::Desaturate, [1.0, 0.0, 0.0]) {
            close(v, 0.2627);
        }
        assert_eq!(
            run(
                Adjustment::BrightnessContrast {
                    brightness: 150.0,
                    contrast: 0.0,
                    legacy: true
                },
                [0.0, 0.5, 1.0]
            ),
            [1.0, 1.5, 2.0]
        );
        let r = run(
            Adjustment::BrightnessContrast {
                brightness: 150.0,
                contrast: 0.0,
                legacy: false,
            },
            [0.0, 0.5, 1.0],
        );
        assert_eq!(r[0], 0.0);
        assert_eq!(r[2], 1.0);
        assert!(r[1] > 0.5);
    }
    #[test]
    fn tonal_identities_and_invalid_domains() {
        for a in [
            Adjustment::Levels {
                input_black: [0.0; 3],
                input_white: [1.0; 3],
                gamma: [1.0; 3],
                output_black: [0.0; 3],
                output_white: [1.0; 3],
            },
            Adjustment::Curves {
                points: std::array::from_fn(|_| vec![[0.0, 0.0], [1.0, 1.0]]),
            },
            Adjustment::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
                legacy: false,
            },
            Adjustment::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
                legacy: true,
            },
        ] {
            assert_eq!(run(a, [0.0, 0.25, 1.0]), [0.0, 0.25, 1.0]);
        }
        for a in [
            Adjustment::Posterize { levels: 1 },
            Adjustment::Threshold { level: f32::NAN },
            Adjustment::BrightnessContrast {
                brightness: 0.0,
                contrast: 101.0,
                legacy: false,
            },
            Adjustment::Curves {
                points: std::array::from_fn(|_| vec![[0.0, 0.0], [0.0, 1.0]]),
            },
            Adjustment::Levels {
                input_black: [1.0; 3],
                input_white: [1.0; 3],
                gamma: [1.0; 3],
                output_black: [0.0; 3],
                output_white: [1.0; 3],
            },
        ] {
            assert!(a.validate().is_err(), "{a:?}");
        }
    }
    #[test]
    fn curves_interpolate_knots_without_overshoot() {
        let a = Adjustment::Curves {
            points: std::array::from_fn(|_| vec![[0.0, 0.0], [0.25, 0.7], [0.75, 0.7], [1.0, 1.0]]),
        };
        close(run(a.clone(), [0.25; 3])[0], 0.7);
        let mut previous = 0.0;
        for i in 0..=100 {
            let y = run(a.clone(), [i as f32 / 100.0; 3])[0];
            assert!(y >= previous - 1e-6 && y <= 1.0);
            previous = y;
        }
        close(run(a, [0.5; 3])[0], 0.7);
    }
    #[test]
    fn colour_parameter_identities() {
        let rgb = [0.1, 0.4, 0.8];
        for a in [
            Adjustment::Hsl {
                hue_degrees: 0.0,
                saturation: 0.0,
                lightness: 0.0,
            },
            Adjustment::Vibrance {
                vibrance: 0.0,
                saturation: 0.0,
            },
            Adjustment::PhotoFilter {
                colour: [1.0, 0.0, 0.0],
                density: 0.0,
                preserve_luminosity: true,
            },
            Adjustment::ChannelMixer {
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                constant: [0.0; 3],
            },
            Adjustment::SelectiveColour {
                corrections: [[0.0; 4]; 9],
                relative: true,
            },
        ] {
            let out = run(a, rgb);
            for c in 0..3 {
                close(out[c], rgb[c]);
            }
        }
    }
    #[test]
    fn colour_known_values() {
        let green = run(
            Adjustment::Hsl {
                hue_degrees: 120.0,
                saturation: 0.0,
                lightness: 0.0,
            },
            [1.0, 0.0, 0.0],
        );
        for (x, y) in green.into_iter().zip([0.0, 1.0, 0.0]) {
            close(x, y);
        }
        assert_eq!(
            run(
                Adjustment::Hsl {
                    hue_degrees: 0.0,
                    saturation: -1.0,
                    lightness: 0.0
                },
                [1.0, 0.0, 0.0]
            ),
            [0.5; 3]
        );
        for v in run(
            Adjustment::Vibrance {
                vibrance: 0.5,
                saturation: -1.0,
            },
            [1.0, 0.0, 0.0],
        ) {
            close(v, 0.2627);
        }
        assert_eq!(
            run(
                Adjustment::PhotoFilter {
                    colour: [1.0, 0.5, 0.0],
                    density: 1.0,
                    preserve_luminosity: false
                },
                [0.8; 3]
            ),
            [0.8, 0.4, 0.0]
        );
        let before = [0.1, 0.3, 0.9];
        let out = run(
            Adjustment::PhotoFilter {
                colour: [1.0, 0.4, 0.2],
                density: 0.7,
                preserve_luminosity: true,
            },
            before,
        );
        close(luminance(before), luminance(out));
        assert_eq!(
            run(
                Adjustment::ChannelMixer {
                    matrix: [[0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]],
                    constant: [0.1, 0.0, 0.0]
                },
                [0.2, 0.4, 0.8]
            ),
            [0.5, 0.8, 0.2]
        );
        let stops = vec![(0.0, [1.0, 0.0, 0.0]), (1.0, [0.0, 0.0, 1.0])];
        let out = run(
            Adjustment::GradientMap {
                stops: stops.clone(),
                reverse: false,
            },
            [0.25; 3],
        );
        close(out[0], 0.75);
        close(out[2], 0.25);
        let out = run(
            Adjustment::GradientMap {
                stops,
                reverse: true,
            },
            [0.25; 3],
        );
        close(out[0], 0.25);
        close(out[2], 0.75);
        let mut corrections = [[0.0; 4]; 9];
        corrections[0][0] = 0.25;
        let out = run(
            Adjustment::SelectiveColour {
                corrections,
                relative: false,
            },
            [1.0, 0.0, 0.0],
        );
        close(out[0], 0.75);
        close(out[1], 0.0);
        let out = run(
            Adjustment::BlackWhite {
                weights: [2.0, 1.0, 1.0, 1.0, 1.0, 1.0],
                tint: None,
            },
            [1.0, 0.0, 0.0],
        );
        for x in out {
            close(x, 0.5254);
        }
    }
    #[test]
    fn colour_invalid_domains() {
        for a in [
            Adjustment::Hsl {
                hue_degrees: f32::NAN,
                saturation: 0.0,
                lightness: 0.0,
            },
            Adjustment::Vibrance {
                vibrance: 1.1,
                saturation: 0.0,
            },
            Adjustment::PhotoFilter {
                colour: [1.0; 3],
                density: -0.1,
                preserve_luminosity: false,
            },
            Adjustment::ChannelMixer {
                matrix: [[f32::NAN; 3]; 3],
                constant: [0.0; 3],
            },
            Adjustment::GradientMap {
                stops: vec![],
                reverse: false,
            },
            Adjustment::GradientMap {
                stops: vec![(0.0, [0.0; 3]), (0.0, [1.0; 3])],
                reverse: false,
            },
            Adjustment::SelectiveColour {
                corrections: [[2.0; 4]; 9],
                relative: false,
            },
            Adjustment::BlackWhite {
                weights: [-1.0; 6],
                tint: None,
            },
        ] {
            assert!(a.validate().is_err(), "{a:?}");
        }
    }
    #[test]
    fn global_match_transfers_oklab_mean_and_std() {
        let mut p = [[0.125, 0.125, 0.125, 0.25], [0.512, 0.512, 0.512, 0.75]];
        Adjustment::MatchColour {
            target: vec![[0.008; 3], [0.064; 3]],
            amount: 1.0,
        }
        .apply(&mut p)
        .unwrap();
        for (&a, &b) in p[0][..3].iter().zip(&p[1][..3]) {
            close(a, 0.008);
            close(b, 0.064);
        }
        assert_eq!(p[0][3], 0.25);
        assert_eq!(p[1][3], 0.75);
        let out = run(
            Adjustment::MatchColour {
                target: vec![[0.9, 0.2, 0.4]],
                amount: 1.0,
            },
            [0.1, 0.3, 0.7],
        );
        for (x, y) in out.into_iter().zip([0.9, 0.2, 0.4]) {
            close(x, y);
        }
    }
    #[test]
    fn match_identity_empty_and_invalid() {
        let rgb = [-0.1, 0.3, 2.0];
        assert_eq!(
            run(
                Adjustment::MatchColour {
                    target: vec![[0.0; 3]],
                    amount: 0.0
                },
                rgb
            ),
            rgb
        );
        let mut p = [[0.1, 0.3, 0.7, 0.2], [0.8, 0.2, 0.4, 0.0]];
        let before = p;
        Adjustment::MatchColour {
            target: p.iter().map(|p| [p[0], p[1], p[2]]).collect(),
            amount: 1.0,
        }
        .apply(&mut p)
        .unwrap();
        for (a, b) in p.iter().flatten().zip(before.iter().flatten()) {
            close(*a, *b);
        }
        Adjustment::MatchColour {
            target: vec![[0.0; 3]],
            amount: 1.0,
        }
        .apply(&mut [])
        .unwrap();
        for a in [
            Adjustment::MatchColour {
                target: vec![],
                amount: 1.0,
            },
            Adjustment::MatchColour {
                target: vec![[f32::NAN; 3]],
                amount: 1.0,
            },
            Adjustment::MatchColour {
                target: vec![[0.0; 3]],
                amount: 1.1,
            },
        ] {
            assert!(a.validate().is_err());
        }
    }
    #[test]
    fn neutral_parameters_are_exact_hdr_identities() {
        let original = [
            [-0.3, 0.002, 4.0, 0.0],
            [0.17, 0.8, 1.1, f32::from_bits(0x7fc01234)],
        ];
        for a in [
            Adjustment::default(),
            Adjustment::Levels {
                input_black: [0.0; 3],
                input_white: [1.0; 3],
                gamma: [1.0; 3],
                output_black: [0.0; 3],
                output_white: [1.0; 3],
            },
            Adjustment::Curves {
                points: std::array::from_fn(|_| vec![[0.0, 0.0], [0.3, 0.3], [1.0, 1.0]]),
            },
            Adjustment::Hsl {
                hue_degrees: 360.0,
                saturation: 0.0,
                lightness: 0.0,
            },
            Adjustment::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
                legacy: false,
            },
            Adjustment::BrightnessContrast {
                brightness: 0.0,
                contrast: 0.0,
                legacy: true,
            },
            Adjustment::Vibrance {
                vibrance: 0.0,
                saturation: 0.0,
            },
            Adjustment::PhotoFilter {
                colour: [0.2, 0.4, 0.6],
                density: 0.0,
                preserve_luminosity: true,
            },
            Adjustment::PhotoFilter {
                colour: [1.0; 3],
                density: 0.5,
                preserve_luminosity: false,
            },
            Adjustment::ChannelMixer {
                matrix: [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                constant: [0.0; 3],
            },
            Adjustment::SelectiveColour {
                corrections: [[0.0; 4]; 9],
                relative: false,
            },
            Adjustment::MatchColour {
                target: vec![[0.0; 3]],
                amount: 0.0,
            },
        ] {
            assert!(a.is_identity(), "{a:?}");
            let mut p = original;
            a.apply(&mut p).unwrap();
            for (x, y) in p.iter().flatten().zip(original.iter().flatten()) {
                assert_eq!(x.to_bits(), y.to_bits(), "{a:?}");
            }
        }
        assert!(!Adjustment::Invert.is_identity());
    }
    #[test]
    fn nonfinite_rgb_and_overflow_are_atomic_errors() {
        let mut p = [[0.1, 0.2, 0.3, 0.4], [f32::INFINITY, 0.2, 0.3, 0.4]];
        let before = p;
        assert!(Adjustment::Invert.apply(&mut p).is_err());
        assert_eq!(p, before);
        let a = Adjustment::Exposure {
            stops: 32.0,
            offset: 0.0,
            gamma: 1.0,
        };
        let mut p = [[0.1, 0.2, 0.3, 0.4], [f32::MAX, 0.0, 0.0, 0.7]];
        let before = p;
        assert!(a.apply(&mut p).is_err());
        assert_eq!(p, before);
    }
    #[test]
    fn nonidentity_preserves_alpha_bits_and_accepts_empty_input() {
        let a = Adjustment::Invert;
        let mut p = [[0.1, 0.2, 0.3, f32::from_bits(0x7fc01234)]];
        a.apply(&mut p).unwrap();
        assert_eq!(p[0][3].to_bits(), 0x7fc01234);
        a.apply(&mut []).unwrap();
    }
    #[test]
    fn chromatic_global_match_reproduces_target_statistics() {
        let target = vec![
            [0.7, 0.1, 0.2],
            [0.2, 0.6, 0.3],
            [0.3, 0.1, 0.8],
            [0.2, 0.3, 0.4],
        ];
        let mut p = [
            [0.1, 0.2, 0.3, 0.0],
            [0.2, 0.5, 0.4, 0.2],
            [0.9, 0.1, 0.2, 0.7],
            [0.6, 0.5, 0.2, 1.0],
        ];
        let expected = lab_statistics(target.iter().copied());
        Adjustment::MatchColour {
            target,
            amount: 1.0,
        }
        .apply(&mut p)
        .unwrap();
        let actual = lab_statistics(p.iter().map(|p| [p[0], p[1], p[2]]));
        for c in 0..3 {
            assert!((actual.0[c] - expected.0[c]).abs() < 1e-6);
            assert!((actual.1[c] - expected.1[c]).abs() < 1e-6);
        }
    }
    #[test]
    fn vibrance_targets_muted_colours_and_preserves_luminance() {
        let a = Adjustment::Vibrance {
            vibrance: 1.0,
            saturation: 0.0,
        };
        let rgb = [0.3, 0.4, 0.5];
        let out = run(a.clone(), rgb);
        close(luminance(out), luminance(rgb));
        assert!(out[2] - out[0] > rgb[2] - rgb[0]);
        let out = run(a, [1.0, 0.0, 0.0]);
        for (x, y) in out.into_iter().zip([1.0, 0.0, 0.0]) {
            close(x, y);
        }
    }
    #[test]
    fn selective_relative_mode_and_bw_tint() {
        let mut corrections = [[0.0; 4]; 9];
        corrections[7][0] = 0.2;
        let absolute = run(
            Adjustment::SelectiveColour {
                corrections,
                relative: false,
            },
            [0.5; 3],
        );
        let relative = run(
            Adjustment::SelectiveColour {
                corrections,
                relative: true,
            },
            [0.5; 3],
        );
        close(absolute[0], 0.3);
        close(relative[0], 0.4);
        let out = run(
            Adjustment::BlackWhite {
                weights: [1.0; 6],
                tint: Some([1.0, 0.5, 0.0]),
            },
            [0.5; 3],
        );
        close(out[0], 0.5);
        close(out[1], 0.25);
        close(out[2], 0.0);
    }
    fn close(a: f32, b: f32) {
        assert!((a - b).abs() < 2e-5, "{a} != {b}");
    }
    #[test]
    fn exposure_identity_and_gain_preserve_hdr_and_alpha() {
        let original = [[-0.1, 0.2, 2.0, 0.3], [0.0, 0.5, 1.0, 0.0]];
        let mut p = original;
        Adjustment::default().apply(&mut p).unwrap();
        assert_eq!(p, original);
        Adjustment::Exposure {
            stops: 1.0,
            offset: 0.1,
            gamma: 1.0,
        }
        .apply(&mut p)
        .unwrap();
        close(p[0][0], -0.1);
        close(p[0][1], 0.5);
        close(p[0][2], 4.1);
        assert_eq!(p[0][3], original[0][3]);
        assert_eq!(p[1][3], 0.0);
    }
    #[test]
    fn invalid_exposure_does_not_mutate() {
        for adjustment in [
            Adjustment::Exposure {
                stops: f32::NAN,
                offset: 0.0,
                gamma: 1.0,
            },
            Adjustment::Exposure {
                stops: 0.0,
                offset: 0.0,
                gamma: 0.0,
            },
        ] {
            let mut p = [[0.1, 0.2, 0.3, 0.4]];
            let before = p;
            assert!(adjustment.validate().is_err());
            assert!(adjustment.apply(&mut p).is_err());
            assert_eq!(p, before);
        }
    }
}
