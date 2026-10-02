//! Cancellation-free tone evaluation, mirrored operation-for-operation in
//! pipeline-gpu/src/operators.wgsl. Keep constants, branches and order paired.
//! These are f32 evaluations of the existing curve, not a luminance floor.

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

fn softplus(v: f32) -> f32 {
    v.max(0.0) + log_one_plus((-v.abs()).exp())
}

pub(super) fn upper_integral(z: f32, center: f32) -> f32 {
    if z < 0.5 {
        // S(z-k)-S(-k) = log1p(expm1(z)/(1+exp(k))).
        // Evaluate the small difference directly rather than subtracting two
        // O(1) softplus values and later magnifying their error by 1/Y.
        return log_one_plus(exp_minus_one(z) / (1.0 + center.exp()));
    }
    // z >= .5: subtraction is conditioned, and avoids exp(z) HDR overflow.
    softplus(z - center) - softplus(-center)
}
