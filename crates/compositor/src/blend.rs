//! Photoshop blend modes, Blend If and the dissolve hash: the scalar CPU
//! reference. Formulas and their sources are in COMPOSITOR.md §2–3.

use serde::{Deserialize, Serialize};

/// The 27 Photoshop layer blend modes. Groups add Pass Through via
/// [`crate::GroupMode`], not here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BlendMode {
    /// `B = s`.
    #[default]
    Normal,
    /// Normal with alpha replaced by a hashed 0/1 threshold.
    Dissolve,
    /// `min(b, s)`.
    Darken,
    /// `b·s`.
    Multiply,
    /// `1 − min(1, (1 − b)/s)`.
    ColorBurn,
    /// `max(0, b + s − 1)`.
    LinearBurn,
    /// Whole colour with the lower channel sum.
    DarkerColor,
    /// `max(b, s)`.
    Lighten,
    /// `b + s − b·s`.
    Screen,
    /// `min(1, b/(1 − s))`.
    ColorDodge,
    /// `min(1, b + s)` (Add).
    LinearDodge,
    /// Whole colour with the higher channel sum.
    LighterColor,
    /// Hard Light with the layers swapped.
    Overlay,
    /// Photoshop's soft light (not the W3C variant).
    SoftLight,
    /// Multiply or Screen by `2s`.
    HardLight,
    /// Colour Burn or Colour Dodge by `2s`.
    VividLight,
    /// `b + 2s − 1`, clamped.
    LinearLight,
    /// Darken or Lighten by `2s`.
    PinLight,
    /// `1` if `b + s ≥ 1` else `0`.
    HardMix,
    /// `|b − s|`.
    Difference,
    /// `b + s − 2bs`.
    Exclusion,
    /// `max(0, b − s)`.
    Subtract,
    /// `min(1, b/s)`.
    Divide,
    /// Hue of the source, saturation and luminosity of the backdrop.
    Hue,
    /// Saturation of the source.
    Saturation,
    /// Hue and saturation of the source.
    Color,
    /// Luminosity of the source.
    Luminosity,
}

impl BlendMode {
    /// Every mode, in Photoshop's menu order.
    pub const ALL: [BlendMode; 27] = [
        BlendMode::Normal,
        BlendMode::Dissolve,
        BlendMode::Darken,
        BlendMode::Multiply,
        BlendMode::ColorBurn,
        BlendMode::LinearBurn,
        BlendMode::DarkerColor,
        BlendMode::Lighten,
        BlendMode::Screen,
        BlendMode::ColorDodge,
        BlendMode::LinearDodge,
        BlendMode::LighterColor,
        BlendMode::Overlay,
        BlendMode::SoftLight,
        BlendMode::HardLight,
        BlendMode::VividLight,
        BlendMode::LinearLight,
        BlendMode::PinLight,
        BlendMode::HardMix,
        BlendMode::Difference,
        BlendMode::Exclusion,
        BlendMode::Subtract,
        BlendMode::Divide,
        BlendMode::Hue,
        BlendMode::Saturation,
        BlendMode::Color,
        BlendMode::Luminosity,
    ];

    /// Stable index (menu order); also the GPU mode code.
    pub fn index(self) -> u32 {
        Self::ALL.iter().position(|m| *m == self).unwrap_or(0) as u32
    }

    /// True if `B` acts on each channel independently.
    pub fn is_separable(self) -> bool {
        !matches!(
            self,
            BlendMode::DarkerColor
                | BlendMode::LighterColor
                | BlendMode::Hue
                | BlendMode::Saturation
                | BlendMode::Color
                | BlendMode::Luminosity
        )
    }

