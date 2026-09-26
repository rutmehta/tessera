//! Display-referred Shadows/Highlights local operator (spec 02 §7).
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};

/// Local tonal controls. Fractions use [0,1], signed controls [-1,1],
/// radii are level-zero pixels. Defaults deliberately form an identity.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ShadowsHighlights {
    /// Shadow lift strength, [0,1].
    pub shadows_amount: f32,
    /// Width of the shadow luminance range, [0,1].
    pub shadows_tone: f32,
    /// Shadow bilateral support radius in level-zero pixels, [0,250].
    pub shadows_radius: f32,
    /// Highlight compression strength, [0,1].
    pub highlights_amount: f32,
    /// Width of the highlight luminance range, [0,1].
    pub highlights_tone: f32,
    /// Highlight bilateral support radius in level-zero pixels, [0,250].
    pub highlights_radius: f32,
    /// Chroma correction about luminance, [-1,1].
    pub color: f32,
    /// Midtone contrast strength, [-1,1].
    pub midtone: f32,
    /// Normalized black endpoint (not a histogram percentile), [0,1].
    pub black_clip: f32,
    /// Normalized white endpoint reduction, [0,1]; clip sum must be below 1.
    pub white_clip: f32,
}
impl Default for ShadowsHighlights {
    fn default() -> Self {
        Self {
            shadows_amount: 0.0,
            shadows_tone: 0.5,
            shadows_radius: 30.0,
            highlights_amount: 0.0,
            highlights_tone: 0.5,
            highlights_radius: 30.0,
            color: 0.0,
            midtone: 0.0,
            black_clip: 0.0,
            white_clip: 0.0,
        }
    }
}
impl ShadowsHighlights {
    /// Reject invalid public/serialized controls rather than silently changing them.
    pub fn validate(&self) -> EngineResult<()> {
        let unit = [
            self.shadows_amount,
            self.shadows_tone,
            self.highlights_amount,
            self.highlights_tone,
            self.black_clip,
            self.white_clip,
        ];
        if unit
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            || [self.color, self.midtone]
                .iter()
                .any(|v| !v.is_finite() || !(-1.0..=1.0).contains(v))
            || [self.shadows_radius, self.highlights_radius]
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=250.0).contains(v))
            || self.black_clip + self.white_clip >= 1.0
        {
            return Err(EngineError::invalid(
                "shadows_highlights",
                "invalid controls: fractions [0,1], color/midtone [-1,1], radius [0,250], clip sum <1",
            ));
        }
        Ok(())
    }

    /// Whether any enabled tonal range requires neighbouring backdrop samples.
    pub fn needs_neighbourhood(&self) -> bool {
        (self.shadows_amount != 0.0 && self.shadows_radius > 0.0)
            || (self.highlights_amount != 0.0 && self.highlights_radius > 0.0)
    }

    /// Support in pixels at the requested pyramid level (not Gaussian sigma).
    pub fn halo(&self, level: u8) -> usize {
        let radius = (if self.shadows_amount != 0.0 {
            self.shadows_radius
        } else {
            0.0
        })
        .max(if self.highlights_amount != 0.0 {
            self.highlights_radius
        } else {
            0.0
        });
        (radius / 2.0f32.powi(i32::from(level))).ceil() as usize
    }

    /// Process a rectangular interior of a *padded*, straight interleaved RGBA
    /// backdrop. Output is row-major interior only. Every interior pixel must
    /// have `halo(level)` real backdrop pixels on each side; only the caller
    /// knows where document edges are and may replicate those edges. Tile-edge
    /// clamping is intentionally NOT performed here. Alpha is preserved exactly.
    ///
    /// A direct bilateral luminance base uses spatial sigma radius/2 and range
    /// sigma 0.15. This reference path is O(interior area * radius squared).
    /// It is a native operator, not a claim of Adobe numerical equivalence.
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
        let h = self.halo(level);
        if width.checked_mul(height) != Some(pixels.len())
            || x0 > x1
            || y0 > y1
            || x1 > width
            || y1 > height
            || x0 < h
            || y0 < h
            || width - x1 < h
            || height - y1 < h
        {
            return Err(EngineError::invalid(
                "shadows_highlights",
                "invalid dimensions or insufficient backdrop halo",
            ));
        }
        if pixels.iter().flatten().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid(
                "shadows_highlights",
                "non-finite backdrop",
            ));
        }
        let scale = 2.0f32.powi(i32::from(level));
        let mut out = Vec::with_capacity((x1 - x0) * (y1 - y0));
        for y in y0..y1 {
            for x in x0..x1 {
                let p = pixels[y * width + x];
                if p[3] <= 0.0 || self.is_identity() {
                    out.push(p);
                    continue;
                }
                let l = luma(p);
                let sb = if self.shadows_amount != 0.0 {
                    bilateral(pixels, width, x, y, self.shadows_radius / scale)
                } else {
                    l
                };
                let hb = if self.highlights_amount != 0.0 {
                    bilateral(pixels, width, x, y, self.highlights_radius / scale)
                } else {
                    l
                };
                let rgb = self.map_rgb([p[0], p[1], p[2]], sb, hb);
                out.push([rgb[0], rgb[1], rgb[2], p[3]]);
            }
        }
        Ok(out)
    }

    /// True when every output-changing control is neutral.
    pub fn is_identity(&self) -> bool {
        self.shadows_amount == 0.0
            && self.highlights_amount == 0.0
            && self.color == 0.0
            && self.midtone == 0.0
            && self.black_clip == 0.0
            && self.white_clip == 0.0
    }

    fn map_rgb(&self, rgb: [f32; 3], sb: f32, hb: f32) -> [f32; 3] {
        let l = luma([rgb[0], rgb[1], rgb[2], 1.0]);
        let shadow = membership(sb, self.shadows_tone);
        let highlight = membership(1.0 - hb, self.highlights_tone);
        let mut mapped = l + 0.5 * self.shadows_amount * shadow * (1.0 - l).max(0.0)
            - 0.5 * self.highlights_amount * highlight * l.max(0.0);
        let t = mapped.clamp(0.0, 1.0);
        mapped += self.midtone * (t - 0.5) * 4.0 * t * (1.0 - t);
        let span = 1.0 - self.black_clip - self.white_clip;
        rgb.map(|v| {
            ((mapped + (v - l) * (1.0 + self.color) - self.black_clip) / span).clamp(0.0, 1.0)
        })
    }
}

