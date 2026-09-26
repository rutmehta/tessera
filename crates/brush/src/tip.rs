//! Brush tips (computed round and sampled), dual brush and texture.

use std::sync::Arc;

use engine_api::{EngineError, EngineResult};

use crate::planner::Dab;

/// A grayscale sampled tip or pattern; `1.0` = full paint.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct SampledTip {
    /// Name (ABR sample id or preset name).
    pub name: String,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Row-major coverage `0..=1`.
    pub data: Vec<f32>,
}

impl SampledTip {
    /// Validates dimensions against `data`.
    pub fn new(
        name: impl Into<String>,
        width: u32,
        height: u32,
        data: Vec<f32>,
    ) -> EngineResult<Self> {
        if width == 0 || height == 0 || data.len() != width as usize * height as usize {
            return Err(EngineError::invalid("tip", "size does not match data"));
        }
        if data.iter().any(|v| !v.is_finite()) {
            return Err(EngineError::invalid("tip", "non-finite sample"));
        }
        Ok(Self {
            name: name.into(),
            width,
            height,
            data,
        })
    }

    /// Texel `(x, y)`; zero outside.
    #[inline]
    pub fn texel(&self, x: i64, y: i64) -> f32 {
        if x < 0 || y < 0 || x >= i64::from(self.width) || y >= i64::from(self.height) {
            0.0
        } else {
            self.data[y as usize * self.width as usize + x as usize]
        }
    }

    /// Bilinear sample in texel coordinates (centres at `i + 0.5`), zero
    /// outside.
    #[inline]
    pub fn bilinear(&self, x: f32, y: f32) -> f32 {
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let a = self.texel(x0, y0) + (self.texel(x0 + 1, y0) - self.texel(x0, y0)) * ax;
        let b = self.texel(x0, y0 + 1) + (self.texel(x0 + 1, y0 + 1) - self.texel(x0, y0 + 1)) * ax;
        a + (b - a) * ay
    }

    /// Bilinear sample with wrap-around (patterns).
    pub fn wrapped(&self, x: f32, y: f32) -> f32 {
        let (w, h) = (i64::from(self.width), i64::from(self.height));
        let (fx, fy) = (x - 0.5, y - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let t = |x: i64, y: i64| self.texel(x.rem_euclid(w), y.rem_euclid(h));
        let a = t(x0, y0) + (t(x0 + 1, y0) - t(x0, y0)) * ax;
        let b = t(x0, y0 + 1) + (t(x0 + 1, y0 + 1) - t(x0, y0 + 1)) * ax;
        a + (b - a) * ay
    }
}

/// Shape of a tip.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub enum TipShape {
    /// Computed round tip; `hardness` 0 = soft, 1 = hard (1 px anti-aliasing).
    Round {
        /// Hardness `0..=1`.
        hardness: f32,
    },
    /// Sampled tip; its larger side maps to the brush diameter.
    Sampled(Arc<SampledTip>),
}

/// A tip with its static pose.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Tip {
    /// Shape.
    pub shape: TipShape,
    /// Angle in radians (added to the dynamic angle).
    pub angle: f32,
    /// Roundness `0.01..=1` (minor/major axis ratio).
    pub roundness: f32,
}

impl Default for Tip {
    fn default() -> Self {
        Self::round(1.0)
    }
}