    /// The PSD `blendModeKey` four-character code.
    pub fn psd_key(self) -> [u8; 4] {
        *match self {
            BlendMode::Normal => b"norm",
            BlendMode::Dissolve => b"diss",
            BlendMode::Darken => b"dark",
            BlendMode::Multiply => b"mul ",
            BlendMode::ColorBurn => b"idiv",
            BlendMode::LinearBurn => b"lbrn",
            BlendMode::DarkerColor => b"dkCl",
            BlendMode::Lighten => b"lite",
            BlendMode::Screen => b"scrn",
            BlendMode::ColorDodge => b"div ",
            BlendMode::LinearDodge => b"lddg",
            BlendMode::LighterColor => b"lgCl",
            BlendMode::Overlay => b"over",
            BlendMode::SoftLight => b"sLit",
            BlendMode::HardLight => b"hLit",
            BlendMode::VividLight => b"vLit",
            BlendMode::LinearLight => b"lLit",
            BlendMode::PinLight => b"pLit",
            BlendMode::HardMix => b"hMix",
            BlendMode::Difference => b"diff",
            BlendMode::Exclusion => b"smud",
            BlendMode::Subtract => b"fsub",
            BlendMode::Divide => b"fdiv",
            BlendMode::Hue => b"hue ",
            BlendMode::Saturation => b"sat ",
            BlendMode::Color => b"colr",
            BlendMode::Luminosity => b"lum ",
        }
    }

    /// Inverse of [`psd_key`](Self::psd_key).
    pub fn from_psd_key(key: &[u8; 4]) -> Option<Self> {
        Self::ALL.into_iter().find(|m| &m.psd_key() == key)
    }
}

#[inline(always)]
fn color_burn(b: f32, s: f32) -> f32 {
    if b >= 1.0 {
        1.0
    } else if s <= 0.0 {
        0.0
    } else {
        1.0 - ((1.0 - b) / s).min(1.0)
    }
}

#[inline(always)]
fn color_dodge(b: f32, s: f32) -> f32 {
    if b <= 0.0 {
        0.0
    } else if s >= 1.0 {
        1.0
    } else {
        (b / (1.0 - s)).min(1.0)
    }
}

#[inline(always)]
fn hard_light(b: f32, s: f32) -> f32 {
    if s <= 0.5 {
        b * 2.0 * s
    } else {
        let t = 2.0 * s - 1.0;
        b + t - b * t
    }
}

/// The separable blend function `B(b, s)` for one channel. Non-separable
/// modes return `s` here; use [`blend_pixel`].
#[inline(always)]
pub fn blend_channel(mode: BlendMode, b: f32, s: f32) -> f32 {
    match mode {
        BlendMode::Normal | BlendMode::Dissolve => s,
        BlendMode::Darken => b.min(s),
        BlendMode::Multiply => b * s,
        BlendMode::ColorBurn => color_burn(b, s),
        BlendMode::LinearBurn => (b + s - 1.0).max(0.0),
        BlendMode::Lighten => b.max(s),
        BlendMode::Screen => b + s - b * s,
        BlendMode::ColorDodge => color_dodge(b, s),
        BlendMode::LinearDodge => (b + s).min(1.0),
        BlendMode::Overlay => hard_light(s, b),
        BlendMode::SoftLight => {
            if s <= 0.5 {
                2.0 * b * s + b * b * (1.0 - 2.0 * s)
            } else {
                2.0 * b * (1.0 - s) + b.max(0.0).sqrt() * (2.0 * s - 1.0)
            }
        }
        BlendMode::HardLight => hard_light(b, s),
        BlendMode::VividLight => {
            if s <= 0.5 {
                color_burn(b, 2.0 * s)
            } else {
                color_dodge(b, 2.0 * s - 1.0)
            }
        }
        BlendMode::LinearLight => (b + 2.0 * s - 1.0).clamp(0.0, 1.0),
        BlendMode::PinLight => {
            if s <= 0.5 {
                b.min(2.0 * s)
            } else {
                b.max(2.0 * s - 1.0)
            }
        }
        BlendMode::HardMix => {
            if b + s >= 1.0 {
                1.0
            } else {
                0.0
            }
        }
        BlendMode::Difference => (b - s).abs(),
        BlendMode::Exclusion => b + s - 2.0 * b * s,
        BlendMode::Subtract => (b - s).max(0.0),
        BlendMode::Divide => {
            if s <= 0.0 {
                if b <= 0.0 { 0.0 } else { 1.0 }
            } else {
                (b / s).min(1.0)
            }
        }
        _ => s,
    }
}

