//! Adjustment layers: per-pixel functions of the composite below
//! (spec 02 §7). Formulas are in COMPOSITOR.md §4.
//!
//! These are display-referred operators on the document's own encoding.
//! pipeline-cpu's operators are scene-referred linear Rec.2020 and pull in
//! the raw decoder, so they are not reused here (see COMPOSITOR.md §9).

use serde::{Deserialize, Serialize};

#[path = "adjust/presets.rs"]
mod presets;
pub use presets::PhotoFilterPreset;

#[path = "adjust/color.rs"]
pub(crate) mod color;
#[path = "adjust/hdr.rs"]
pub mod hdr;
#[path = "adjust/icc.rs"]
mod icc;
#[path = "adjust/lookup.rs"]
mod lookup;
#[path = "adjust/shadows.rs"]
pub mod shadows;
#[path = "adjust/statistics.rs"]
mod statistics;

/// Color space used between gradient stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GradientMethod {
    /// Interpolate encoded RGB.
    Classic,
    /// Interpolate Oklab.
    Perceptual,
    /// Interpolate decoded linear sRGB.
    Linear,
}
/// Histogram-derived automatic correction mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AutoMode {
    /// Stretch each channel independently.
    Tone,
    /// Stretch with pooled channel endpoints.
    Contrast,
    /// Stretch and neutralize channel means.
    Color,
}

/// Opt-in versioned interchange. Existing document enum encoding is unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VersionedAdjustment {
    /// Schema version; checked decoding accepts only 1.
    pub version: u32,
    /// Parameters; direct serde decoding does not validate them.
    pub adjustment: Adjustment,
}
impl Adjustment {
    /// Check finite parameters and structural invariants before rendering.
    /// Direct enum deserialization is intentionally unchanged; callers must
    /// validate it before compilation. Existing finite clamping stays intact.
    pub fn validate(&self) -> engine_api::EngineResult<()> {
        let finite = |v: &[f32]| v.iter().all(|x| x.is_finite());
        let level = |c: &LevelsChannel| {
            finite(&[c.in_black, c.in_white, c.gamma, c.out_black, c.out_white])
        };
        let valid = match self {
            Self::Vibrance {
                vibrance,
                saturation,
            } => finite(&[*vibrance, *saturation]),
            Self::ColorBalance {
                shadows,
                midtones,
                highlights,
                ..
            } => finite(shadows) && finite(midtones) && finite(highlights),
            Self::BlackWhite { sliders, tint } => {
                finite(sliders) && tint.as_ref().is_none_or(|v| finite(v))
            }
            Self::PhotoFilter { color, density, .. } => finite(color) && density.is_finite(),
            Self::GradientMap { stops, .. } => stops.iter().all(|v| finite(v)),
            Self::SelectiveColor { colors, .. } => colors.iter().all(|v| finite(v)),
            Self::Equalize { maps } => maps.iter().all(|v| finite(v)),
            Self::Auto {
                black,
                white,
                gamma,
                ..
            } => finite(black) && finite(white) && finite(gamma),
            Self::MatchColor {
                source_layer,
                source_mean,
                source_std,
                target_mean,
                target_std,
                luminance,
                color_intensity,
                fade,
            } => {
                *source_layer != 0
                    && finite(source_mean)
                    && finite(source_std)
                    && finite(target_mean)
                    && finite(target_std)
                    && source_std.iter().chain(target_std).all(|v| *v >= 0.0)
                    && finite(&[*luminance, *color_intensity, *fade])
            }
            Self::ReplaceColor {
                color,
                fuzziness,
                hue,
                saturation,
                lightness,
            } => finite(color) && finite(&[*fuzziness, *hue, *saturation, *lightness]),
            Self::ColorLookup { size, data } => lookup::valid(*size, data),
            Self::ShadowsHighlights { settings } => return settings.validate(),
            Self::HdrToning { settings } => return settings.validate(),
            Self::BrightnessContrast {
                brightness,
                contrast,
                ..
            } => finite(&[*brightness, *contrast]),
            Self::Levels { master, rgb } => level(master) && rgb.iter().all(level),
            Self::Curves { master, rgb } => std::iter::once(master)
                .chain(rgb)
                .all(|c| c.0.iter().all(|p| finite(p))),
            Self::HueSaturation {
                hue,
                saturation,
                lightness,
                ..
            } => finite(&[*hue, *saturation, *lightness]),
            Self::Exposure {
                exposure,
                offset,
                gamma,
            } => finite(&[*exposure, *offset, *gamma]),
            Self::Threshold { level } => level.is_finite(),
            Self::ChannelMixer {
                matrix, constant, ..
            } => matrix.iter().all(|r| finite(r)) && finite(constant),
            Self::Invert | Self::Desaturate | Self::Posterize { .. } => true,
        };
        if valid {
            Ok(())
        } else {
            Err(engine_api::EngineError::invalid(
                "adjustment",
                "nonfinite parameter or invalid structure",
            ))
        }
    }
    /// Serialize the unchanged enum inside the version-1 interchange envelope.
    pub fn to_versioned_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string(&VersionedAdjustment {
            version: 1,
            adjustment: self.clone(),
        })
    }
    /// Decode version 1 and reject nonfinite parameters or invalid structures.
    pub fn from_versioned_json(s: &str) -> Result<Self, serde_json::Error> {
        let envelope: VersionedAdjustment = serde_json::from_str(s)?;
        if envelope.version != 1 {
            return Err(<serde_json::Error as serde::de::Error>::custom(
                "unsupported adjustment version",
            ));
        }
        envelope
            .adjustment
            .validate()
            .map_err(<serde_json::Error as serde::de::Error>::custom)?;
        Ok(envelope.adjustment)
    }
}

