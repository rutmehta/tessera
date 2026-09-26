//! CPU layer effects. All planes are full-canvas, straight RGBA, and must
//! be composited in returned order within their `outside` group. Geometry
//! comes from source alpha, never source RGB or fill opacity.
//!
//! This is a deterministic approximation, not Adobe's proprietary renderer:
//! morphology uses a square footprint, blur is a truncated Gaussian, and
//! bevel uses a blurred alpha height field. Contour and jitter are preserved
//! metadata only. Effect geometry scales; Fill coordinates remain in canvas
//! pixels, matching `Fill::sample`.

use crate::{
    blend::BlendMode,
    document::Fill,
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::{EngineError, EngineResult};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

const MAX_RADIUS: f32 = 256.0;
const MAX_DISTANCE: f32 = 16384.0;
const MAX_PIXELS: usize = 16_777_216;

/// Document-wide light direction, in degrees. Angle 90 lights from above.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GlobalLight {
    /// Finite azimuth in degrees: 0 lights from the right, 90 from above.
    pub angle: f32,
    /// Elevation above the canvas in degrees, in `0..=90`; used by bevels.
    pub elevation: f32,
}
impl Default for GlobalLight {
    fn default() -> Self {
        Self {
            angle: 120.0,
            elevation: 30.0,
        }
    }
}
impl GlobalLight {
    /// Check that the azimuth is finite and elevation lies in `0..=90` degrees.
    ///
    /// # Errors
    /// Returns an invalid-input error for non-finite or out-of-range values.
    pub fn validate(self) -> EngineResult<()> {
        finite(self.angle, "light angle")?;
        range(self.elevation, 0.0, 90.0, "light elevation")
    }
}

/// Effects may repeat. Equal-kind effects retain their vector order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LayerStyles {
    /// Effects in insertion order, with at most 64 entries when validated.
    /// Rendering uses effect-kind stacking ranks, preserving ties in this order.
    pub effects: Vec<StyleEffect>,
    /// Geometry multiplier in `0..=100`, not a percentage; `1` is actual size.
    /// Scales pixel sizes, spread, soften, and distances, but not fill coordinates.
    pub scale: f32,
}
impl Default for LayerStyles {
    fn default() -> Self {
        Self {
            effects: Vec::new(),
            scale: 1.0,
        }
    }
}

/// Stored contour/jitter controls, intentionally not evaluated by this CPU
/// reference. Points are normalized input/output pairs.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct EffectShape {
    /// Up to 4096 normalized `[input, output]` pairs, each component in `0..=1`.
    /// Retained for round trips; does not alter this renderer's output.
    pub contour: Vec<[f32; 2]>,
    /// Jitter amount in `0..=1`, retained but not evaluated by this renderer.
    pub jitter: f32,
}

/// A layer effect and its settings; repeated variants are supported.
/// Named overlay variants select stacking ranks, not restrictions on fill type.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "settings", rename_all = "snake_case")]
pub enum StyleEffect {
    /// Offset, expanded, blurred source alpha composited behind the layer.
    DropShadow(Shadow),
    /// Complement of offset, choked, blurred alpha clipped to the layer.
    InnerShadow(Shadow),
    /// Expanded, blurred alpha glow composited behind the layer.
    OuterGlow(Glow),
    /// Edge- or center-originating glow clipped to source alpha.
    InnerGlow(Glow),
    /// Highlight and shadow bands lit from a blurred alpha height field.
    Bevel(Bevel),
    /// Interior shading from the difference of opposite alpha offsets.
    Satin(Satin),
    /// Fill overlay at the color-overlay stacking rank.
    ColorOverlay(Overlay),
    /// Fill overlay at the gradient-overlay stacking rank, below color overlays.
    GradientOverlay(Overlay),
    /// Fill overlay at the pattern-overlay stacking rank, below gradient overlays.
    PatternOverlay(Overlay),
    /// Generic Fill overlay; the named variants supply PSD stacking ranks.
    Overlay(Overlay),
    /// Filled morphological band along the source alpha boundary.
    Stroke(Stroke),
}

/// Shared settings for drop shadows and inner shadows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Shadow {
    /// Whether to render this effect; disabled settings are still validated.
    pub enabled: bool,
    /// Straight RGBA with finite RGB and alpha in `0..=1`.
    pub color: [f32; 4],
    /// Blend mode used to composite the shadow plane.
    pub mode: BlendMode,
    /// Compositing opacity in `0..=1`, applied separately from color alpha.
    pub opacity: f32,
    /// Local light azimuth in degrees; 0 casts left, 90 casts downward.
    pub angle: f32,
    /// Use the document light angle instead of the local angle.
    pub use_global_light: bool,
    /// Nonnegative shadow offset in pixels before layer-style scaling.
    pub distance: f32,
    /// Nonnegative Gaussian support radius in pixels before style scaling.
    pub size: f32,
    /// Nonnegative solid expansion/choke in pixels before scaling and blur.
    /// This is not a percentage.
    pub spread: f32,
    /// Preserved contour and jitter metadata; not evaluated by the renderer.
    pub shape: EffectShape,
}
impl Default for Shadow {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [0.0, 0.0, 0.0, 1.0],
            mode: BlendMode::Multiply,
            opacity: 0.75,
            angle: 120.0,
            use_global_light: true,
            distance: 5.0,
            size: 5.0,
            spread: 0.0,
            shape: EffectShape::default(),
        }
    }
}

/// Shared settings for outer and inner alpha-derived glows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Glow {
    /// Whether to render this effect; disabled settings are still validated.
    pub enabled: bool,
    /// Straight RGBA with finite RGB and alpha in `0..=1`.
    pub color: [f32; 4],
    /// Blend mode used to composite the glow plane.
    pub mode: BlendMode,
    /// Compositing opacity in `0..=1`, separate from color alpha.
    pub opacity: f32,
    /// Nonnegative Gaussian support radius in pixels before style scaling.
    pub size: f32,
    /// Nonnegative expansion/choke in pixels before style scaling and blur.
    pub spread: f32,
    /// Make an inner glow originate at the center rather than the edge.
    /// Ignored for outer glows.
    pub center: bool,
    /// Preserved contour and jitter metadata; not evaluated by the renderer.
    pub shape: EffectShape,
}
impl Default for Glow {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [1.0; 4],
            mode: BlendMode::Screen,
            opacity: 0.75,
            size: 5.0,
            spread: 0.0,
            center: false,
            shape: EffectShape::default(),
        }
    }
}