/// Rec.601-style luminosity used by the non-separable modes and Blend If
/// "Gray" (PDF 1.7 §11.3.5.3).
#[inline(always)]
pub fn lum(c: [f32; 3]) -> f32 {
    0.3 * c[0] + 0.59 * c[1] + 0.11 * c[2]
}

#[inline(always)]
fn clip_color(c: [f32; 3]) -> [f32; 3] {
    let l = lum(c);
    let n = c[0].min(c[1]).min(c[2]);
    let x = c[0].max(c[1]).max(c[2]);
    let mut o = c;
    if n < 0.0 {
        let d = l - n;
        for v in &mut o {
            *v = if d > 0.0 { l + (*v - l) * l / d } else { l };
        }
    }
    if x > 1.0 {
        let d = x - l;
        for v in &mut o {
            *v = if d > 0.0 {
                l + (*v - l) * (1.0 - l) / d
            } else {
                l
            };
        }
    }
    o
}

#[inline(always)]
fn set_lum(c: [f32; 3], l: f32) -> [f32; 3] {
    let d = l - lum(c);
    clip_color([c[0] + d, c[1] + d, c[2] + d])
}

#[inline(always)]
fn sat(c: [f32; 3]) -> f32 {
    c[0].max(c[1]).max(c[2]) - c[0].min(c[1]).min(c[2])
}

/// PDF `SetSat`: max → s, min → 0, mid scaled. Written in the closed form
/// `(c − min)·s/(max − min)`, which equals the PDF definition for every
/// channel (ties included) and is what the WGSL port uses.
#[inline(always)]
fn set_sat(c: [f32; 3], s: f32) -> [f32; 3] {
    let mx = c[0].max(c[1]).max(c[2]);
    let mn = c[0].min(c[1]).min(c[2]);
    let range = mx - mn;
    if range > 0.0 {
        c.map(|v| (v - mn) * s / range)
    } else {
        [0.0; 3]
    }
}

/// The full blend `B(Cb, Cs)` for any mode (colours straight, not
/// premultiplied).
#[inline(always)]
pub fn blend_pixel(mode: BlendMode, b: [f32; 3], s: [f32; 3]) -> [f32; 3] {
    match mode {
        BlendMode::DarkerColor => {
            if s[0] + s[1] + s[2] < b[0] + b[1] + b[2] {
                s
            } else {
                b
            }
        }
        BlendMode::LighterColor => {
            if s[0] + s[1] + s[2] > b[0] + b[1] + b[2] {
                s
            } else {
                b
            }
        }
        BlendMode::Hue => set_lum(set_sat(s, sat(b)), lum(b)),
        BlendMode::Saturation => set_lum(set_sat(b, sat(s)), lum(b)),
        BlendMode::Color => set_lum(s, lum(b)),
        BlendMode::Luminosity => set_lum(b, lum(s)),
        m => [
            blend_channel(m, b[0], s[0]),
            blend_channel(m, b[1], s[1]),
            blend_channel(m, b[2], s[2]),
        ],
    }
}

/// One Blend If channel's pair of split sliders, normalized to `[0, 1]`:
/// `[black_lo, black_hi, white_lo, white_hi]`.
pub type SliderRange = [f32; 4];

/// The full-range (no-op) slider.
pub const FULL_RANGE: SliderRange = [0.0, 0.0, 1.0, 1.0];

/// Blend If ranges for one channel ("This Layer" and "Underlying Layer").
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BlendIfChannel {
    /// Sliders applied to the layer's own value.
    pub this_layer: SliderRange,
    /// Sliders applied to the composite below.
    pub underlying: SliderRange,
}

impl Default for BlendIfChannel {
    fn default() -> Self {
        Self {
            this_layer: FULL_RANGE,
            underlying: FULL_RANGE,
        }
    }
}

/// Blend If for Gray and the three colour channels.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct BlendIf {
    /// Gray (luminosity, [`lum`]).
    pub gray: BlendIfChannel,
    /// Red, green, blue.
    pub rgb: [BlendIfChannel; 3],
}