#[cfg(test)]
#[path = "adjust/extended_tests.rs"]
mod extended_tests;

/// Levels for one channel.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LevelsChannel {
    /// Input black point `ib`.
    pub in_black: f32,
    /// Input white point `iw`.
    pub in_white: f32,
    /// Midtone gamma `γ` (> 0; 1 = neutral).
    pub gamma: f32,
    /// Output black `ob`.
    pub out_black: f32,
    /// Output white `ow`.
    pub out_white: f32,
}

impl Default for LevelsChannel {
    fn default() -> Self {
        Self {
            in_black: 0.0,
            in_white: 1.0,
            gamma: 1.0,
            out_black: 0.0,
            out_white: 1.0,
        }
    }
}

impl LevelsChannel {
    /// `ob + (ow − ob)·clamp((v − ib)/(iw − ib), 0, 1)^(1/γ)`.
    pub fn apply(&self, v: f32) -> f32 {
        let span = self.in_white - self.in_black;
        let t = if span.abs() < 1e-9 {
            if v >= self.in_white { 1.0 } else { 0.0 }
        } else {
            ((v - self.in_black) / span).clamp(0.0, 1.0)
        };
        let g = if self.gamma > 0.0 { self.gamma } else { 1.0 };
        let t = if g == 1.0 { t } else { t.powf(1.0 / g) };
        self.out_black + (self.out_white - self.out_black) * t
    }
}

/// A curve through control points `(input, output)` in `[0, 1]`,
/// evaluated as a monotone (Fritsch–Carlson) cubic Hermite spline.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct Curve(pub Vec<[f32; 2]>);

impl Curve {
    /// True when the curve is the identity (no points, or the diagonal).
    pub fn is_identity(&self) -> bool {
        self.0.is_empty() || self.0 == [[0.0, 0.0], [1.0, 1.0]]
    }

    /// Samples the curve into a `n`-entry LUT over `[0, 1]`.
    pub fn lut(&self, n: usize) -> Vec<f32> {
        let mut pts = self.0.clone();
        pts.retain(|p| p[0].is_finite() && p[1].is_finite());
        pts.sort_by(|a, b| a[0].total_cmp(&b[0]));
        pts.dedup_by(|a, b| a[0] == b[0]);
        if pts.len() < 2 {
            let c = pts.first().map(|p| p[1]);
            return (0..n)
                .map(|i| c.unwrap_or(i as f32 / (n - 1) as f32))
                .collect();
        }
        let k = pts.len();
        let d: Vec<f32> = (0..k - 1)
            .map(|i| (pts[i + 1][1] - pts[i][1]) / (pts[i + 1][0] - pts[i][0]))
            .collect();
        let mut m = vec![0.0f32; k];
        m[0] = d[0];
        m[k - 1] = d[k - 2];
        for i in 1..k - 1 {
            m[i] = if d[i - 1] * d[i] <= 0.0 {
                0.0
            } else {
                (d[i - 1] + d[i]) / 2.0
            };
        }
        for i in 0..k - 1 {
            if d[i] == 0.0 {
                m[i] = 0.0;
                m[i + 1] = 0.0;
            } else {
                let (a, b) = (m[i] / d[i], m[i + 1] / d[i]);
                let s = a * a + b * b;
                if s > 9.0 {
                    let t = 3.0 / s.sqrt();
                    m[i] = t * a * d[i];
                    m[i + 1] = t * b * d[i];
                }
            }
        }
        (0..n)
            .map(|j| {
                let x = j as f32 / (n - 1) as f32;
                if x <= pts[0][0] {
                    return pts[0][1].clamp(0.0, 1.0);
                }
                if x >= pts[k - 1][0] {
                    return pts[k - 1][1].clamp(0.0, 1.0);
                }
                let i = pts.partition_point(|p| p[0] <= x) - 1;
                let h = pts[i + 1][0] - pts[i][0];
                let t = (x - pts[i][0]) / h;
                let (t2, t3) = (t * t, t * t * t);
                let y = (2.0 * t3 - 3.0 * t2 + 1.0) * pts[i][1]
                    + (t3 - 2.0 * t2 + t) * h * m[i]
                    + (-2.0 * t3 + 3.0 * t2) * pts[i + 1][1]
                    + (t3 - t2) * h * m[i + 1];
                y.clamp(0.0, 1.0)
            })
            .collect()
    }
}

