//! Native HDR tone mapping. Parameters are frozen and serialize with the layer;
//! these formulas do not claim numerical equivalence to Adobe HDR Toning.
use super::{Curve, lut_eval};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Native tone mapping method.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HdrMethod {
    /// Edge-aware luminance base compression and detail reconstruction.
    #[default]
    LocalAdaptation,
    /// Frozen luminance histogram CDF.
    EqualizeHistogram,
    /// Per-channel exposure followed by reciprocal gamma.
    ExposureGamma,
    /// Luminance-preserving Reinhard compression.
    HighlightCompression,
}

/// HDR controls. The default local operator is neutral (including over-range
/// RGB). Other methods use only their relevant fields, as documented below.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HdrToning {
    /// Tone mapping method.
    pub method: HdrMethod,
    /// Local bilateral support radius in level-zero pixels, [0,250].
    pub radius: f32,
    /// Local base compression strength, [0,1].
    pub strength: f32,
    /// Reciprocal output gamma, [0.1,10].
    pub gamma: f32,
    /// Exposure in stops, [-20,20].
    pub exposure: f32,
    /// Local residual gain minus one, [-1,1].
    pub detail: f32,
    /// Local shadow lift, [-1,1].
    pub shadows: f32,
    /// Local highlight reduction, [-1,1].
    pub highlights: f32,
    /// Local chroma boost weighted by inverse RGB saturation, [-1,1].
    pub vibrance: f32,
    /// Local chroma gain minus one, [-1,1].
    pub saturation: f32,
    /// Local output luminance curve on [0,1], sampled at 4096 points.
    pub curve: Curve,
    /// Frozen normalized luminance CDF, empty for identity. At least two
    /// monotone unit-domain values are required when nonempty.
    pub equalize_map: Vec<f32>,
    /// Positive maximum luminance represented by the last histogram bin.
    pub equalize_max: f32,
}
impl Default for HdrToning {
    fn default() -> Self {
        Self {
            method: HdrMethod::LocalAdaptation,
            radius: 30.0,
            strength: 0.0,
            gamma: 1.0,
            exposure: 0.0,
            detail: 0.0,
            shadows: 0.0,
            highlights: 0.0,
            vibrance: 0.0,
            saturation: 0.0,
            curve: Curve::default(),
            equalize_map: Vec::new(),
            equalize_max: 1.0,
        }
    }
}
impl HdrToning {
    /// Check all serialized controls, including dormant method settings.
    pub fn validate(&self) -> EngineResult<()> {
        let in_range = |v: f32, a: f32, b: f32| v.is_finite() && (a..=b).contains(&v);
        if !in_range(self.radius, 0.0, 250.0)
            || !in_range(self.strength, 0.0, 1.0)
            || !in_range(self.gamma, 0.1, 10.0)
            || !in_range(self.exposure, -20.0, 20.0)
            || [
                self.detail,
                self.shadows,
                self.highlights,
                self.vibrance,
                self.saturation,
            ]
            .iter()
            .any(|&v| !in_range(v, -1.0, 1.0))
            || !self.equalize_max.is_finite()
            || self.equalize_max <= 0.0
            || (!self.equalize_map.is_empty() && self.equalize_map.len() < 2)
            || self.equalize_map.iter().any(|&v| !in_range(v, 0.0, 1.0))
            || self.equalize_map.windows(2).any(|w| w[1] < w[0])
            || self
                .curve
                .0
                .iter()
                .flatten()
                .any(|&v| !in_range(v, 0.0, 1.0))
            || self.curve.0.windows(2).any(|w| w[1][0] <= w[0][0])
        {
            return Err(EngineError::invalid(
                "hdr_toning",
                "invalid HDR controls, curve, or frozen histogram",
            ));
        }
        Ok(())
    }

    /// Resolve a luminance histogram once. Bins uniformly span [0,max_luminance].
    /// Empty or constant populations use a linear map. Counts accumulate as f64
    /// to avoid u64 overflow; analysis never depends on the current render tile.
    pub fn equalize_from_histogram(histogram: &[u64], max_luminance: f32) -> EngineResult<Self> {
        if histogram.len() < 2 || !max_luminance.is_finite() || max_luminance <= 0.0 {
            return Err(EngineError::invalid(
                "hdr_histogram",
                "at least two bins and positive finite maximum required",
            ));
        }
        let total = histogram.iter().map(|&v| v as f64).sum::<f64>();
        let first = histogram.iter().find(|&&v| v > 0).copied().unwrap_or(0) as f64;
        let mut sum = 0.0;
        let equalize_map = histogram
            .iter()
            .enumerate()
            .map(|(i, &v)| {
                sum += v as f64;
                if total <= first {
                    i as f32 / (histogram.len() - 1) as f32
                } else {
                    ((sum - first) / (total - first)).clamp(0.0, 1.0) as f32
                }
            })
            .collect();
        Ok(Self {
            method: HdrMethod::EqualizeHistogram,
            equalize_map,
            equalize_max: max_luminance,
            ..Self::default()
        })
    }