/// Weight of one slider pair at value `v` (piecewise linear, COMPOSITOR.md §3).
/// A black slider at 0 or a white slider at 1 never excludes anything, so
/// values that round (or, in float documents, legitimately lie) outside
/// `[0, 1]` are kept.
#[inline(always)]
pub fn slider_weight(v: f32, r: SliderRange) -> f32 {
    let lo = if r[1] <= 0.0 || v >= r[1] {
        1.0
    } else if v < r[0] {
        0.0
    } else {
        (v - r[0]) / (r[1] - r[0])
    };
    let hi = if r[2] >= 1.0 || v <= r[2] {
        1.0
    } else if v > r[3] {
        0.0
    } else {
        (r[3] - v) / (r[3] - r[2])
    };
    lo.min(hi)
}

impl BlendIf {
    /// True if every slider is at full range.
    pub fn is_identity(&self) -> bool {
        *self == BlendIf::default()
    }

    /// Product of the eight slider weights for layer colour `s` over
    /// backdrop colour `b` (both straight).
    #[inline]
    pub fn weight(&self, s: [f32; 3], b: [f32; 3]) -> f32 {
        let mut w = slider_weight(lum(s), self.gray.this_layer)
            * slider_weight(lum(b), self.gray.underlying);
        for c in 0..3 {
            w *= slider_weight(s[c], self.rgb[c].this_layer)
                * slider_weight(b[c], self.rgb[c].underlying);
        }
        w
    }

    /// Packs the ranges for the GPU: `[gray this, gray under, r this, …]`.
    pub fn packed(&self) -> [[f32; 4]; 8] {
        [
            self.gray.this_layer,
            self.gray.underlying,
            self.rgb[0].this_layer,
            self.rgb[0].underlying,
            self.rgb[1].this_layer,
            self.rgb[1].underlying,
            self.rgb[2].this_layer,
            self.rgb[2].underlying,
        ]
    }
}

/// Deterministic per-pixel hash in `[0, 1)` for Dissolve; identical in WGSL.
#[inline(always)]
pub fn dissolve_threshold(x: u32, y: u32, seed: u32) -> f32 {
    let mut h =
        x.wrapping_mul(0x8da6_b343) ^ y.wrapping_mul(0xd816_3841) ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 16;
    h = h.wrapping_mul(0x7feb_352d);
    h ^= h >> 15;
    h = h.wrapping_mul(0x846c_a68b);
    h ^= h >> 16;
    (h >> 8) as f32 * (1.0 / 16_777_216.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn psd_keys_round_trip() {
        for m in BlendMode::ALL {
            assert_eq!(BlendMode::from_psd_key(&m.psd_key()), Some(m));
            assert_eq!(BlendMode::ALL[m.index() as usize], m);
        }
    }

    #[test]
    fn slider_weights() {
        assert_eq!(slider_weight(0.3, FULL_RANGE), 1.0);
        let r = [0.2, 0.4, 0.6, 0.8];
        assert_eq!(slider_weight(0.1, r), 0.0);
        assert!((slider_weight(0.3, r) - 0.5).abs() < 1e-6);
        assert_eq!(slider_weight(0.5, r), 1.0);
        assert!((slider_weight(0.75, r) - 0.25).abs() < 1e-6);
        assert_eq!(slider_weight(0.9, r), 0.0);
        // Unsplit sliders are hard steps, inclusive at both ends.
        assert_eq!(slider_weight(0.2, [0.2, 0.2, 0.6, 0.6]), 1.0);
        assert_eq!(slider_weight(0.6, [0.2, 0.2, 0.6, 0.6]), 1.0);
        assert_eq!(slider_weight(0.61, [0.2, 0.2, 0.6, 0.6]), 0.0);
        // Endpoints at 0 / 1 are open-ended.
        assert_eq!(slider_weight(1.000_000_1, FULL_RANGE), 1.0);
        assert_eq!(slider_weight(-1e-7, FULL_RANGE), 1.0);
        assert_eq!(slider_weight(3.0, [0.0, 0.0, 1.0, 1.0]), 1.0);
    }

    #[test]
    fn set_sat_matches_pdf() {
        // PDF: mid = (mid-min)*s/(max-min), max = s, min = 0.
        let o = set_sat([0.8, 0.2, 0.5], 0.3);
        assert!((o[0] - 0.3).abs() < 1e-6 && o[1] == 0.0 && (o[2] - 0.15).abs() < 1e-6);
    }
}
