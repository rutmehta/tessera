//! Cancellation-free tone evaluation, mirrored operation-for-operation in
//! pipeline-gpu/src/operators.wgsl. Keep constants, branches and order paired.
//! These are f32 evaluations of the existing curves, not a luminance floor.

pub(super) fn log_one_plus(x: f32) -> f32 {
    if x.abs() < 0.5 {
        // log(1+x) = 2 atanh(x/(2+x)). No addition to 1 loses small x.
        // At |x| < .5, |t| < 1/3; the omitted tail is below f32 roundoff.
        let t = x / (2.0 + x);
        let t2 = t * t;
        let mut r = 1.0 / 15.0;
        r = 1.0 / 13.0 + t2 * r;
        r = 1.0 / 11.0 + t2 * r;
        r = 1.0 / 9.0 + t2 * r;
        r = 1.0 / 7.0 + t2 * r;
        r = 1.0 / 5.0 + t2 * r;
        r = 1.0 / 3.0 + t2 * r;
        return 2.0 * t * (1.0 + t2 * r);
    }
    (1.0 + x).ln()
}

pub(super) fn exp_minus_one(x: f32) -> f32 {
    if x.abs() < 0.5 {
        let mut r = 1.0 / 3628800.0;
        r = 1.0 / 362880.0 + x * r;
        r = 1.0 / 40320.0 + x * r;
        r = 1.0 / 5040.0 + x * r;
        r = 1.0 / 720.0 + x * r;
        r = 1.0 / 120.0 + x * r;
        r = 1.0 / 24.0 + x * r;
        r = 1.0 / 6.0 + x * r;
        r = 0.5 + x * r;
        return x * (1.0 + x * r);
    }
    x.exp() - 1.0
}

/// Logistic 1/(1+exp(-x)); the argument is clamped so exp never overflows.
pub(super) fn sigmoid(x: f32) -> f32 {
    1.0 / (1.0 + (-x.clamp(-60.0, 60.0)).exp())
}

/// tanh through one exp, so both backends take the same path and rounding.
pub(super) fn tanh_unit(x: f32) -> f32 {
    let t = (-2.0 * x.abs().min(20.0)).exp();
    let r = (1.0 - t) / (1.0 + t);
    if x < 0.0 { -r } else { r }
}

/// One stop in the natural-log luminance axis.
const STOP: f32 = std::f32::consts::LN_2;
/// Half-range of the contrast S-curve: at Contrast ±100 the far shadows and
/// highlights shift by at most ±1.5 stops (×2 slope) or ∓0.75 (×0.5 slope).
const CONTRAST_REACH: f32 = 1.5 * STOP;

/// Per region (Blacks, Shadows, Highlights, Whites): the gain at ±100 in
/// stops, the band centre in stops from 18% grey, the feather width in
/// natural-log units, and whether the band is above its centre.
pub(super) const REGIONS: [(f32, f32, f32, bool); 4] = [
    (1.25, -3.5, 0.55, false),
    (1.5, -2.0, 0.75, false),
    (1.6, 1.25, 0.55, true),
    (1.0, 2.75, 0.6, true),
];

/// Luminance gain of the basic tone controls for `y > 0`. `slope` is the
/// contrast slope at 18% grey (`2^(contrast/100)`); `amounts` are the Blacks,
/// Shadows, Highlights and Whites amounts divided by 100.
///
/// With `u = ln(y/.18)`, contrast is `u1 = u + (slope-1)·K·tanh(u/K)`: slope
/// `slope` at the pivot, slope 1 and a bounded shift far from it. Each region
/// then adds `amount·range·w(u1)` stops, where `w` is a logistic feather in
/// `u1` that saturates to 1 inside the band and 0 outside it. Blacks and
/// Shadows therefore act as a constant gain on near-black values (black stays
/// black) and vanish in the highlights; Highlights and Whites are the mirror.
/// The composition is monotone: every feather derivative is bounded and the
/// combined log-slope stays above .26 at every slider extreme.
pub(super) fn tone_gain(y: f32, slope: f32, amounts: [f32; 4]) -> f32 {
    let u = (y / 0.18).ln();
    let shift = (slope - 1.0) * CONTRAST_REACH * tanh_unit(u / CONTRAST_REACH);
    let u1 = u + shift;
    let mut stops = 0.0;
    for (amount, (range, centre, width, upper)) in amounts.into_iter().zip(REGIONS) {
        let x = (u1 - centre * STOP) / width;
        let w = sigmoid(if upper { x } else { -x });
        stops += amount * range * w;
    }
    (shift + stops * STOP).exp()
}