/// Placement and relief direction of bevel bands around the alpha boundary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BevelKind {
    /// Shade only the band inside the source alpha boundary.
    #[default]
    Inner,
    /// Shade only the band outside the source alpha boundary.
    Outer,
    /// Shade both inner and outer bands with the same relief direction.
    Emboss,
    /// Shade both bands, reversing the relief direction on the inner band.
    Pillow,
}
/// Alpha-height-field bevel with independently composited highlights and shadows.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Bevel {
    /// Whether to render this effect; disabled settings are still validated.
    pub enabled: bool,
    /// Bands to shade and their relative relief directions.
    pub kind: BevelKind,
    /// Nonnegative band width and height-field blur radius in unscaled pixels.
    pub size: f32,
    /// Additional nonnegative height-field blur radius in unscaled pixels.
    pub soften: f32,
    /// Relief-strength multiplier in `0..=100`, not a percentage.
    pub depth: f32,
    /// Reverse the surface relief direction to create a recessed appearance.
    pub down: bool,
    /// Local light azimuth in degrees: 0 from the right, 90 from above.
    pub angle: f32,
    /// Local light elevation above the canvas in degrees, in `0..=90`.
    pub elevation: f32,
    /// Use both document light angles instead of the local angles.
    pub use_global_light: bool,
    /// Straight highlight RGBA with finite RGB and alpha in `0..=1`.
    pub highlight_color: [f32; 4],
    /// Blend mode used for highlight planes.
    pub highlight_mode: BlendMode,
    /// Highlight compositing opacity in `0..=1`, separate from color alpha.
    pub highlight_opacity: f32,
    /// Straight shadow RGBA with finite RGB and alpha in `0..=1`.
    pub shadow_color: [f32; 4],
    /// Blend mode used for shadow planes.
    pub shadow_mode: BlendMode,
    /// Shadow compositing opacity in `0..=1`, separate from color alpha.
    pub shadow_opacity: f32,
    /// Preserved contour and jitter metadata; not evaluated by the renderer.
    pub shape: EffectShape,
}
impl Default for Bevel {
    fn default() -> Self {
        Self {
            enabled: true,
            kind: BevelKind::Inner,
            size: 5.0,
            soften: 0.0,
            depth: 1.0,
            down: false,
            angle: 120.0,
            elevation: 30.0,
            use_global_light: true,
            highlight_color: [1.0; 4],
            highlight_mode: BlendMode::Screen,
            highlight_opacity: 0.75,
            shadow_color: [0.0, 0.0, 0.0, 1.0],
            shadow_mode: BlendMode::Multiply,
            shadow_opacity: 0.75,
            shape: EffectShape::default(),
        }
    }
}

/// Interior shading derived from two oppositely offset blurred alpha fields.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Satin {
    /// Whether to render this effect; disabled settings are still validated.
    pub enabled: bool,
    /// Straight RGBA with finite RGB and alpha in `0..=1`.
    pub color: [f32; 4],
    /// Blend mode used to composite the satin plane.
    pub mode: BlendMode,
    /// Compositing opacity in `0..=1`, separate from color alpha.
    pub opacity: f32,
    /// Offset-axis angle in degrees; 0 is horizontal and 90 is vertical.
    /// Satin always uses this local angle, not the document light.
    pub angle: f32,
    /// Nonnegative displacement of each opposing offset in unscaled pixels.
    pub distance: f32,
    /// Nonnegative Gaussian support radius in pixels before style scaling.
    pub size: f32,
    /// Complement the absolute offset difference before clipping to source alpha.
    pub invert: bool,
    /// Preserved contour and jitter metadata; not evaluated by the renderer.
    pub shape: EffectShape,
}
impl Default for Satin {
    fn default() -> Self {
        Self {
            enabled: true,
            color: [0.0, 0.0, 0.0, 1.0],
            mode: BlendMode::Multiply,
            opacity: 0.5,
            angle: 19.0,
            distance: 11.0,
            size: 14.0,
            invert: false,
            shape: EffectShape::default(),
        }
    }
}

/// A sampled fill clipped to source alpha, independent of source RGB.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Overlay {
    /// Whether to render this effect; disabled settings are still validated.
    pub enabled: bool,
    /// Fill sampled at canvas pixel centers without layer-style scaling.
    /// Sampled alpha is multiplied by source alpha.
    pub fill: Fill,
    /// Blend mode used to composite the overlay plane.
    pub mode: BlendMode,
    /// Compositing opacity in `0..=1`, separate from sampled fill alpha.
    pub opacity: f32,
}
impl Default for Overlay {
    fn default() -> Self {
        Self {
            enabled: true,
            fill: Fill::Solid {
                color: [1.0, 0.0, 0.0],
            },
            mode: BlendMode::Normal,
            opacity: 1.0,
        }
    }
}
/// Placement of a stroke's total width relative to the source alpha boundary.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum StrokePosition {
    /// Extend the full width inward from the boundary.
    Inside,
    /// Extend half the width on each side of the boundary.
    Center,
    /// Extend the full width outward and composite behind the source layer.
    #[default]
    Outside,
}
/// A filled band obtained by square-footprint dilation and erosion of alpha.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Stroke {
    /// Whether to render this effect; disabled settings are still validated.
    pub enabled: bool,
    /// Fill sampled at canvas pixel centers without layer-style scaling.
    /// Sampled alpha is multiplied by stroke-band coverage.
    pub fill: Fill,
    /// Blend mode used to composite the stroke plane.
    pub mode: BlendMode,
    /// Compositing opacity in `0..=1`, separate from sampled fill alpha.
    pub opacity: f32,
    /// Nonnegative total stroke width in pixels before layer-style scaling.
    pub size: f32,
    /// Placement of the stroke width around the source alpha boundary.
    pub position: StrokePosition,
}
impl Default for Stroke {
    fn default() -> Self {
        Self {
            enabled: true,
            fill: Fill::Solid { color: [0.0; 3] },
            mode: BlendMode::Normal,
            opacity: 1.0,
            size: 3.0,
            position: StrokePosition::Outside,
        }
    }
}