/// Coverage of a computed round tip at distance `d` from its centre.
///
/// Fully opaque inside `hardness · radius`, smoothstep falloff to zero at
/// `radius + 0.5`; the falloff is never narrower than one pixel (the hard
/// tip's anti-aliasing band).
#[inline]
pub fn round_coverage(d: f32, radius: f32, hardness: f32) -> f32 {
    let h = hardness.clamp(0.0, 1.0);
    let w = (1.0 - h) * radius + 1.0;
    let t = ((radius + 0.5 - d) / w).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

impl Tip {
    /// A round tip.
    pub fn round(hardness: f32) -> Self {
        Self {
            shape: TipShape::Round { hardness },
            angle: 0.0,
            roundness: 1.0,
        }
    }

    /// A sampled tip.
    pub fn sampled(tip: SampledTip) -> Self {
        Self {
            shape: TipShape::Sampled(Arc::new(tip)),
            angle: 0.0,
            roundness: 1.0,
        }
    }

    /// Half the side of an axis-aligned square that contains every non-zero
    /// coverage of a dab of diameter `size`, in any pose.
    pub fn half_extent(&self, size: f32) -> f32 {
        match &self.shape {
            TipShape::Round { .. } => size * 0.5 + 1.0,
            TipShape::Sampled(t) => {
                // Half diagonal plus the bilinear support of one texel.
                let (w, h) = (t.width as f32, t.height as f32);
                let texel = size / w.max(h);
                0.5 * size * w.hypot(h) / w.max(h) + texel + 1.0
            }
        }
    }

    /// `(coverage, normalized radial distance)` of a dab at canvas offset
    /// `(dx, dy)` from its centre.
    #[inline]
    pub fn coverage(&self, dab: &Dab, dx: f32, dy: f32) -> (f32, f32) {
        self.coverage_pose(&dab.pose(), dx, dy)
    }

    /// Like [`Tip::coverage`] for an explicit pose.
    #[inline]
    pub fn coverage_pose(&self, p: &Pose, dx: f32, dy: f32) -> (f32, f32) {
        let (s, c) = p.angle.sin_cos();
        let mut u = dx * c + dy * s;
        let mut v = -dx * s + dy * c;
        if p.flip_x {
            u = -u;
        }
        if p.flip_y {
            v = -v;
        }
        v /= p.roundness.clamp(0.01, 1.0);
        let radius = (p.size * 0.5).max(1e-3);
        let d = u.hypot(v);
        let cov = match &self.shape {
            TipShape::Round { hardness } => round_coverage(d, radius, *hardness),
            TipShape::Sampled(t) => {
                let scale = p.size.max(1e-3) / t.width.max(t.height) as f32;
                t.bilinear(
                    u / scale + t.width as f32 * 0.5,
                    v / scale + t.height as f32 * 0.5,
                )
            }
        };
        (cov, d / radius)
    }
}

/// Geometric pose of one stamp.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// Diameter, pixels.
    pub size: f32,
    /// Angle, radians.
    pub angle: f32,
    /// Roundness.
    pub roundness: f32,
    /// Horizontal flip.
    pub flip_x: bool,
    /// Vertical flip.
    pub flip_y: bool,
}

/// Dual brush: coverage is multiplied by a second tip stamped `count` times
/// per primary dab, scattered around it.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct DualBrush {
    /// Secondary tip.
    pub tip: Tip,
    /// Secondary diameter, pixels.
    pub size: f32,
    /// Scatter radius as a multiple of the primary diameter.
    pub scatter: f32,
    /// Secondary stamps per primary dab (≥ 1).
    pub count: u32,
}

/// How a texture modulates coverage.
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextureMode {
    /// `c · (1 − depth · (1 − t))`.
    #[default]
    Multiply,
    /// `max(0, c − depth · (1 − t))`.
    Subtract,
    /// Height: the coverage must exceed the texture relief.
    Height,
}

/// Canvas-anchored pattern applied to every dab ("texture each tip").
#[derive(serde::Serialize, serde::Deserialize, Debug, Clone, PartialEq)]
pub struct Texture {
    /// Tiled pattern.
    pub pattern: Arc<SampledTip>,
    /// Pattern scale (texels per canvas pixel⁻¹; 1 = native).
    pub scale: f32,
    /// Depth `0..=1`.
    pub depth: f32,
    /// Invert the pattern.
    pub invert: bool,
    /// Modulation.
    pub mode: TextureMode,
}

impl Texture {
    /// Modulates coverage `c` at canvas point `(x, y)`.
    #[inline]
    pub fn apply(&self, c: f32, x: f32, y: f32) -> f32 {
        let s = self.scale.max(1e-3);
        let mut t = self.pattern.wrapped(x / s, y / s).clamp(0.0, 1.0);
        if self.invert {
            t = 1.0 - t;
        }
        let relief = self.depth.clamp(0.0, 1.0) * (1.0 - t);
        match self.mode {
            TextureMode::Multiply => c * (1.0 - relief),
            TextureMode::Subtract => (c - relief).max(0.0),
            TextureMode::Height => {
                if relief >= 1.0 {
                    0.0
                } else {
                    ((c - relief) / (1.0 - relief)).clamp(0.0, 1.0)
                }
            }
        }
    }
}

/// Wet-edges profile: the interior drops to half strength and the rim keeps
/// full strength (`rn` = normalized radial distance).
#[inline]
pub fn wet_edges(c: f32, rn: f32) -> f32 {
    let t = ((rn - 0.5) / 0.5).clamp(0.0, 1.0);
    c * (0.5 + 0.5 * t * t * (3.0 - 2.0 * t))
}
