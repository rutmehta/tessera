//! Same-level procedural mask rasterisation over scene-linear Rec.2020 RGB.
//!
//! Coordinates are normalized to extent; ellipse rotation is in normalized space.
//! Feather uses a cubic smoothstep across the inner edge. Component opacity is
//! independent of LocalAdjustment amount/enabled (the application stage owns those).
//! Empty component lists select nothing, even with group inversion enabled.
//!
//! Brush radius is relative to image width, so stamps are circular in pixels.
//! Pressure scales opacity (not radius); paint uses `a + (1-a)*alpha`, erase
//! uses `a*(1-alpha)`. Paths interpolate pressure and position at quarter-radius
//! spacing, with a quarter-pixel minimum. Coordinates outside [-16,16], radii
//! outside [1e-6,16], and more than one million interpolated stamps are rejected.
//! Luminance/depth bands have exterior smoothstep shoulders of smoothness/200;
//! color selection uses the nearest Euclidean OkLab distance, tolerance amount/100,
//! with an interior smoothstep shoulder controlled by `color_smoothness`.
//! Guided refinement uses linear luminance and truncated square windows, after
//! component composition and before group inversion. It is disabled by default.
use crate::Image;
use engine_api::{
    EngineError, EngineResult,
    recipe::mask::{LocalAdjustment, MaskCombine, MaskKind},
};
/// Optional edge-aware refinement in level pixels.
#[derive(Clone, Copy, Debug)]
pub struct GuidedRefinement {
    /// Window radius at the requested pyramid level; zero is identity.
    pub radius: u32,
    /// Positive, finite regularization in squared linear luminance units.
    pub epsilon: f32,
}
/// Runtime inputs absent from the procedural recipe schema.
#[derive(Clone, Copy, Debug)]
pub struct MaskOptions<'a> {
    /// Same-level normalized near-to-far depth, one finite 0..=1 sample per pixel.
    pub depth: Option<&'a [f32]>,
    /// Optional edge-aware refinement of the composite mask.
    pub refinement: Option<GuidedRefinement>,
    /// Color-range edge smoothness, 0..=100; default 50. Not stored in MaskKind.
    pub color_smoothness: f32,
}
impl Default for MaskOptions<'_> {
    fn default() -> Self {
        Self {
            depth: None,
            refinement: None,
            color_smoothness: 50.,
        }
    }
}
/// Rasterise pixel centres normalized to the complete input extent.
pub fn rasterize(
    input: &Image,
    group: &LocalAdjustment,
    options: MaskOptions<'_>,
) -> EngineResult<Vec<f32>> {
    validate(input, group, options)?;
    let w = input.width() as usize;
    let h = input.height() as usize;
    let mut out = vec![0.; w * h];
    if group.components.is_empty() {
        return Ok(out);
    }
    for (index, component) in group.components.iter().enumerate() {
        let mut plane = vec![0.; w * h];
        match component.kind {
            MaskKind::Linear { start, end } => {
                let dx = end[0] - start[0];
                let dy = end[1] - start[1];
                for (i, v) in plane.iter_mut().enumerate() {
                    let x = ((i % w) as f32 + 0.5) / w as f32;
                    let y = ((i / w) as f32 + 0.5) / h as f32;
                    *v = (1. - ((x - start[0]) * dx + (y - start[1]) * dy) / (dx * dx + dy * dy))
                        .clamp(0., 1.);
                }
            }
            MaskKind::Radial {
                center,
                radii,
                angle,
                feather,
            } => {
                let (s, c) = angle.to_radians().sin_cos();
                for (i, v) in plane.iter_mut().enumerate() {
                    let x = ((i % w) as f32 + 0.5) / w as f32 - center[0];
                    let y = ((i / w) as f32 + 0.5) / h as f32 - center[1];
                    let d = ((c * x + s * y) / radii[0]).hypot((-s * x + c * y) / radii[1]);
                    *v = falloff(d, feather / 100.);
                }
            }
            MaskKind::Brush { ref strokes } => {
                for stroke in strokes {
                    let radius = stroke.radius * w as f32;
                    let stamp = |plane: &mut [f32], p: [f32; 3]| {
                        let cx = p[0] * w as f32;
                        let cy = p[1] * h as f32;
                        let x0 = (cx - radius).floor().max(0.) as usize;
                        let x1 = ((cx + radius).ceil().max(0.) as usize).min(w);
                        let y0 = (cy - radius).floor().max(0.) as usize;
                        let y1 = ((cy + radius).ceil().max(0.) as usize).min(h);
                        for y in y0..y1 {
                            for x in x0..x1 {
                                let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) / radius;
                                let alpha =
                                    falloff(d, stroke.feather / 100.) * p[2] * stroke.flow / 100.;
                                let a = &mut plane[y * w + x];
                                *a = if stroke.erase {
                                    *a * (1. - alpha)
                                } else {
                                    *a + (1. - *a) * alpha
                                };
                            }
                        }
                    };
                    if let Some(&p) = stroke.points.first() {
                        stamp(&mut plane, p);
                    }
                    for pair in stroke.points.windows(2) {
                        let [a, b] = [pair[0], pair[1]];
                        let distance = ((b[0] - a[0]) * w as f32).hypot((b[1] - a[1]) * h as f32);
                        let steps = (distance / (radius * 0.25).max(0.25)).ceil().max(1.) as usize;
                        for step in 1..=steps {
                            let t = step as f32 / steps as f32;
                            stamp(
                                &mut plane,
                                std::array::from_fn(|c| a[c] + (b[c] - a[c]) * t),
                            );
                        }
                    }
                }
            }
            MaskKind::LuminanceRange { range, smoothness } => {
                for (i, v) in plane.iter_mut().enumerate() {
                    *v = band(luminance(input, i), range, smoothness / 200.);
                }
            }
            MaskKind::Depth { range, feather, .. } => {
                let depth = options.depth.ok_or_else(|| {
                    EngineError::invalid("mask.depth", "same-level depth plane required")
                })?;
                if depth.len() != plane.len() {
                    return Err(EngineError::invalid("mask.depth", "wrong plane length"));
                }
                for (v, &d) in plane.iter_mut().zip(depth) {
                    *v = band(d, range, feather / 200.);
                }
            }
            MaskKind::ColorRange {
                ref samples,
                amount,
            } => {
                for (i, v) in plane.iter_mut().enumerate() {
                    let lab = to_lab([
                        input.planes()[0][i],
                        input.planes()[1][i],
                        input.planes()[2][i],
                    ]);
                    let distance = samples
                        .iter()
                        .map(|s| (lab[0] - s[0]).hypot(lab[1] - s[1]).hypot(lab[2] - s[2]))
                        .fold(f32::INFINITY, f32::min);
                    *v = if amount == 0. {
                        if distance <= 1e-6 { 1. } else { 0. }
                    } else {
                        falloff(distance / (amount / 100.), options.color_smoothness / 100.)
                    };
                }
            }
            _ => return Err(EngineError::invalid("mask", "unsupported component")),
        }
        for (a, mut b) in out.iter_mut().zip(plane) {
            if component.invert {
                b = 1. - b;
            }
            *a = if index == 0 {
                b
            } else {
                match component.combine {
                    MaskCombine::Add => f32::max(*a, b),
                    MaskCombine::Subtract => *a * (1. - b),
                    MaskCombine::Intersect => *a * b,
                }
            };
        }
    }
    if let Some(refinement) = options.refinement {
        out = guided(input, &out, refinement);
    }
    if group.invert {
        for v in &mut out {
            *v = 1. - *v;
        }
    }
    Ok(out)
}
// Band is fully selected inside the inclusive range, with exterior shoulders
// of width smoothness/200. HDR luminance is not clipped into the range.
fn band(v: f32, range: [f32; 2], shoulder: f32) -> f32 {
    let d = (range[0] - v).max(v - range[1]).max(0.);
    if d == 0. {
        1.
    } else if shoulder == 0. {
        0.
    } else {
        smooth(1. - d / shoulder)
    }
}
fn luminance(input: &Image, i: usize) -> f32 {
    0.2627 * input.planes()[0][i] + 0.6780 * input.planes()[1][i] + 0.0593 * input.planes()[2][i]
}
use crate::color_detail::to_lab;
// Truncated square windows at image boundaries. Prefix sums keep runtime O(N)
// independent of radius; f64 moments avoid HDR variance overflow/cancellation.
fn box_mean(v: &[f64], w: usize, h: usize, r: usize) -> Vec<f64> {
    let stride = w + 1;
    let mut sums = vec![0.; stride * (h + 1)];
    for y in 0..h {
        let mut row = 0.;
        for x in 0..w {
            row += v[y * w + x];
            sums[(y + 1) * stride + x + 1] = sums[y * stride + x + 1] + row;
        }
    }
    let mut out = vec![0.; w * h];
    for y in 0..h {
        for x in 0..w {
            let x0 = x.saturating_sub(r);
            let y0 = y.saturating_sub(r);
            let x1 = x.saturating_add(r).saturating_add(1).min(w);
            let y1 = y.saturating_add(r).saturating_add(1).min(h);
            out[y * w + x] =
                (sums[y1 * stride + x1] - sums[y0 * stride + x1] - sums[y1 * stride + x0]
                    + sums[y0 * stride + x0])
                    / ((x1 - x0) * (y1 - y0)) as f64;
        }
    }
    out
}
fn guided(input: &Image, p: &[f32], settings: GuidedRefinement) -> Vec<f32> {
    if settings.radius == 0 {
        return p.to_vec();
    }
    let w = input.width() as usize;
    let h = input.height() as usize;
    let r = settings.radius as usize;
    let guide: Vec<f64> = (0..p.len()).map(|i| luminance(input, i) as f64).collect();
    let p: Vec<f64> = p.iter().map(|&v| v as f64).collect();
    let mean_i = box_mean(&guide, w, h, r);
    let mean_p = box_mean(&p, w, h, r);
    let ii: Vec<_> = guide.iter().map(|v| v * v).collect();
    let ip: Vec<_> = guide.iter().zip(&p).map(|(i, p)| i * p).collect();
    let mean_ii = box_mean(&ii, w, h, r);
    let mean_ip = box_mean(&ip, w, h, r);
    let a: Vec<_> = (0..p.len())
        .map(|i| {
            (mean_ip[i] - mean_i[i] * mean_p[i])
                / ((mean_ii[i] - mean_i[i] * mean_i[i]).max(0.) + settings.epsilon as f64)
        })
        .collect();
    let b: Vec<_> = (0..p.len()).map(|i| mean_p[i] - a[i] * mean_i[i]).collect();
    let a = box_mean(&a, w, h, r);
    let b = box_mean(&b, w, h, r);
    (0..p.len())
        .map(|i| (a[i] * guide[i] + b[i]).clamp(0., 1.) as f32)
        .collect()
}
fn invalid(reason: &str) -> EngineError {
    EngineError::invalid("mask", reason)
}
fn bounded(v: f32, low: f32, high: f32) -> bool {
    v.is_finite() && (low..=high).contains(&v)
}
fn validate(input: &Image, group: &LocalAdjustment, options: MaskOptions<'_>) -> EngineResult<()> {
    if input.planes().iter().flatten().any(|v| !v.is_finite()) {
        return Err(invalid("RGB samples must be finite"));
    }
    if input.planes().len() != 3 {
        return Err(invalid("RGB image required"));
    }
    if !bounded(options.color_smoothness, 0., 100.) {
        return Err(invalid("color smoothness must be 0..=100"));
    }
    let n = input.width() as usize * input.height() as usize;
    if let Some(depth) = options.depth
        && (depth.len() != n || depth.iter().any(|&v| !bounded(v, 0., 1.)))
    {
        return Err(invalid("depth must be a finite same-level plane in 0..=1"));
    }
    if let Some(r) = options.refinement
        && (!r.epsilon.is_finite() || r.epsilon <= 0.)
    {
        return Err(invalid("guided epsilon must be finite and positive"));
    }
    let coords = |p: &[f32]| p.iter().all(|&v| bounded(v, -16., 16.));
    let range = |r: &[f32; 2]| bounded(r[0], 0., 1.) && bounded(r[1], r[0], 1.);
    let mut stamps = 0usize;
    for component in &group.components {
        let valid = match &component.kind {
            MaskKind::Linear { start, end } => {
                coords(start) && coords(end) && (end[0] - start[0]).hypot(end[1] - start[1]) >= 1e-6
            }
            MaskKind::Radial {
                center,
                radii,
                angle,
                feather,
            } => {
                coords(center)
                    && radii.iter().all(|&v| bounded(v, 1e-6, 16.))
                    && angle.is_finite()
                    && bounded(*feather, 0., 100.)
            }
            MaskKind::LuminanceRange {
                range: r,
                smoothness,
            } => range(r) && bounded(*smoothness, 0., 100.),
            MaskKind::Depth {
                range: r, feather, ..
            } => range(r) && bounded(*feather, 0., 100.) && options.depth.is_some(),
            MaskKind::ColorRange { samples, amount } => {
                bounded(*amount, 0., 100.) && samples.iter().flatten().all(|v| v.is_finite())
            }
            MaskKind::Brush { strokes } => {
                for s in strokes {
                    if !bounded(s.radius, 1e-6, 16.)
                        || !bounded(s.flow, 0., 100.)
                        || !bounded(s.feather, 0., 100.)
                        || s.points
                            .iter()
                            .any(|p| !coords(&p[..2]) || !bounded(p[2], 0., 1.))
                    {
                        return Err(invalid("invalid brush geometry, pressure, flow or feather"));
                    }
                    stamps = stamps.saturating_add(usize::from(!s.points.is_empty()));
                    for pair in s.points.windows(2) {
                        let distance = ((pair[1][0] - pair[0][0]) * input.width() as f32)
                            .hypot((pair[1][1] - pair[0][1]) * input.height() as f32);
                        let steps = (distance / (s.radius * input.width() as f32 * 0.25).max(0.25))
                            .ceil()
                            .max(1.) as usize;
                        stamps = stamps.saturating_add(steps);
                        if stamps > 1_000_000 {
                            return Err(invalid("brush interpolation exceeds one million stamps"));
                        }
                    }
                    if stamps > 1_000_000 {
                        return Err(invalid("brush interpolation exceeds one million stamps"));
                    }
                }
                true
            }
            _ => {
                return Err(invalid(
                    "AI segmentation requires an external raster and is unsupported",
                ));
            }
        };
        if !valid {
            return Err(invalid("invalid mask parameters or missing depth"));
        }
    }
    Ok(())
}
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    t * t * (3. - 2. * t)
}
fn falloff(d: f32, feather: f32) -> f32 {
    if feather == 0. {
        if d <= 1. { 1. } else { 0. }
    } else {
        smooth((1. - d) / feather)
    }
}