/// Full-canvas effect buffer. Opacity is separate, not baked into alpha.
#[derive(Clone, Debug)]
pub(crate) struct StylePlane {
    /// Stroke coverage straddles the shape and is applied after the interior.
    pub stroke: bool,
    /// Full-canvas straight RGBA pixels with effect coverage baked into alpha.
    pub raster: Raster,
    /// Blend mode to use when compositing this plane.
    pub mode: BlendMode,
    /// Compositing opacity in `0..=1`, not baked into raster alpha.
    pub opacity: f32,
    /// Whether to composite behind the source layer rather than above it.
    pub outside: bool,
}

fn finite(v: f32, name: &str) -> EngineResult<()> {
    if v.is_finite() {
        Ok(())
    } else {
        Err(EngineError::invalid(
            "layer styles",
            format!("non-finite style {name}"),
        ))
    }
}
fn range(v: f32, lo: f32, hi: f32, name: &str) -> EngineResult<()> {
    finite(v, name)?;
    if (lo..=hi).contains(&v) {
        Ok(())
    } else {
        Err(EngineError::invalid(
            "layer styles",
            format!("style {name} outside {lo}..={hi}"),
        ))
    }
}
fn color(c: &[f32; 4]) -> EngineResult<()> {
    for v in c {
        finite(*v, "color")?;
    }
    range(c[3], 0.0, 1.0, "color alpha")
}
fn shape(s: &EffectShape) -> EngineResult<()> {
    range(s.jitter, 0.0, 1.0, "jitter")?;
    if s.contour.len() > 4096 {
        return Err(EngineError::invalid(
            "layer styles",
            "too many contour points",
        ));
    }
    for p in &s.contour {
        for v in p {
            range(*v, 0.0, 1.0, "contour")?;
        }
    }
    Ok(())
}
fn validate_fill(fill: &Fill) -> EngineResult<()> {
    match fill {
        Fill::Solid { color } => {
            for c in color {
                finite(*c, "fill color")?;
            }
        }
        Fill::Gradient {
            start, end, stops, ..
        } => {
            for c in start.iter().chain(end) {
                range(*c, -1.0e9, 1.0e9, "gradient coordinate")?;
            }
            if stops.is_empty() || stops.len() > 4096 {
                return Err(EngineError::invalid(
                    "layer styles",
                    "invalid style gradient stops",
                ));
            }
            let mut prev = 0.0;
            for s in stops {
                range(s.position, prev, 1.0, "gradient stop")?;
                color(&s.color)?;
                prev = s.position;
            }
        }
        Fill::Pattern {
            width,
            height,
            rgba,
            origin,
        } => {
            let count = width.checked_mul(*height).and_then(|n| n.checked_mul(4));
            if *width == 0
                || *height == 0
                || count.map(|n| n as usize) != Some(rgba.len())
                || rgba.len() > MAX_PIXELS * 4
            {
                return Err(EngineError::invalid(
                    "layer styles",
                    "invalid style pattern dimensions",
                ));
            }
            for c in rgba.as_chunks::<4>().0 {
                color(c)?;
            }
            for c in origin {
                range(*c, -1.0e9, 1.0e9, "pattern origin")?;
            }
        }
    }
    Ok(())
}
impl LayerStyles {
    /// Validate even disabled effects so enabling one cannot expose NaNs or
    /// unbounded kernels. A style scale is a multiplier, not a percentage.
    ///
    /// # Errors
    /// Returns an invalid-input error for non-finite values, invalid fills or
    /// normalized controls, more than 64 effects, or out-of-range geometry.
    /// Scaled radii (including combined blur and spread/soften support) may
    /// not exceed 256 pixels; scaled distances may not exceed 16384 pixels.
    pub fn validate(&self) -> EngineResult<()> {
        range(self.scale, 0.0, 100.0, "scale")?;
        if self.effects.len() > 64 {
            return Err(EngineError::invalid(
                "layer styles",
                "too many layer effects",
            ));
        }
        let radius = |r: f32| -> EngineResult<()> {
            range(r, 0.0, MAX_DISTANCE, "size")?;
            range(r * self.scale, 0.0, MAX_RADIUS, "scaled radius")
        };
        let distance = |r: f32| -> EngineResult<()> {
            range(r, 0.0, MAX_DISTANCE, "distance")?;
            range(r * self.scale, 0.0, MAX_DISTANCE, "scaled distance")
        };
        for e in &self.effects {
            match e {
                StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => {
                    color(&s.color)?;
                    range(s.opacity, 0.0, 1.0, "opacity")?;
                    finite(s.angle, "angle")?;
                    radius(s.size)?;
                    radius(s.spread)?;
                    radius(s.size + s.spread)?;
                    distance(s.distance)?;
                    shape(&s.shape)?;
                }
                StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => {
                    color(&s.color)?;
                    range(s.opacity, 0.0, 1.0, "opacity")?;
                    radius(s.size)?;
                    radius(s.spread)?;
                    radius(s.size + s.spread)?;
                    shape(&s.shape)?;
                }
                StyleEffect::Bevel(s) => {
                    color(&s.highlight_color)?;
                    color(&s.shadow_color)?;
                    range(s.highlight_opacity, 0.0, 1.0, "highlight opacity")?;
                    range(s.shadow_opacity, 0.0, 1.0, "shadow opacity")?;
                    radius(s.size)?;
                    radius(s.soften)?;
                    radius(s.size + s.soften)?;
                    range(s.depth, 0.0, 100.0, "bevel depth")?;
                    GlobalLight {
                        angle: s.angle,
                        elevation: s.elevation,
                    }
                    .validate()?;
                    shape(&s.shape)?;
                }
                StyleEffect::Satin(s) => {
                    color(&s.color)?;
                    range(s.opacity, 0.0, 1.0, "opacity")?;
                    finite(s.angle, "angle")?;
                    radius(s.size)?;
                    distance(s.distance)?;
                    shape(&s.shape)?;
                }
                StyleEffect::Overlay(s)
                | StyleEffect::ColorOverlay(s)
                | StyleEffect::GradientOverlay(s)
                | StyleEffect::PatternOverlay(s) => {
                    validate_fill(&s.fill)?;
                    range(s.opacity, 0.0, 1.0, "opacity")?;
                }
                StyleEffect::Stroke(s) => {
                    validate_fill(&s.fill)?;
                    range(s.opacity, 0.0, 1.0, "opacity")?;
                    radius(s.size)?;
                }
            }
        }
        Ok(())
    }
}

