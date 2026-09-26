//! Adjustment layers: per-pixel functions of the composite below
//! (spec 02 §7). Formulas are in COMPOSITOR.md §4.
//!
//! These are display-referred operators on the document's own encoding.
//! pipeline-cpu's operators are scene-referred linear Rec.2020 and pull in
//! the raw decoder, so they are not reused here (see COMPOSITOR.md §9).

use serde::{Deserialize, Serialize};

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
    Direct(&'a Adjustment),
}

const LUT_N: usize = 4096;

#[inline(always)]
fn lut_eval(lut: &[f32], v: f32) -> f32 {
    let x = v.clamp(0.0, 1.0) * (LUT_N - 1) as f32;
    let i = (x as usize).min(LUT_N - 2);
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
            other => Compiled::Direct(other),
        }
    }
}

impl Compiled<'_> {
    /// The adjusted colour (straight RGB in, straight RGB out).
    #[inline]
    pub(crate) fn apply(&self, c: [f32; 3]) -> [f32; 3] {
        match self {
            Compiled::Luts(ch, master) => {
                let mut o = [0.0; 3];
                for i in 0..3 {
                    o[i] = lut_eval(master, lut_eval(&ch[i], c[i]));
                }
                o
            }
            Compiled::Direct(a) => apply_direct(a, c),
        }
    }
}

fn apply_direct(a: &Adjustment, c: [f32; 3]) -> [f32; 3] {
    match *a {
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
        Adjustment::Levels { .. } | Adjustment::Curves { .. } => c,
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
