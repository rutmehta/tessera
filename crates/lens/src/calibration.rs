use crate::{BrownConrady, Point};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Estimate<T> {
    pub value: T,
    pub residual: f64,
    pub confidence: f64,
}
/// Fit k1 by total-least-squares straightness of observed polylines.
/// A polyline must follow one physically straight scene edge (not a chord).
pub fn estimate_k1(lines: &[Vec<Point>], bounds: [f64; 2]) -> Option<Estimate<f64>> {
    if !bounds.iter().all(|v| v.is_finite()) || bounds[0] >= bounds[1] {
        return None;
    }
    let lines: Vec<_> = lines
        .iter()
        .filter(|l| l.len() >= 5 && l.iter().flatten().all(|v| v.is_finite()))
        .collect();
    if lines.len() < 2 {
        return None;
    }
    let cost = |k| {
        let m = BrownConrady {
            k1: k,
            ..Default::default()
        };
        let mut score = 0.;
        let mut count = 0;
        for line in &lines {
            let Some(q) = line
                .iter()
                .map(|p| m.undistort(*p))
                .collect::<Option<Vec<_>>>()
            else {
                return f64::INFINITY;
            };
            let (_, _, error, span) = fit_line(&q);
            if span > 0.01 {
                score += error;
                count += 1;
            }
        }
        if count < 2 {
            f64::INFINITY
        } else {
            score / count as f64
        }
    };
    let (value, residual) = minimize(cost, bounds);
    if !residual.is_finite() {
        return None;
    }
    let baseline = cost(0.);
    let contrast = (cost(bounds[0]) - residual).max(cost(bounds[1]) - residual);
    if contrast < 1e-12 {
        return None;
    }
    Some(Estimate {
        value,
        residual,
        confidence: if baseline > 1e-12 {
            (1. - residual / baseline).clamp(0., 1.)
        } else {
            1.
        },
    })
}
pub(crate) fn minimize(f: impl Fn(f64) -> f64, bounds: [f64; 2]) -> (f64, f64) {
    let step = (bounds[1] - bounds[0]) / 80.;
    let mut best = bounds[0];
    let mut score = f(best);
    for i in 1..=80 {
        let x = bounds[0] + step * i as f64;
        let s = f(x);
        if s < score {
            score = s;
            best = x
        }
    }
    let mut lo = (best - step).max(bounds[0]);
    let mut hi = (best + step).min(bounds[1]);
    for _ in 0..35 {
        let a = lo + (hi - lo) / 3.;
        let b = hi - (hi - lo) / 3.;
        if f(a) < f(b) {
            hi = b
        } else {
            lo = a
        }
    }
    let x = (lo + hi) / 2.;
    (x, f(x))
}
pub(crate) fn fit_line(q: &[Point]) -> (Point, Point, f64, f64) {
    let n = q.len() as f64;
    let c = [
        q.iter().map(|p| p[0]).sum::<f64>() / n,
        q.iter().map(|p| p[1]).sum::<f64>() / n,
    ];
    let (mut xx, mut yy, mut xy) = (0., 0., 0.);
    for p in q {
        let x = p[0] - c[0];
        let y = p[1] - c[1];
        xx += x * x;
        yy += y * y;
        xy += x * y
    }
    let theta = 0.5 * (2. * xy).atan2(xx - yy);
    let d = [theta.cos(), theta.sin()];
    let e = q
        .iter()
        .map(|p| ((p[0] - c[0]) * d[1] - (p[1] - c[1]) * d[0]).powi(2))
        .sum::<f64>()
        / n;
    (c, d, e, (xx + yy) / n)
}