/// Padded scalar alpha field. Padding prevents blur energy outside the
/// canvas from being lost before an offset moves it back into the canvas.
#[derive(Clone)]
struct Mask {
    w: usize,
    h: usize,
    data: Vec<f32>,
}
impl Mask {
    fn at(&self, x: isize, y: isize) -> f32 {
        if x < 0 || y < 0 || x >= self.w as isize || y >= self.h as isize {
            0.0
        } else {
            self.data[y as usize * self.w + x as usize]
        }
    }
    fn sample(&self, x: f32, y: f32) -> f32 {
        let (ix, iy) = (x.floor() as isize, y.floor() as isize);
        let (fx, fy) = (x - x.floor(), y - y.floor());
        let a = self.at(ix, iy) * (1.0 - fx) + self.at(ix + 1, iy) * fx;
        let b = self.at(ix, iy + 1) * (1.0 - fx) + self.at(ix + 1, iy + 1) * fx;
        a * (1.0 - fy) + b * fy
    }
    fn map(&self, f: impl Fn(f32) -> f32) -> Self {
        Self {
            w: self.w,
            h: self.h,
            data: self.data.iter().map(|v| f(*v)).collect(),
        }
    }
}

/// Separable Gaussian, zero beyond padded support. Size is support radius;
/// sigma=size/3. No per-edge renormalization (outside the shape is empty).
fn blur(a: &Mask, size: f32) -> Mask {
    if size <= 0.0 {
        return a.clone();
    }
    let r = size.ceil() as isize;
    let sigma = (size / 3.0).max(0.01);
    let mut weights: Vec<f32> = (-r..=r)
        .map(|i| (-0.5 * (i as f32 / sigma).powi(2)).exp())
        .collect();
    let sum: f32 = weights.iter().sum();
    for v in &mut weights {
        *v /= sum;
    }
    let mut tmp = a.map(|_| 0.0);
    let mut out = tmp.clone();
    for y in 0..a.h {
        for x in 0..a.w {
            tmp.data[y * a.w + x] = weights
                .iter()
                .enumerate()
                .map(|(k, w)| w * a.at(x as isize + k as isize - r, y as isize))
                .sum();
        }
    }
    for y in 0..a.h {
        for x in 0..a.w {
            out.data[y * a.w + x] = weights
                .iter()
                .enumerate()
                .map(|(k, w)| w * tmp.at(x as isize, y as isize + k as isize - r))
                .sum();
        }
    }
    out
}

/// Linear-time separable square dilation/erosion; fractional radii blend
/// adjacent integer footprints rather than rounding away style scaling.
fn morphology(a: &Mask, radius: f32, dilate: bool) -> Mask {
    fn integer(a: &Mask, r: usize, dilate: bool) -> Mask {
        if r == 0 {
            return a.clone();
        }
        let mut src = a.clone();
        for vertical in [false, true] {
            let mut dst = src.map(|_| 0.0);
            let (lines, n) = if vertical { (a.w, a.h) } else { (a.h, a.w) };
            for line in 0..lines {
                let mut q: VecDeque<(isize, f32)> = VecDeque::new();
                for t in -(r as isize)..n as isize + r as isize {
                    let v = if vertical {
                        src.at(line as isize, t)
                    } else {
                        src.at(t, line as isize)
                    };
                    while q
                        .back()
                        .is_some_and(|(_, b)| if dilate { *b <= v } else { *b >= v })
                    {
                        q.pop_back();
                    }
                    q.push_back((t, v));
                    let x = t - r as isize;
                    while q.front().is_some_and(|(i, _)| *i < x - r as isize) {
                        q.pop_front();
                    }
                    if x >= 0 && x < n as isize {
                        let idx = if vertical {
                            x as usize * a.w + line
                        } else {
                            line * a.w + x as usize
                        };
                        dst.data[idx] = q.front().unwrap().1;
                    }
                }
            }
            src = dst;
        }
        src
    }
    let lo = radius.floor() as usize;
    let mut out = integer(a, lo, dilate);
    let f = radius - lo as f32;
    if f > 0.0 {
        let hi = integer(a, lo + 1, dilate);
        for (v, h) in out.data.iter_mut().zip(hi.data) {
            *v += (h - *v) * f;
        }
    }
    out
}
fn offset(angle: f32, distance: f32) -> (f32, f32) {
    let a = angle.rem_euclid(360.0).to_radians();
    // Light at 0 is on the right: shadow travels left; image y points down.
    (-a.cos() * distance, a.sin() * distance)
}
fn rank(e: &StyleEffect) -> u8 {
    match e {
        StyleEffect::DropShadow(_) => 0,
        StyleEffect::OuterGlow(_) => 1,
        StyleEffect::PatternOverlay(_) => 2,
        StyleEffect::GradientOverlay(_) => 3,
        StyleEffect::ColorOverlay(_) | StyleEffect::Overlay(_) => 4,
        StyleEffect::Satin(_) => 5,
        StyleEffect::InnerGlow(_) => 6,
        StyleEffect::InnerShadow(_) => 7,
        StyleEffect::Stroke(_) => 8,
        StyleEffect::Bevel(_) => 9,
    }
}
fn support(e: &StyleEffect) -> f32 {
    match e {
        StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) => s.size + s.spread,
        StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) => s.size + s.spread,
        StyleEffect::Bevel(s) => s.size + s.soften,
        StyleEffect::Satin(s) => s.size,
        StyleEffect::Stroke(s) => s.size,
        _ => 0.0,
    }
}