/// Adjustment layer parameters.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Adjustment {
    /// Oklab chroma scale with inverse-saturation and skin-hue protection.
    Vibrance {
        /// Chroma boost percent, conventionally -100..100.
        vibrance: f32,
        /// Saturation shift percent, conventionally -100..100.
        saturation: f32,
    },
    /// Percent RGB offsets by tonal range.
    ColorBalance {
        /// Shadow RGB offsets in percent.
        shadows: [f32; 3],
        /// Midtone RGB offsets in percent.
        midtones: [f32; 3],
        /// Highlight RGB offsets in percent.
        highlights: [f32; 3],
        /// Restore input weighted luminance after filtering.
        preserve_luminosity: bool,
    },
    /// Hue-band percent weights in R,Y,G,C,B,M order; optional RGB tint.
    BlackWhite {
        /// R,Y,G,C,B,M hue-band weights in percent.
        sliders: [f32; 6],
        /// Optional encoded RGB tint.
        tint: Option<[f32; 3]>,
    },
    /// Multiply by an encoded RGB filter, blended by density.
    PhotoFilter {
        /// Encoded RGB filter or selection color; normally [0,1].
        color: [f32; 3],
        /// Filter strength in percent, 0..100.
        density: f32,
        /// Restore input weighted luminance after filtering.
        preserve_luminosity: bool,
    },
    /// Stops are [position, red, green, blue], in document encoding.
    GradientMap {
        /// Position plus encoded RGB; compiled in sorted order.
        stops: Vec<[f32; 4]>,
        /// Add deterministic spatial sub-byte noise at the requested mip level.
        dither: bool,
        /// Reverse the luminance-to-gradient mapping.
        reverse: bool,
        /// Interpolation color space.
        method: GradientMethod,
    },
    /// CMYK percent corrections for R,Y,G,C,B,M,white,neutral,black.
    SelectiveColor {
        /// R,Y,G,C,B,M,white,neutral,black CMYK percent offsets.
        colors: [[f32; 4]; 9],
        /// Use absolute rather than available-ink-relative corrections.
        absolute: bool,
    },
    /// Neutral gray at HSL lightness.
    Desaturate,
    /// Frozen histogram equalization.
    Equalize {
        /// Frozen per-channel CDF maps over [0,1].
        maps: [Vec<f32>; 3],
    },
    /// Frozen automatic tone, contrast or color correction.
    Auto {
        /// Histogram analysis mode retained for interchange.
        mode: AutoMode,
        /// Per-channel input black points.
        black: [f32; 3],
        /// Per-channel input white points.
        white: [f32; 3],
        /// Per-channel midtone gamma.
        gamma: [f32; 3],
    },
    /// Frozen CIE Lab D65 statistics; source_layer preserves source identity.
    MatchColor {
        /// Identity of the source whose statistics were frozen.
        source_layer: u64,
        /// Source CIE Lab D65 population mean.
        source_mean: [f32; 3],
        /// Source CIE Lab D65 population standard deviation.
        source_std: [f32; 3],
        /// Destination CIE Lab D65 population mean.
        target_mean: [f32; 3],
        /// Destination CIE Lab D65 population standard deviation.
        target_std: [f32; 3],
        /// Luminance transfer strength, neutral at 100.
        luminance: f32,
        /// Chroma transfer strength, neutral at 100.
        color_intensity: f32,
        /// Blend back to original, 0..100 percent.
        fade: f32,
    },
    /// Normalized encoded-RGB distance selection with HSL shifts.
    ReplaceColor {
        /// Encoded RGB filter or selection color; normally [0,1].
        color: [f32; 3],
        /// Fuzziness 0..200 maps to normalized RGB distance radius 0..1.
        fuzziness: f32,
        /// Hue rotation in degrees.
        hue: f32,
        /// Saturation shift percent, conventionally -100..100.
        saturation: f32,
        /// Lightness shift in percent.
        lightness: f32,
    },
    /// Red-fastest cube: r + size * (g + size * b).
    ColorLookup {
        /// Cube edge length; checked constructors require 2..=256.
        size: u32,
        /// Red-fastest finite RGB cube samples.
        data: Vec<[f32; 3]>,
    },
    /// Local tonal operator; requires neighborhood rendering.
    ShadowsHighlights {
        /// Validated local neighborhood operator controls.
        settings: shadows::ShadowsHighlights,
    },
    /// Native HDR tone mapping (native document interchange only).
    HdrToning {
        /// Validated tone mapping method and controls.
        settings: hdr::HdrToning,
    },
    /// Endpoint-preserving tone curve, or legacy affine correction.
    BrightnessContrast {
        /// Brightness percent-like control, conventionally -150..150.
        brightness: f32,
        /// Contrast control, conventionally -100..100.
        contrast: f32,
        /// Use the legacy affine instead of modern tone curve.
        legacy: bool,
    },
    /// Levels: per channel, then the composite ("RGB") channel.
    Levels {
        /// Applied after the channels.
        master: LevelsChannel,
        /// Red, green, blue.
        rgb: [LevelsChannel; 3],
    },
    /// Curves: per channel, then the composite curve.
    Curves {
        /// Applied after the channels.
        master: Curve,
        /// Red, green, blue.
        rgb: [Curve; 3],
    },
    /// Hue/Saturation (master range only) or Colorize.
    HueSaturation {
        /// Degrees, −180..180.
        hue: f32,
        /// −100..100.
        saturation: f32,
        /// −100..100.
        lightness: f32,
        /// Replace hue/saturation instead of shifting.
        colorize: bool,
    },
    /// Exposure / offset / gamma correction.
    Exposure {
        /// Stops.
        exposure: f32,
        /// Added after the gain.
        offset: f32,
        /// Gamma correction (> 0).
        gamma: f32,
    },
    /// `1 − v`.
    Invert,
    /// `levels` output levels per channel (2..=255).
    Posterize {
        /// Level count.
        levels: u32,
    },
    /// Black/white at a luminance threshold.
    Threshold {
        /// Threshold in `[0, 1]` (Photoshop's 1..255 / 255).
        level: f32,
    },
    /// 3×3 channel mix plus constant.
    ChannelMixer {
        /// Output row per channel, as fractions (100 % = 1).
        matrix: [[f32; 3]; 3],
        /// Added per output channel.
        constant: [f32; 3],
        /// Use row 0 for all channels.
        monochrome: bool,
    },
}