fn luma(p: [f32; 4]) -> f32 {
    0.2126 * p[0] + 0.7152 * p[1] + 0.0722 * p[2]
}
fn membership(l: f32, tone: f32) -> f32 {
    if tone <= 0.0 {
        return 0.0;
    }
    let t = (1.0 - l / tone).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}
fn bilateral(pixels: &[[f32; 4]], width: usize, x: usize, y: usize, radius: f32) -> f32 {
    let center = luma(pixels[y * width + x]);
    if radius <= 0.0 {
        return center;
    }
    let support = radius.ceil() as usize;
    let sigma = (radius * 0.5).max(0.5);
    let (mut sum, mut weight) = (0.0f64, 0.0f64);
    for yy in y - support..=y + support {
        for xx in x - support..=x + support {
            let p = pixels[yy * width + xx];
            if p[3] <= 0.0 {
                continue;
            }
            let l = luma(p);
            let dx = xx as f32 - x as f32;
            let dy = yy as f32 - y as f32;
            let w = (-(dx * dx + dy * dy) / (2.0 * sigma * sigma)
                - (l - center).powi(2) / (2.0 * 0.15 * 0.15))
                .exp()
                * p[3];
            sum += f64::from(w) * f64::from(l);
            weight += f64::from(w);
        }
    }
    if weight > 0.0 {
        (sum / weight) as f32
    } else {
        center
    }
}