pub(crate) fn render(
    input: &Raster,
    styles: &LayerStyles,
    light: GlobalLight,
) -> EngineResult<Vec<StylePlane>> {
    styles.validate()?;
    light.validate()?;
    if input.channels() != 4 {
        return Err(EngineError::invalid(
            "layer styles",
            "styles require straight RGBA",
        ));
    }
    if styles.effects.is_empty() {
        return Ok(Vec::new());
    }
    let extent = input.extent();
    if extent.width == 0 || extent.height == 0 {
        return Ok(Vec::new());
    }
    let pad =
        (styles.effects.iter().map(support).fold(0.0, f32::max) * styles.scale).ceil() as usize + 2;
    let (w, h) = (
        extent.width as usize + 2 * pad,
        extent.height as usize + 2 * pad,
    );
    let count = w
        .checked_mul(h)
        .filter(|n| *n <= MAX_PIXELS)
        .ok_or_else(|| {
            EngineError::invalid("layer styles", "style alpha canvas exceeds CPU pixel limit")
        })?;
    let mut alpha = Mask {
        w,
        h,
        data: vec![0.0; count],
    };
    for y in 0..extent.height {
        for x in 0..extent.width {
            let a = input.pixel(x, y)[3];
            finite(a, "source alpha")?;
            alpha.data[(y as usize + pad) * w + x as usize + pad] = a.clamp(0.0, 1.0);
        }
    }
    let mut planes = Vec::new();
    let mut effects: Vec<_> = styles.effects.iter().collect();
    effects.sort_by_key(|e| rank(e));
    // Raster buffers are F32 to avoid quantizing a fractional effect before blending.
    let mut emit = |mode: BlendMode,
                    opacity: f32,
                    outside: bool,
                    stroke: bool,
                    paint: &dyn Fn(usize, usize) -> [f32; 4]|
     -> EngineResult<()> {
        let mut raster = Raster::new(extent, 4, Depth::F32, 0.0);
        raster.edit_region(Rect::of_extent(extent), 1, |x, y, p| {
            *p = paint(x as usize + pad, y as usize + pad);
            p[3] = p[3].clamp(0.0, 1.0);
        })?;
        planes.push(StylePlane {
            stroke,
            raster,
            mode,
            opacity,
            outside,
        });
        Ok(())
    };
    let sc = styles.scale;
    for e in effects {
        match e {
            StyleEffect::DropShadow(s) | StyleEffect::InnerShadow(s) if s.enabled => {
                let inner = matches!(e, StyleEffect::InnerShadow(_));
                // Inner choke erodes the source before complementing it. This
                // also treats the infinite canvas exterior as transparent.
                let field = blur(&morphology(&alpha, s.spread * sc, !inner), s.size * sc);
                let (dx, dy) = offset(
                    if s.use_global_light {
                        light.angle
                    } else {
                        s.angle
                    },
                    s.distance * sc,
                );
                emit(s.mode, s.opacity, !inner, false, &|x, y| {
                    let a = alpha.data[y * w + x];
                    let shifted = field.sample(x as f32 - dx, y as f32 - dy);
                    let coverage = if inner { a * (1.0 - shifted) } else { shifted };
                    let mut c = s.color;
                    c[3] *= coverage;
                    c
                })?;
            }
            StyleEffect::OuterGlow(s) | StyleEffect::InnerGlow(s) if s.enabled => {
                let inner = matches!(e, StyleEffect::InnerGlow(_));
                let field = blur(&morphology(&alpha, s.spread * sc, !inner), s.size * sc);
                emit(s.mode, s.opacity, !inner, false, &|x, y| {
                    let i = y * w + x;
                    let a = alpha.data[i];
                    let coverage = if inner {
                        a * if s.center {
                            field.data[i]
                        } else {
                            1.0 - field.data[i]
                        }
                    } else {
                        field.data[i]
                    };
                    let mut c = s.color;
                    c[3] *= coverage;
                    c
                })?;
            }
            StyleEffect::Overlay(s)
            | StyleEffect::ColorOverlay(s)
            | StyleEffect::GradientOverlay(s)
            | StyleEffect::PatternOverlay(s)
                if s.enabled =>
            {
                emit(s.mode, s.opacity, false, false, &|x, y| {
                    let mut c = s
                        .fill
                        .sample((x - pad) as f32 + 0.5, (y - pad) as f32 + 0.5);
                    c[3] *= alpha.data[y * w + x];
                    c
                })?;
            }
            StyleEffect::Stroke(s) if s.enabled => {
                let r = s.size * sc;
                let (outer, inner) = match s.position {
                    StrokePosition::Outside => (morphology(&alpha, r, true), alpha.clone()),
                    StrokePosition::Inside => (alpha.clone(), morphology(&alpha, r, false)),
                    StrokePosition::Center => (
                        morphology(&alpha, r * 0.5, true),
                        morphology(&alpha, r * 0.5, false),
                    ),
                };
                emit(
                    s.mode,
                    s.opacity,
                    s.position == StrokePosition::Outside,
                    true,
                    &|x, y| {
                        let mut c = s
                            .fill
                            .sample((x - pad) as f32 + 0.5, (y - pad) as f32 + 0.5);
                        c[3] *= (outer.data[y * w + x] - inner.data[y * w + x]).max(0.0);
                        c
                    },
                )?;
            }
            StyleEffect::Satin(s) if s.enabled => {
                let field = blur(&alpha, s.size * sc);
                let (dx, dy) = offset(s.angle, s.distance * sc);
                emit(s.mode, s.opacity, false, false, &|x, y| {
                    let d = (field.sample(x as f32 - dx, y as f32 - dy)
                        - field.sample(x as f32 + dx, y as f32 + dy))
                    .abs();
                    let mut c = s.color;
                    c[3] *= alpha.data[y * w + x] * if s.invert { 1.0 - d } else { d };
                    c
                })?;
            }
            StyleEffect::Bevel(s) if s.enabled => {
                let height = blur(&blur(&alpha, s.size * sc), s.soften * sc);
                let l = if s.use_global_light {
                    light
                } else {
                    GlobalLight {
                        angle: s.angle,
                        elevation: s.elevation,
                    }
                };
                let angle = l.angle.rem_euclid(360.0).to_radians();
                let elevation = l.elevation.to_radians();
                let (lx, ly) = (
                    angle.cos() * elevation.cos(),
                    -angle.sin() * elevation.cos(),
                );
                let outer = morphology(&alpha, s.size * sc, true);
                let inner = morphology(&alpha, s.size * sc, false);
                // Emit outside then inside pieces for emboss; parent can place
                // the former behind source without hiding the inside highlight.
                for outside in [true, false] {
                    if (outside && s.kind == BevelKind::Inner)
                        || (!outside && s.kind == BevelKind::Outer)
                    {
                        continue;
                    }
                    for highlight in [false, true] {
                        let (c, mode, opacity) = if highlight {
                            (s.highlight_color, s.highlight_mode, s.highlight_opacity)
                        } else {
                            (s.shadow_color, s.shadow_mode, s.shadow_opacity)
                        };
                        emit(mode, opacity, outside, false, &|x, y| {
                            let i = y * w + x;
                            let (x0, y0) = (x as isize, y as isize);
                            let gx = (height.at(x0 + 1, y0) - height.at(x0 - 1, y0)) * 0.5;
                            let gy = (height.at(x0, y0 + 1) - height.at(x0, y0 - 1)) * 0.5;
                            let direction = if s.down { -1.0 } else { 1.0 }
                                * if s.kind == BevelKind::Pillow && !outside {
                                    -1.0
                                } else {
                                    1.0
                                };
                            let strength = s.depth * s.size * sc;
                            let (nx, ny) = (-gx * strength * direction, -gy * strength * direction);
                            let illumination = (nx * lx + ny * ly + elevation.sin())
                                / (nx * nx + ny * ny + 1.0).sqrt()
                                - elevation.sin();
                            let band = if outside {
                                (outer.data[i] - alpha.data[i]).max(0.0)
                            } else {
                                (alpha.data[i] - inner.data[i]).max(0.0)
                            };
                            let mut c = c;
                            c[3] *= band
                                * if highlight {
                                    illumination.max(0.0)
                                } else {
                                    (-illumination).max(0.0)
                                };
                            c
                        })?;
                    }
                }
            }
            _ => {}
        }
    }
    // Stable partition retains Photoshop rank within behind/above groups.
    planes.sort_by_key(|p| !p.outside);
    Ok(planes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine_api::tile::Extent;
    fn dot() -> Raster {
        let mut r = Raster::new(Extent::new(7, 7), 4, Depth::F32, 0.0);
        r.edit_region(Rect::new(3, 3, 4, 4), 1, |_, _, p| {
            *p = [0.2, 0.4, 0.6, 1.0]
        })
        .unwrap();
        r
    }
    fn one(r: &Raster, effect: StyleEffect, scale: f32, light: GlobalLight) -> Vec<StylePlane> {
        render(
            r,
            &LayerStyles {
                effects: vec![effect],
                scale,
            },
            light,
        )
        .unwrap()
    }
    fn near(a: f32, b: f32) {
        assert!((a - b).abs() < 1e-5, "{a} != {b}");
    }
    fn square() -> Raster {
        let mut r = dot();
        r.edit_region(Rect::new(2, 2, 5, 5), 2, |_, _, p| {
            *p = [0.2, 0.4, 0.6, 1.0]
        })
        .unwrap();
        r
    }
    #[test]
    fn shadow_distance_scales_and_uses_global_or_local_light() {
        let s = Shadow {
            size: 0.0,
            distance: 1.0,
            angle: 0.0,
            ..Shadow::default()
        };
        let light = GlobalLight {
            angle: 180.0,
            elevation: 30.0,
        };
        let p = one(&dot(), StyleEffect::DropShadow(s.clone()), 2.0, light);
        near(p[0].raster.pixel(5, 3)[3], 1.0);
        near(p[0].raster.pixel(4, 3)[3], 0.0);
        assert!(p[0].outside);
        let p = one(
            &dot(),
            StyleEffect::DropShadow(Shadow {
                use_global_light: false,
                ..s
            }),
            1.0,
            light,
        );
        near(p[0].raster.pixel(2, 3)[3], 1.0);
    }
    #[test]
    fn inner_shadow_is_complement_of_shifted_alpha() {
        let s = Shadow {
            size: 0.0,
            distance: 1.0,
            angle: 0.0,
            use_global_light: false,
            ..Shadow::default()
        };
        let p = one(
            &square(),
            StyleEffect::InnerShadow(s),
            1.0,
            GlobalLight::default(),
        );
        near(p[0].raster.pixel(4, 3)[3], 1.0);
        near(p[0].raster.pixel(3, 3)[3], 0.0);
        near(p[0].raster.pixel(5, 3)[3], 0.0);
        assert!(!p[0].outside);
    }
    #[test]
    fn outer_glow_gaussian_matches_impulse_kernel() {
        let p = one(
            &dot(),
            StyleEffect::OuterGlow(Glow {
                size: 3.0,
                ..Glow::default()
            }),
            1.0,
            GlobalLight::default(),
        );
        // Radius 3 => sigma 1, so the 2D center is 1/Z^2.
        let z: f32 = (-3..=3).map(|k| (-0.5 * (k * k) as f32).exp()).sum();
        near(p[0].raster.pixel(3, 3)[3], 1.0 / (z * z));
        near(p[0].raster.pixel(4, 3)[3], (-0.5f32).exp() / (z * z));
        assert!(p[0].outside);
    }
    #[test]
    fn spread_and_blur_size_scale() {
        let g = Glow {
            size: 0.0,
            spread: 1.0,
            ..Glow::default()
        };
        let p = one(
            &dot(),
            StyleEffect::OuterGlow(g),
            2.0,
            GlobalLight::default(),
        );
        near(p[0].raster.pixel(1, 3)[3], 1.0);
        near(p[0].raster.pixel(0, 3)[3], 0.0);
        let p = one(
            &dot(),
            StyleEffect::OuterGlow(Glow {
                size: 1.5,
                ..Glow::default()
            }),
            2.0,
            GlobalLight::default(),
        );
        let q = one(
            &dot(),
            StyleEffect::OuterGlow(Glow {
                size: 3.0,
                ..Glow::default()
            }),
            1.0,
            GlobalLight::default(),
        );
        for x in 0..7 {
            near(p[0].raster.pixel(x, 3)[3], q[0].raster.pixel(x, 3)[3]);
        }
    }
    #[test]
    fn inner_glow_edge_and_center_are_complements() {
        let g = Glow {
            size: 0.0,
            spread: 1.0,
            ..Glow::default()
        };
        let edge = one(
            &square(),
            StyleEffect::InnerGlow(g.clone()),
            1.0,
            GlobalLight::default(),
        );
        let center = one(
            &square(),
            StyleEffect::InnerGlow(Glow { center: true, ..g }),
            1.0,
            GlobalLight::default(),
        );
        near(edge[0].raster.pixel(2, 3)[3], 1.0);
        near(edge[0].raster.pixel(3, 3)[3], 0.0);
        near(center[0].raster.pixel(3, 3)[3], 1.0);
        near(center[0].raster.pixel(2, 3)[3], 0.0);
    }
    #[test]
    fn all_stroke_positions_use_morphological_bands() {
        for (position, size, edge, center, exterior) in [
            (StrokePosition::Outside, 1.0, 0.0, 0.0, 1.0),
            (StrokePosition::Inside, 1.0, 1.0, 0.0, 0.0),
            (StrokePosition::Center, 2.0, 1.0, 0.0, 1.0),
        ] {
            let p = one(
                &square(),
                StyleEffect::Stroke(Stroke {
                    position,
                    size,
                    ..Stroke::default()
                }),
                1.0,
                GlobalLight::default(),
            );
            near(p[0].raster.pixel(2, 3)[3], edge);
            near(p[0].raster.pixel(3, 3)[3], center);
            near(p[0].raster.pixel(1, 3)[3], exterior);
            assert_eq!(p[0].outside, position == StrokePosition::Outside);
        }
        let p = one(
            &dot(),
            StyleEffect::Stroke(Stroke {
                size: 1.0,
                ..Stroke::default()
            }),
            0.5,
            GlobalLight::default(),
        );
        near(p[0].raster.pixel(2, 3)[3], 0.5);
    }
    #[test]
    fn satin_is_difference_of_opposite_offsets_and_can_invert() {
        let s = Satin {
            size: 0.0,
            distance: 1.0,
            angle: 0.0,
            ..Satin::default()
        };
        let p = one(
            &square(),
            StyleEffect::Satin(s.clone()),
            1.0,
            GlobalLight::default(),
        );
        near(p[0].raster.pixel(2, 3)[3], 1.0);
        near(p[0].raster.pixel(3, 3)[3], 0.0);
        let p = one(
            &square(),
            StyleEffect::Satin(Satin { invert: true, ..s }),
            1.0,
            GlobalLight::default(),
        );
        near(p[0].raster.pixel(2, 3)[3], 0.0);
        near(p[0].raster.pixel(3, 3)[3], 1.0);
    }
    #[test]
    fn bevel_light_reversal_swaps_highlight_edge() {
        let b = Bevel {
            size: 1.0,
            ..Bevel::default()
        };
        let p = one(
            &square(),
            StyleEffect::Bevel(b.clone()),
            1.0,
            GlobalLight {
                angle: 0.0,
                elevation: 0.0,
            },
        );
        let q = one(
            &square(),
            StyleEffect::Bevel(b.clone()),
            1.0,
            GlobalLight {
                angle: 180.0,
                elevation: 0.0,
            },
        );
        assert_eq!(p.len(), 2);
        assert_eq!(p[1].mode, BlendMode::Screen);
        let k = (-4.5f32).exp();
        let slope = (1.0 - k / (1.0 + 2.0 * k)) * 0.5;
        near(
            p[1].raster.pixel(4, 3)[3],
            slope / (1.0 + slope * slope).sqrt(),
        );
        near(p[1].raster.pixel(2, 3)[3], 0.0);
        near(p[1].raster.pixel(4, 3)[3], q[1].raster.pixel(2, 3)[3]);
        near(p[1].raster.pixel(3, 3)[3], 0.0);
        let high = one(
            &square(),
            StyleEffect::Bevel(b),
            1.0,
            GlobalLight {
                angle: 0.0,
                elevation: 90.0,
            },
        );
        near(high[1].raster.pixel(4, 3)[3], 0.0);
        let outer = one(
            &square(),
            StyleEffect::Bevel(Bevel {
                kind: BevelKind::Outer,
                size: 1.0,
                ..Bevel::default()
            }),
            1.0,
            GlobalLight::default(),
        );
        assert!(outer.iter().all(|p| p.outside));
    }
    #[test]
    fn gradient_and_pattern_overlays_use_fill_sampler_and_fill_alpha() {
        use crate::document::{GradientKind, GradientStop};
        let gradient = Fill::Gradient {
            gradient: GradientKind::Linear,
            start: [3.0, 0.0],
            end: [4.0, 0.0],
            stops: vec![
                GradientStop {
                    position: 0.0,
                    color: [0.0, 0.0, 0.0, 0.25],
                },
                GradientStop {
                    position: 1.0,
                    color: [1.0, 0.0, 0.0, 0.75],
                },
            ],
        };
        let p = one(
            &dot(),
            StyleEffect::GradientOverlay(Overlay {
                fill: gradient,
                ..Overlay::default()
            }),
            1.0,
            GlobalLight::default(),
        );
        assert_eq!(p[0].raster.pixel(3, 3), [0.5, 0.0, 0.0, 0.5]);
        let pattern = Fill::Pattern {
            width: 1,
            height: 1,
            rgba: vec![0.0, 1.0, 0.0, 0.5],
            origin: [0.0; 2],
        };
        let p = one(
            &dot(),
            StyleEffect::PatternOverlay(Overlay {
                fill: pattern,
                ..Overlay::default()
            }),
            1.0,
            GlobalLight::default(),
        );
        assert_eq!(p[0].raster.pixel(3, 3), [0.0, 1.0, 0.0, 0.5]);
    }
    #[test]
    fn effects_are_stably_sorted_behind_and_above() {
        let styles = LayerStyles {
            effects: vec![
                StyleEffect::ColorOverlay(Overlay {
                    mode: BlendMode::Color,
                    ..Overlay::default()
                }),
                StyleEffect::Stroke(Stroke::default()),
                StyleEffect::DropShadow(Shadow::default()),
                StyleEffect::PatternOverlay(Overlay {
                    mode: BlendMode::Difference,
                    ..Overlay::default()
                }),
                StyleEffect::ColorOverlay(Overlay {
                    mode: BlendMode::Hue,
                    ..Overlay::default()
                }),
            ],
            ..LayerStyles::default()
        };
        let p = render(&dot(), &styles, GlobalLight::default()).unwrap();
        assert_eq!(
            p.iter().map(|p| (p.outside, p.mode)).collect::<Vec<_>>(),
            vec![
                (true, BlendMode::Multiply),
                (true, BlendMode::Normal),
                (false, BlendMode::Difference),
                (false, BlendMode::Color),
                (false, BlendMode::Hue)
            ]
        );
    }
    #[test]
    fn empty_disabled_transparent_and_zero_size_are_safe() {
        assert!(
            render(&dot(), &LayerStyles::default(), GlobalLight::default())
                .unwrap()
                .is_empty()
        );
        assert!(
            one(
                &dot(),
                StyleEffect::Stroke(Stroke {
                    enabled: false,
                    ..Stroke::default()
                }),
                1.0,
                GlobalLight::default()
            )
            .is_empty()
        );
        let blank = Raster::new(Extent::new(1, 1), 4, Depth::F32, 0.0);
        let p = one(
            &blank,
            StyleEffect::InnerGlow(Glow::default()),
            1.0,
            GlobalLight::default(),
        );
        near(p[0].raster.pixel(0, 0)[3], 0.0);
        let p = one(
            &dot(),
            StyleEffect::Stroke(Stroke::default()),
            0.0,
            GlobalLight::default(),
        );
        for y in 0..7 {
            for x in 0..7 {
                near(p[0].raster.pixel(x, y)[3], 0.0);
            }
        }
    }
    #[test]
    fn invalid_numbers_kernels_and_patterns_are_rejected() {
        for scale in [f32::NAN, f32::INFINITY, -1.0, 101.0] {
            assert!(
                LayerStyles {
                    scale,
                    ..LayerStyles::default()
                }
                .validate()
                .is_err()
            );
        }
        let mut s = LayerStyles {
            effects: vec![StyleEffect::Stroke(Stroke {
                size: 257.0,
                ..Stroke::default()
            })],
            ..LayerStyles::default()
        };
        assert!(s.validate().is_err());
        s.effects = vec![StyleEffect::Overlay(Overlay {
            fill: Fill::Pattern {
                width: u32::MAX,
                height: 2,
                rgba: vec![],
                origin: [0.0; 2],
            },
            ..Overlay::default()
        })];
        assert!(s.validate().is_err());
        assert!(
            GlobalLight {
                angle: f32::NAN,
                ..GlobalLight::default()
            }
            .validate()
            .is_err()
        );
    }
    #[test]
    fn serde_round_trip_preserves_placeholder_controls_and_defaults() {
        assert_eq!(
            serde_json::from_str::<LayerStyles>("{}").unwrap(),
            LayerStyles::default()
        );
        assert_eq!(
            GlobalLight::default(),
            GlobalLight {
                angle: 120.0,
                elevation: 30.0
            }
        );
        let s = LayerStyles {
            effects: vec![StyleEffect::OuterGlow(Glow {
                shape: EffectShape {
                    contour: vec![[0.0, 0.0], [0.5, 0.25], [1.0, 1.0]],
                    jitter: 0.5,
                },
                ..Glow::default()
            })],
            scale: 2.0,
        };
        assert_eq!(
            serde_json::from_str::<LayerStyles>(&serde_json::to_string(&s).unwrap()).unwrap(),
            s
        );
    }
    #[test]
    fn overlay_is_straight_and_shape_clipped() {
        let planes = render(
            &dot(),
            &LayerStyles {
                effects: vec![StyleEffect::ColorOverlay(Overlay {
                    fill: Fill::Solid {
                        color: [1.0, 0.0, 0.0],
                    },
                    opacity: 0.5,
                    ..Overlay::default()
                })],
                ..LayerStyles::default()
            },
            GlobalLight::default(),
        )
        .unwrap();
        assert_eq!(planes.len(), 1);
        assert_eq!(planes[0].raster.pixel(3, 3), [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(planes[0].raster.pixel(2, 3)[3], 0.0);
        assert_eq!(planes[0].opacity, 0.5);
        assert!(!planes[0].outside);
    }
}
