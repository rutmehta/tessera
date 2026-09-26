//! Boolean ops, transform and Modify (grow/contract/border/smooth/feather).

use compositor::Affine;
use engine_api::{EngineError, EngineResult};

use crate::filter::{gaussian, signed_distance};
use crate::mask::Mask;

/// How a new selection combines with the current one (per pixel).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Combine {
    /// `b`.
    Replace,
    /// `max(a, b)`.
    Add,
    /// `min(a, 1 − b)`.
    Subtract,
    /// `min(a, b)`.
    Intersect,
    /// `|a − b|` (symmetric difference).
    Xor,
}

/// Combines `a` (current) with `b` (new).
pub fn combine(a: &Mask, b: &Mask, op: Combine) -> EngineResult<Mask> {
    if a.extent() != b.extent() {
        return Err(EngineError::invalid("selection", "extent mismatch"));
    }
    let data = a
        .data()
        .iter()
        .zip(b.data())
        .map(|(&x, &y)| match op {
            Combine::Replace => y,
            Combine::Add => x.max(y),
            Combine::Subtract => x.min(1.0 - y),
            Combine::Intersect => x.min(y),
            Combine::Xor => (x - y).abs(),
        })
        .collect();
    Mask::from_vec(a.width(), a.height(), data)
}

/// `1 − m`.
pub fn invert(m: &Mask) -> Mask {
    let mut o = m.clone();
    o.data_mut().iter_mut().for_each(|v| *v = 1.0 - *v);
    o
}

/// Gaussian feather; `radius` is 2σ.
pub fn feather(m: &Mask, radius: f32) -> Mask {
    if radius <= 0.0 {
        return m.clone();
    }
    let d = gaussian(
        m.data(),
        m.width() as usize,
        m.height() as usize,
        radius * 0.5,
    );
    Mask::from_vec(m.width(), m.height(), d).unwrap_or_else(|_| m.clone())
}

fn from_sdf(m: &Mask, f: impl Fn(f32) -> f32) -> Mask {
    let sdf = signed_distance(m.data(), m.width() as usize, m.height() as usize);
    let d = sdf
        .into_iter()
        .map(|s| (0.5 - f(s)).clamp(0.0, 1.0))
        .collect();
    Mask::from_vec(m.width(), m.height(), d).unwrap_or_else(|_| m.clone())
}

/// Expands the selection edge outwards by `px` (Euclidean).
pub fn grow(m: &Mask, px: f32) -> Mask {
    from_sdf(m, |s| s - px)
}

/// Contracts the selection edge inwards by `px`.
pub fn contract(m: &Mask, px: f32) -> Mask {
    from_sdf(m, |s| s + px)
}

/// A band of `width` pixels centred on the selection edge.
pub fn border(m: &Mask, width: f32) -> Mask {
    from_sdf(m, |s| s.abs() - width * 0.5)
}

/// Smooths the outline: Gaussian of the signed distance (σ = `radius`/2),
/// re-thresholded, which rounds corners and removes jaggies while keeping a
/// crisp anti-aliased edge.
pub fn smooth(m: &Mask, radius: f32) -> Mask {
    let (w, h) = (m.width() as usize, m.height() as usize);
    let sdf = signed_distance(m.data(), w, h);
    let s = gaussian(&sdf, w, h, radius * 0.5);
    let d = s.into_iter().map(|v| (0.5 - v).clamp(0.0, 1.0)).collect();
    Mask::from_vec(m.width(), m.height(), d).unwrap_or_else(|_| m.clone())
}

/// Transform Selection: maps the selection by `t` (source → destination
/// pixel coordinates) into a `width × height` mask, bilinear.
pub fn transform(m: &Mask, t: &Affine, width: u32, height: u32) -> EngineResult<Mask> {
    let inv = t
        .inverse()
        .ok_or_else(|| EngineError::invalid("transform", "singular"))?;
    Ok(Mask::from_fn(width, height, |x, y| {
        let (sx, sy) = inv.apply(f64::from(x) + 0.5, f64::from(y) + 0.5);
        let (fx, fy) = (sx as f32 - 0.5, sy as f32 - 0.5);
        let (x0, y0) = (fx.floor(), fy.floor());
        let (ax, ay) = (fx - x0, fy - y0);
        let (x0, y0) = (x0 as i64, y0 as i64);
        let a = m.get(x0, y0) + (m.get(x0 + 1, y0) - m.get(x0, y0)) * ax;
        let b = m.get(x0, y0 + 1) + (m.get(x0 + 1, y0 + 1) - m.get(x0, y0 + 1)) * ax;
        (a + (b - a) * ay).clamp(0.0, 1.0)
    }))
}