/// A compiled adjustment, cheap to evaluate per pixel.
pub(crate) enum Compiled<'a> {
    Luts([Vec<f32>; 3], Vec<f32>),
    Channels([Vec<f32>; 3]),
    Auto(&'a Adjustment, [Vec<f32>; 3]),
    Direct(&'a Adjustment),
    Hdr(&'a hdr::HdrToning, Vec<f32>),
    Gradient(Vec<[f32; 4]>, bool, bool, GradientMethod),
}

const LUT_N: usize = 4096;

#[inline(always)]
fn lut_eval(lut: &[f32], v: f32) -> f32 {
    if lut.is_empty() {
        return v;
    }
    if lut.len() == 1 {
        return lut[0];
    }
    let x = v.clamp(0.0, 1.0) * (lut.len() - 1) as f32;
    let i = (x as usize).min(lut.len() - 2);
    let f = x - i as f32;
    lut[i] + (lut[i + 1] - lut[i]) * f
}

fn rgb_to_hsl(c: [f32; 3]) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let l = (mx + mn) / 2.0;
    if mx == mn {
        return [0.0, 0.0, l];
    }
    let d = mx - mn;
    let s = if l > 0.5 {
        d / (2.0 - mx - mn)
    } else {
        d / (mx + mn)
    };
    let h = if mx == c[0] {
        (c[1] - c[2]) / d + if c[1] < c[2] { 6.0 } else { 0.0 }
    } else if mx == c[1] {
        (c[2] - c[0]) / d + 2.0
    } else {
        (c[0] - c[1]) / d + 4.0
    };
    [h / 6.0, s, l]
}

fn hsl_to_rgb(h: [f32; 3]) -> [f32; 3] {
    let [hh, s, l] = h;
    if s <= 0.0 {
        return [l, l, l];
    }
    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;
    let f = |t: f32| {
        let t = t.rem_euclid(1.0);
        if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 0.5 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        }
    };
    [f(hh + 1.0 / 3.0), f(hh), f(hh - 1.0 / 3.0)]
}