    /// Whether the local base influences output and needs neighbor pixels.
    pub fn needs_neighbourhood(&self) -> bool {
        self.method == HdrMethod::LocalAdaptation
            && self.radius > 0.0
            && (self.strength != 0.0 || self.detail != 0.0)
    }
    /// Level-scaled bilateral support; inactive spatial controls need no halo.
    pub fn halo(&self, level: u8) -> usize {
        if self.needs_neighbourhood() {
            (self.radius / 2.0f32.powi(i32::from(level))).ceil() as usize
        } else {
            0
        }
    }
    /// Whether all controls of the selected method leave RGB unchanged.
    pub fn is_identity(&self) -> bool {
        match self.method {
            HdrMethod::LocalAdaptation => {
                self.strength == 0.0
                    && self.detail == 0.0
                    && self.gamma == 1.0
                    && self.exposure == 0.0
                    && self.shadows == 0.0
                    && self.highlights == 0.0
                    && self.vibrance == 0.0
                    && self.saturation == 0.0
                    && self.curve.is_identity()
            }
            HdrMethod::ExposureGamma => self.exposure == 0.0 && self.gamma == 1.0,
            _ => false,
        }
    }
    /// Apply using an already computed local luminance base. This convenience
    /// method builds the curve LUT; tiled execution builds it just once.
    pub fn map_rgb(&self, rgb: [f32; 3], base: f32) -> [f32; 3] {
        let lut = self.curve_lut();
        self.map_with_lut(rgb, base, &lut)
    }
    pub(crate) fn curve_lut(&self) -> Vec<f32> {
        if self.curve.is_identity() {
            Vec::new()
        } else {
            self.curve.lut(4096)
        }
    }
    pub(crate) fn map_with_lut(&self, rgb: [f32; 3], base: f32, curve: &[f32]) -> [f32; 3] {
        if self.is_identity() {
            return rgb;
        }
        let l = luminance(rgb).max(0.0);
        match self.method {
            HdrMethod::ExposureGamma => rgb.map(|v| {
                power((v * 2.0f32.powf(self.exposure)).max(0.0), self.gamma).clamp(0.0, 1.0)
            }),
            HdrMethod::HighlightCompression => rgb.map(|v| (v / (1.0 + l)).clamp(0.0, 1.0)),
            HdrMethod::EqualizeHistogram => {
                let mapped = lut_eval(&self.equalize_map, (l / self.equalize_max).clamp(0.0, 1.0));
                let scale = if l > 1e-8 { mapped / l } else { 0.0 };
                rgb.map(|v| (v * scale).clamp(0.0, 1.0))
            }
            HdrMethod::LocalAdaptation => {
                let b = base.max(0.0);
                let mut mapped =
                    (b / (1.0 + self.strength * b) + (l - b) * (1.0 + self.detail)).max(0.0);
                mapped = power(mapped * 2.0f32.powf(self.exposure), self.gamma);
                let t = mapped.clamp(0.0, 1.0);
                let sw = smooth((1.0 - 2.0 * t).clamp(0.0, 1.0));
                let hw = smooth((2.0 * t - 1.0).clamp(0.0, 1.0));
                mapped += 0.5 * self.shadows * sw * (1.0 - t) - 0.5 * self.highlights * hw * t;
                mapped = lut_eval(curve, mapped).clamp(0.0, 1.0);
                let maximum = rgb[0].max(rgb[1]).max(rgb[2]);
                let minimum = rgb[0].min(rgb[1]).min(rgb[2]);
                let sat = if maximum > 1e-8 {
                    ((maximum - minimum) / maximum).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let chroma = (1.0 + self.saturation) * (1.0 + self.vibrance * (1.0 - sat));
                let scale = if l > 1e-8 { mapped / l } else { 0.0 };
                rgb.map(|v| (mapped + (v - l) * scale * chroma).clamp(0.0, 1.0))
            }
        }
    }
    /// Process an interior of a real padded, straight RGBA backdrop. Caller
    /// replicates document edges; tile edges must carry actual neighboring data.
    /// Alpha (including hidden RGB for zero alpha) is preserved exactly.
    pub fn apply_padded(
        &self,
        pixels: &[[f32; 4]],
        width: usize,
        height: usize,
        interior: [usize; 4],
        level: u8,
    ) -> EngineResult<Vec<[f32; 4]>> {
        self.validate()?;
        let [x0, y0, x1, y1] = interior;
        let halo = self.halo(level);
        if width.checked_mul(height) != Some(pixels.len())
            || x0 > x1
            || y0 > y1
            || x1 > width
            || y1 > height
            || x0 < halo
            || y0 < halo
            || width - x1 < halo
            || height - y1 < halo
            || pixels.iter().flatten().any(|v| !v.is_finite())
        {
            return Err(EngineError::invalid(
                "hdr_toning",
                "invalid padded backdrop or insufficient halo",
            ));
        }
        let bases = if self.needs_neighbourhood() {
            super::shadows::separable_bases(
                pixels,
                width,
                height,
                self.radius / 2.0f32.powi(i32::from(level)),
            )
        } else {
            Vec::new()
        };
        let curve = self.curve_lut();
        let mut out = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for y in y0..y1 {
            for x in x0..x1 {
                let p = pixels[y * width + x];
                if p[3] <= 0.0 || self.is_identity() {
                    out.push(p);
                    continue;
                }
                let rgb = [p[0], p[1], p[2]];
                let base = if bases.is_empty() {
                    luminance(rgb)
                } else {
                    bases[y * width + x]
                };
                let mapped = self.map_with_lut(rgb, base, &curve);
                out.push([mapped[0], mapped[1], mapped[2], p[3]]);
            }
        }
        Ok(out)
    }
}
fn smooth(t: f32) -> f32 {
    t * t * (3.0 - 2.0 * t)
}
fn power(v: f32, gamma: f32) -> f32 {
    if gamma == 1.0 { v } else { v.powf(1.0 / gamma) }
}
pub(crate) fn luminance(rgb: [f32; 3]) -> f32 {
    0.2126 * rgb[0] + 0.7152 * rgb[1] + 0.0722 * rgb[2]
}
