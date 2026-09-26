//! Per-pixel compositing maths shared by the CPU executor (and mirrored in
//! `composite.wgsl`). See COMPOSITOR.md §2–3 for the derivations.

use crate::blend::{BlendIf, BlendMode, blend_pixel, dissolve_threshold};
use crate::document::{Knockout, Layer};

/// Blend parameters of one op.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Params {
    pub mode: BlendMode,
    pub opacity: f32,
    pub fill: f32,
    pub blend_if: Option<BlendIf>,
    pub knockout: Knockout,
    /// Source-atop (clipped layers): the backdrop's alpha is kept.
    pub atop: bool,
    pub seed: u32,
}

impl Params {
    pub fn of(layer: &Layer, atop: bool) -> Self {
        let p = &layer.props;
        Self {
            mode: p.blend_mode,
            opacity: p.opacity.clamp(0.0, 1.0),
            fill: p.fill_opacity.clamp(0.0, 1.0),
            blend_if: (!p.blend_if.is_identity()).then_some(p.blend_if),
            knockout: if atop { Knockout::None } else { p.knockout },
            atop,
            seed: seed_of(layer),
        }
    }

    /// Normal, full opacity, the given fill (clip-group bases).
    pub fn clip_base(layer: &Layer) -> Self {
        Self {
            mode: BlendMode::Normal,
            opacity: 1.0,
            fill: layer.props.fill_opacity.clamp(0.0, 1.0),
            blend_if: None,
            knockout: Knockout::None,
            atop: false,
            seed: seed_of(layer),
        }
    }

    /// The group-level params of a clip group (base's mode/opacity/Blend
    /// If/knockout, fill already applied inside).
    pub fn clip_pop(layer: &Layer) -> Self {
        Self {
            fill: 1.0,
            ..Self::of(layer, false)
        }
    }
}

fn seed_of(layer: &Layer) -> u32 {
    (layer.id.0 as u32) ^ ((layer.id.0 >> 32) as u32) ^ 0x9e37_79b9
}

/// Knockout backdrop for a pixel.
#[derive(Clone, Copy)]
pub(crate) enum Kb {
    Off,
    Transparent,
    Px([f32; 4]),
}

#[inline(always)]
pub(crate) fn unpremul(p: [f32; 4]) -> [f32; 3] {
    if p[3] > 0.0 {
        let k = 1.0 / p[3];
        [p[0] * k, p[1] * k, p[2] * k]
    } else {
        [0.0; 3]
    }
}

/// Composites straight source `s` (alpha = shape σ: content × mask) onto
/// premultiplied backdrop `b`.
#[inline(always)]
pub(crate) fn blend_px<F: Fn([f32; 3], [f32; 3]) -> [f32; 3]>(
    b: [f32; 4],
    s: [f32; 4],
    p: &Params,
    kb: Kb,
    x: u32,
    y: u32,
    f: &F,
) -> [f32; 4] {
    let ab = b[3];
    let cb = unpremul(b);
    let cs = [s[0], s[1], s[2]];
    let mut sigma = s[3];
    if let Some(bi) = &p.blend_if {
        sigma *= bi.weight(cs, cb);
    }
    if sigma <= 0.0 {
        return b;
    }
    if p.mode == BlendMode::Dissolve {
        if dissolve_threshold(x, y, p.seed) >= sigma * p.opacity * p.fill {
            return b;
        }
        return if p.atop {
            [ab * cs[0], ab * cs[1], ab * cs[2], ab]
        } else {
            [cs[0], cs[1], cs[2], 1.0]
        };
    }
    match kb {
        Kb::Off => {
            let a = sigma * p.opacity * p.fill;
            let bl = f(cb, cs);
            if p.atop {
                let k = a * ab;
                [
                    b[0] + k * (bl[0] - cb[0]),
                    b[1] + k * (bl[1] - cb[1]),
                    b[2] + k * (bl[2] - cb[2]),
                    ab,
                ]
            } else {
                let (u, v, w) = (a * (1.0 - ab), a * ab, 1.0 - a);
                [
                    u * cs[0] + v * bl[0] + w * b[0],
                    u * cs[1] + v * bl[1] + w * b[1],
                    u * cs[2] + v * bl[2] + w * b[2],
                    a + w * ab,
                ]
            }
        }
        Kb::Transparent | Kb::Px(_) => {
            let k = match kb {
                Kb::Px(k) => k,
                _ => [0.0; 4],
            };
            let fo = p.fill;
            let ka = k[3];
            let ck = unpremul(k);
            let bl = f(ck, cs);
            let (u, v, w) = (fo * (1.0 - ka), fo * ka, 1.0 - fo);
            let r = [
                u * cs[0] + v * bl[0] + w * k[0],
                u * cs[1] + v * bl[1] + w * k[1],
                u * cs[2] + v * bl[2] + w * k[2],
                fo + w * ka,
            ];
            let t = sigma * p.opacity;
            [
                b[0] + t * (r[0] - b[0]),
                b[1] + t * (r[1] - b[1]),
                b[2] + t * (r[2] - b[2]),
                b[3] + t * (r[3] - b[3]),
            ]
        }
    }
}

/// Applies an adjustment result `a` (straight) to premultiplied backdrop
/// `b` with weight `w` (mask × opacity × fill), keeping the backdrop alpha.
#[inline(always)]
pub(crate) fn adjust_px(
    b: [f32; 4],
    a: [f32; 3],
    mut w: f32,
    p: &Params,
    x: u32,
    y: u32,
) -> [f32; 4] {
    let ab = b[3];
    if ab <= 0.0 {
        return b;
    }
    let cb = unpremul(b);
    if let Some(bi) = &p.blend_if {
        w *= bi.weight(a, cb);
    }
    if p.mode == BlendMode::Dissolve {
        w = if dissolve_threshold(x, y, p.seed) < w {
            1.0
        } else {
            0.0
        };
    }
    if w <= 0.0 {
        return b;
    }
    let bl = blend_pixel(p.mode, cb, a);
    let k = w * ab;
    [
        b[0] + k * (bl[0] - cb[0]),
        b[1] + k * (bl[1] - cb[1]),
        b[2] + k * (bl[2] - cb[2]),
        ab,
    ]
}