impl Adjustment {
    pub(crate) fn compile(&self) -> Compiled<'_> {
        match self {
            // Native modern brightness is the piecewise-linear 4096-sample
            // curve. CPU-generated knots are shared verbatim with the GPU.
            Adjustment::BrightnessContrast { legacy: false, .. } => {
                let l: Vec<f32> = (0..LUT_N)
                    .map(|i| apply_direct(self, [i as f32 / (LUT_N - 1) as f32; 3])[0])
                    .collect();
                Compiled::Channels([l.clone(), l.clone(), l])
            }
            Adjustment::Auto { gamma, .. } => Compiled::Auto(
                self,
                std::array::from_fn(|j| {
                    let g = if gamma[j] > 0.0 { gamma[j] } else { 1.0 };
                    (0..LUT_N)
                        .map(|i| (i as f32 / (LUT_N - 1) as f32).powf(1.0 / g))
                        .collect()
                }),
            ),
            Adjustment::Levels { master, rgb } => {
                let ch = |c: &LevelsChannel| -> Vec<f32> {
                    (0..LUT_N)
                        .map(|i| c.apply(i as f32 / (LUT_N - 1) as f32))
                        .collect()
                };
                Compiled::Luts([ch(&rgb[0]), ch(&rgb[1]), ch(&rgb[2])], ch(master))
            }
            Adjustment::Curves { master, rgb } => Compiled::Luts(
                [rgb[0].lut(LUT_N), rgb[1].lut(LUT_N), rgb[2].lut(LUT_N)],
                master.lut(LUT_N),
            ),
            Adjustment::GradientMap {
                stops,
                dither,
                reverse,
                method,
            } => {
                let mut stops = stops.clone();
                stops.retain(|p| p.iter().all(|v| v.is_finite()));
                stops.sort_by(|a, b| a[0].total_cmp(&b[0]));
                stops.dedup_by(|a, b| a[0] == b[0]);
                Compiled::Gradient(stops, *dither, *reverse, *method)
            }
            Adjustment::ColorLookup { size, data } => {
                assert!(
                    lookup::valid(*size, data),
                    "invalid ColorLookup: use checked constructors or validate before rendering"
                );
                Compiled::Direct(self)
            }
            Adjustment::HdrToning { settings } => Compiled::Hdr(settings, settings.curve_lut()),
            other => Compiled::Direct(other),
        }
    }
}

impl Compiled<'_> {
    /// The adjusted colour at the origin (straight RGB in, straight RGB out).
    #[allow(dead_code)] // Tests and legacy callers without spatial coordinates.
    #[inline]
    pub(crate) fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        self.apply_at(c, 0, 0)
    }

    /// Adjust at absolute integer pixel coordinates in the requested mip level.
    /// Coordinates must include the tile origin so spatial dither never resets.
    #[inline]
    pub(crate) fn apply_at(&self, c: [f32; 3], x: u32, y: u32) -> [f32; 3] {
        match self {
            Compiled::Luts(ch, master) => {
                let mut o = [0.0; 3];
                for i in 0..3 {
                    o[i] = lut_eval(master, lut_eval(&ch[i], c[i]));
                }
                o
            }
            Compiled::Auto(a, ch) => {
                let Adjustment::Auto {
                    black,
                    white,
                    gamma,
                    ..
                } = a
                else {
                    unreachable!()
                };
                std::array::from_fn(|i| {
                    let t = LevelsChannel {
                        in_black: black[i],
                        in_white: white[i],
                        ..Default::default()
                    }
                    .apply(c[i]);
                    if gamma[i] <= 0.0 || gamma[i] == 1.0 {
                        t
                    } else {
                        lut_eval(&ch[i], t)
                    }
                })
            }
            Compiled::Channels(ch) => std::array::from_fn(|i| lut_eval(&ch[i], c[i])),
            Compiled::Direct(a) => apply_direct(a, c),
            Compiled::Hdr(settings, curve) => settings.map_with_lut(c, hdr::luminance(c), curve),
            Compiled::Gradient(stops, dither, reverse, method) => {
                gradient(stops, *dither, *reverse, *method, c, [x, y])
            }
        }
    }
}

fn gradient(
    stops: &[[f32; 4]],
    dither: bool,
    reverse: bool,
    method: GradientMethod,
    c: [f32; 3],
    position: [u32; 2],
) -> [f32; 3] {
    let mut t = luma(c).clamp(0.0, 1.0);
    if reverse {
        t = 1.0 - t;
    }
    if dither {
        // Spatial noise, independent of content, tiles and processing order.
        // Keep wrapping integer math identical to resident/adjustments.wgsl.
        let mut h = position[0] ^ position[1].wrapping_mul(0x9e3779b9);
        h = (h ^ (h >> 16)).wrapping_mul(0x7feb352d);
        h = (h ^ (h >> 15)).wrapping_mul(0x846ca68b);
        h ^= h >> 16;
        t = (t + ((h & 65535) as f32 / 65535.0 - 0.5) / 255.0).clamp(0.0, 1.0);
    }
    if stops.is_empty() {
        return [t; 3];
    }
    let rgb = |p: [f32; 4]| [p[1], p[2], p[3]];
    if t <= stops[0][0] {
        return rgb(stops[0]);
    }
    let i = stops.partition_point(|p| p[0] <= t);
    if i == stops.len() {
        return rgb(stops[i - 1]);
    }
    let a = rgb(stops[i - 1]);
    let b = rgb(stops[i]);
    let f = ((t - stops[i - 1][0]) / (stops[i][0] - stops[i - 1][0])).clamp(0.0, 1.0);
    let lerp = |a: [f32; 3], b: [f32; 3]| std::array::from_fn(|j| a[j] + f * (b[j] - a[j]));
    match method {
        GradientMethod::Classic => lerp(a, b),
        GradientMethod::Linear => {
            lerp(a.map(color::decode), b.map(color::decode)).map(color::encode)
        }
        GradientMethod::Perceptual => color::from_oklab(lerp(color::oklab(a), color::oklab(b))),
    }
    .map(|v| v.clamp(0.0, 1.0))
}
fn luma(c: [f32; 3]) -> f32 {
    0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2]
}
fn preserve_luma(c: [f32; 3], y: f32) -> [f32; 3] {
    let y = y.clamp(0.0, 1.0);
    let old = luma(c);
    if old.abs() < 1e-8 {
        return [y; 3];
    }
    let c = c.map(|v| v * y / old);
    let mut scale = 1.0_f32;
    for v in c {
        if v > 1.0 {
            scale = scale.min((1.0 - y) / (v - y));
        } else if v < 0.0 {
            scale = scale.min(y / (y - v));
        }
    }
    c.map(|v| y + (v - y) * scale)
}
fn apply_direct(a: &Adjustment, c: [f32; 3]) -> [f32; 3] {
    match *a {
        Adjustment::MatchColor {
            source_mean,
            source_std,
            target_mean,
            target_std,
            luminance,
            color_intensity,
            fade,
            ..
        } => {
            let f = (fade / 100.0).clamp(0.0, 1.0);
            if f == 1.0 {
                return c;
            }
            let lab = color::lab(c);
            let mut mapped = std::array::from_fn(|i| {
                (lab[i] - target_mean[i]) * source_std[i].max(0.0) / target_std[i].max(1e-6)
                    + source_mean[i]
            });
            mapped[0] *= (luminance / 100.0).clamp(0.0, 2.0);
            mapped[1] *= (color_intensity / 100.0).clamp(0.0, 2.0);
            mapped[2] *= (color_intensity / 100.0).clamp(0.0, 2.0);
            let o = color::from_lab(mapped).map(|v| v.clamp(0.0, 1.0));
            std::array::from_fn(|i| o[i] * (1.0 - f) + c[i] * f)
        }
        Adjustment::Auto {
            black,
            white,
            gamma,
            ..
        } => std::array::from_fn(|i| {
            LevelsChannel {
                in_black: black[i],
                in_white: white[i],
                gamma: gamma[i],
                ..Default::default()
            }
            .apply(c[i])
        }),
        Adjustment::Equalize { ref maps } => std::array::from_fn(|i| lut_eval(&maps[i], c[i])),
        Adjustment::Vibrance {
            vibrance,
            saturation,
        } => {
            if vibrance == 0.0 && saturation == 0.0 {
                return c;
            }
            let [h, s, _] = rgb_to_hsl(c);
            let dist = (h - 1.0 / 12.0).abs();
            let dist = dist.min(1.0 - dist);
            let skin = (1.0 - dist / (1.0 / 12.0)).max(0.0);
            let scale = (1.0 + (saturation / 100.0).clamp(-1.0, 1.0))
                * (1.0 + (vibrance / 100.0).clamp(-1.0, 1.0) * (1.0 - s) * (1.0 - 0.75 * skin));
            let mut lab = color::oklab(c);
            lab[1] *= scale;
            lab[2] *= scale;
            color::from_oklab(lab).map(|v| v.clamp(0.0, 1.0))
        }
        Adjustment::ReplaceColor {
            color,
            fuzziness,
            hue,
            saturation,
            lightness,
        } => {
            let distance =
                ((c[0] - color[0]).powi(2) + (c[1] - color[1]).powi(2) + (c[2] - color[2]).powi(2))
                    .sqrt()
                    / 3.0_f32.sqrt();
            let radius = (fuzziness / 200.0).clamp(0.0, 1.0);
            let weight = if radius <= 0.0 {
                if distance <= 1e-7 { 1.0 } else { 0.0 }
            } else {
                (1.0 - distance / radius).clamp(0.0, 1.0)
            };
            let o = apply_direct(
                &Adjustment::HueSaturation {
                    hue,
                    saturation,
                    lightness,
                    colorize: false,
                },
                c,
            );
            std::array::from_fn(|i| c[i] + weight * (o[i] - c[i]))
        }
        Adjustment::SelectiveColor { colors, absolute } => {
            let mx = c[0].max(c[1]).max(c[2]);
            let mn = c[0].min(c[1]).min(c[2]);
            let chroma = (mx - mn).clamp(0.0, 1.0);
            let h = rgb_to_hsl(c)[0] * 6.0;
            let y = luma(c).clamp(0.0, 1.0);
            let mut weights = [0.0; 9];
            for (i, w) in weights[..6].iter_mut().enumerate() {
                let d = (h - i as f32).abs();
                *w = chroma * (1.0 - d.min(6.0 - d)).max(0.0);
            }
            weights[6] = (1.0 - chroma) * (2.0 * y - 1.0).max(0.0);
            weights[8] = (1.0 - chroma) * (1.0 - 2.0 * y).max(0.0);
            weights[7] = (1.0 - chroma) * (1.0 - (2.0 * y - 1.0).abs());
            std::array::from_fn(|i| {
                let mut v = c[i];
                for j in 0..9 {
                    let correction = colors[j][i].clamp(-100.0, 100.0) / 100.0;
                    let key = colors[j][3].clamp(-100.0, 100.0) / 100.0;
                    v -= weights[j]
                        * (correction * if absolute { 1.0 } else { 1.0 - c[i] }
                            + key * if absolute { 1.0 } else { 1.0 - mx });
                }
                v.clamp(0.0, 1.0)
            })
        }
        Adjustment::BlackWhite { sliders, tint } => {
            let h = rgb_to_hsl(c)[0] * 6.0;
            let i = h.floor() as usize % 6;
            let f = h.fract();
            let mx = c[0].max(c[1]).max(c[2]);
            let mn = c[0].min(c[1]).min(c[2]);
            let y = (mn + (mx - mn) * (sliders[i] * (1.0 - f) + sliders[(i + 1) % 6] * f) / 100.0)
                .clamp(0.0, 1.0);
            if let Some(tint) = tint {
                let hs = rgb_to_hsl(tint);
                hsl_to_rgb([hs[0], hs[1], y])
            } else {
                [y; 3]
            }
        }
        Adjustment::ColorBalance {
            shadows,
            midtones,
            highlights,
            preserve_luminosity,
        } => {
            let y = luma(c).clamp(0.0, 1.0);
            let o = std::array::from_fn(|i| {
                c[i] + ((1.0 - y).powi(2) * shadows[i].clamp(-100.0, 100.0)
                    + 2.0 * y * (1.0 - y) * midtones[i].clamp(-100.0, 100.0)
                    + y * y * highlights[i].clamp(-100.0, 100.0))
                    / 100.0
            });
            if preserve_luminosity {
                preserve_luma(o, y)
            } else {
                o.map(|v| v.clamp(0.0, 1.0))
            }
        }
        Adjustment::PhotoFilter {
            color,
            density,
            preserve_luminosity,
        } => {
            let d = (density / 100.0).clamp(0.0, 1.0);
            let o = std::array::from_fn(|i| c[i] * (1.0 - d + d * color[i].clamp(0.0, 1.0)));
            if preserve_luminosity {
                preserve_luma(o, luma(c))
            } else {
                o
            }
        }
        Adjustment::Desaturate => [rgb_to_hsl(c)[2]; 3],
        Adjustment::BrightnessContrast {
            brightness,
            contrast,
            legacy,
        } => {
            let b = brightness.clamp(-150.0, 150.0) / 150.0;
            let k = contrast.clamp(-100.0, 100.0) / 100.0;
            c.map(|v| {
                if legacy {
                    ((v - 0.5) * (1.0 + k) + 0.5 + b).clamp(0.0, 1.0)
                } else {
                    let v = v.clamp(0.0, 1.0);
                    let t = v + b * v * (1.0 - v);
                    let p = k.exp2();
                    let x = t.powf(p);
                    x / (x + (1.0 - t).powf(p))
                }
            })
        }
        Adjustment::Invert => [1.0 - c[0], 1.0 - c[1], 1.0 - c[2]],
        Adjustment::Posterize { levels } => {
            let n = levels.clamp(2, 255) as f32;
            c.map(|v| ((v.clamp(0.0, 1.0) * n).floor().min(n - 1.0)) / (n - 1.0))
        }
        Adjustment::Threshold { level } => {
            let y = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
            let o = if y >= level { 1.0 } else { 0.0 };
            [o; 3]
        }
        Adjustment::Exposure {
            exposure,
            offset,
            gamma,
        } => {
            let g = if gamma > 0.0 { 1.0 / gamma } else { 1.0 };
            let k = exposure.exp2();
            c.map(|v| (v * k + offset).max(0.0).powf(g))
        }
        Adjustment::ChannelMixer {
            matrix,
            constant,
            monochrome,
        } => {
            let row = |r: usize| {
                matrix[r][0] * c[0] + matrix[r][1] * c[1] + matrix[r][2] * c[2] + constant[r]
            };
            if monochrome {
                [row(0); 3]
            } else {
                [row(0), row(1), row(2)]
            }
        }
        Adjustment::HueSaturation {
            hue,
            saturation,
            lightness,
            colorize,
        } => {
            let mut hsl = rgb_to_hsl(c);
            let s = (saturation / 100.0).clamp(-1.0, 1.0);
            if colorize {
                let l = 0.299 * c[0] + 0.587 * c[1] + 0.114 * c[2];
                hsl = [(hue / 360.0).rem_euclid(1.0), s.max(0.0), l];
            } else {
                hsl[0] = (hsl[0] + hue / 360.0).rem_euclid(1.0);
                hsl[1] = if s < 0.0 {
                    hsl[1] * (1.0 + s)
                } else {
                    hsl[1] + (1.0 - hsl[1]) * s
                };
            }
            let rgb = hsl_to_rgb(hsl);
            let k = (lightness / 100.0).clamp(-1.0, 1.0);
            rgb.map(|v| {
                if k < 0.0 {
                    v * (1.0 + k)
                } else {
                    v + (1.0 - v) * k
                }
            })
        }
        Adjustment::Levels { .. } | Adjustment::Curves { .. } | Adjustment::GradientMap { .. } => {
            unreachable!("adjustment must be compiled")
        }
        Adjustment::ShadowsHighlights { .. } => {
            panic!("ShadowsHighlights requires neighborhood execution")
        }
        Adjustment::ColorLookup { size, ref data } => lookup::sample(size, data, c),
        Adjustment::HdrToning { ref settings } => settings.map_rgb(c, hdr::luminance(c)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn levels_formula() {
        let l = LevelsChannel {
            in_black: 0.2,
            in_white: 0.8,
            gamma: 2.0,
            out_black: 0.1,
            out_white: 0.9,
        };
        // t = (0.5-0.2)/0.6 = 0.5; 0.5^(1/2) = 0.70711; 0.1 + 0.8*0.70711
        assert!((l.apply(0.5) - 0.665_685).abs() < 1e-5);
        assert_eq!(l.apply(0.0), 0.1);
    }

    #[test]
    fn curve_is_monotone_and_interpolates() {
        let c = Curve(vec![[0.0, 0.0], [0.25, 0.4], [0.75, 0.6], [1.0, 1.0]]);
        let lut = c.lut(1025);
        assert!(lut.windows(2).all(|w| w[1] >= w[0] - 1e-7));
        assert!((lut[256] - 0.4).abs() < 1e-6);
        assert!((lut[768] - 0.6).abs() < 1e-6);
        let id = Curve::default().lut(5);
        assert_eq!(id, vec![0.0, 0.25, 0.5, 0.75, 1.0]);
    }

    #[test]
    fn hsl_round_trip() {
        let c = [0.8, 0.3, 0.55];
        let o = hsl_to_rgb(rgb_to_hsl(c));
        assert!(c.iter().zip(o).all(|(a, b)| (a - b).abs() < 1e-6));
    }
}
