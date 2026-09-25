use crate::{LineSegment, Point};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Homography(pub [[f64; 3]; 3]);
impl Homography {
    pub const IDENTITY: Self = Self([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
    pub fn map(self, p: Point) -> Option<Point> {
        let h = self.0;
        let d = h[2][0] * p[0] + h[2][1] * p[1] + h[2][2];
        if !d.is_finite() || d.abs() < 1e-12 {
            return None;
        }
        let q = [
            (h[0][0] * p[0] + h[0][1] * p[1] + h[0][2]) / d,
            (h[1][0] * p[0] + h[1][1] * p[1] + h[1][2]) / d,
        ];
        q.iter().all(|v| v.is_finite()).then_some(q)
    }
    pub fn inverse(self) -> Option<Self> {
        let mut out = [[0.; 3]; 3];
        for j in 0..3 {
            let mut b = [0.; 3];
            b[j] = 1.;
            let x = crate::vignette::solve(self.0, b)?;
            for i in 0..3 {
                out[i][j] = x[i]
            }
        }
        Some(Self(out))
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UprightMode {
    Off,
    Level,
    Vertical,
    Full,
    Auto,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuideAxis {
    Horizontal,
    Vertical,
}
#[derive(Clone, Copy, Debug)]
pub struct Guide {
    pub start: Point,
    pub end: Point,
    pub axis: GuideAxis,
}
#[derive(Clone, Debug)]
pub struct UprightResult {
    pub homography: Homography,
    pub confidence: f64,
    pub inliers: usize,
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn valid(l: &Guide) -> bool {
    l.start.iter().chain(l.end.iter()).all(|v| v.is_finite())
        && (l.start[0] - l.end[0]).hypot(l.start[1] - l.end[1]) > 1e-6
}
fn equation(l: &Guide) -> [f64; 3] {
    cross([l.start[0], l.start[1], 1.], [l.end[0], l.end[1], 1.])
}
fn angular(l: &Guide, v: [f64; 3]) -> f64 {
    let d = [l.end[0] - l.start[0], l.end[1] - l.start[1]];
    let c = [(l.start[0] + l.end[0]) * 0.5, (l.start[1] + l.end[1]) * 0.5];
    let q = [v[0] - c[0] * v[2], v[1] - c[1] * v[2]];
    let norm = d[0].hypot(d[1]) * q[0].hypot(q[1]);
    if norm < 1e-12 {
        return f64::INFINITY;
    }
    (d[0] * q[1] - d[1] * q[0]).abs() / norm
}
/// Deterministic pair-consensus RANSAC, including vanishing points at infinity.
fn vanishing(lines: &[Guide]) -> Option<([f64; 3], usize)> {
    if lines.len() < 2 {
        return None;
    }
    let mut best = None;
    let mut best_count = 0;
    let mut best_error = f64::INFINITY;
    let n = lines.len().min(128);
    for i in 0..n {
        for j in i + 1..n {
            let mut v = cross(equation(&lines[i]), equation(&lines[j]));
            let norm = v.iter().map(|x| x * x).sum::<f64>().sqrt();
            if norm < 1e-10 {
                continue;
            }
            for x in &mut v {
                *x /= norm
            }
            let errors: Vec<_> = lines
                .iter()
                .map(|l| angular(l, v))
                .filter(|e| *e < 0.025)
                .collect();
            let error = errors.iter().sum::<f64>();
            if errors.len() > best_count || errors.len() == best_count && error < best_error {
                best_count = errors.len();
                best_error = error;
                best = Some(v)
            }
        }
    }
    best.filter(|_| best_count >= 2).map(|v| (v, best_count))
}
fn rectify(h: Option<[f64; 3]>, v: Option<[f64; 3]>) -> Option<Homography> {
    let matrix = match (h, v) {
        (Some(h), Some(v)) => {
            if h[0].abs() < 1e-8 || v[1].abs() < 1e-8 {
                return None;
            }
            Homography([
                [1., v[0] / v[1], 0.],
                [h[1] / h[0], 1., 0.],
                [h[2] / h[0], v[2] / v[1], 1.],
            ])
            .inverse()?
        }
        (None, Some(v)) => {
            if v[1].abs() < 1e-8 {
                return None;
            }
            Homography([[1., -v[0] / v[1], 0.], [0., 1., 0.], [0., -v[2] / v[1], 1.]])
        }
        (Some(h), None) => {
            if h[0].abs() < 1e-8 {
                return None;
            }
            Homography([[1., 0., 0.], [-h[1] / h[0], 1., 0.], [-h[2] / h[0], 0., 1.]])
        }
        _ => return None,
    };
    // Reject solutions whose horizon crosses the source rectangle.
    for p in [[-1., -1.], [-1., 1.], [1., -1.], [1., 1.]] {
        let d = matrix.0[2][0] * p[0] + matrix.0[2][1] * p[1] + matrix.0[2][2];
        if d < 0.1 {
            return None;
        }
    }
    Some(matrix)
}
/// Solve two to four explicitly oriented guide constraints.
pub fn guided_upright(guides: &[Guide]) -> Option<UprightResult> {
    if !(2..=4).contains(&guides.len()) || !guides.iter().all(valid) {
        return None;
    }
    solve_guides(guides)
}
fn solve_guides(guides: &[Guide]) -> Option<UprightResult> {
    let h: Vec<_> = guides
        .iter()
        .copied()
        .filter(|l| l.axis == GuideAxis::Horizontal)
        .collect();
    let v: Vec<_> = guides
        .iter()
        .copied()
        .filter(|l| l.axis == GuideAxis::Vertical)
        .collect();
    let hp = vanishing(&h);
    let vp = vanishing(&v);
    let inliers = hp.map(|p| p.1).unwrap_or(0) + vp.map(|p| p.1).unwrap_or(0);
    let homography = rectify(hp.map(|p| p.0), vp.map(|p| p.0))?;
    Some(UprightResult {
        homography,
        confidence: inliers as f64 / guides.len() as f64,
        inliers,
    })
}
pub fn estimate_upright(lines: &[LineSegment], mode: UprightMode) -> Option<UprightResult> {
    if mode == UprightMode::Off {
        return Some(UprightResult {
            homography: Homography::IDENTITY,
            confidence: 1.,
            inliers: 0,
        });
    }
    let guides: Vec<_> = lines
        .iter()
        .map(|l| Guide {
            start: l.start,
            end: l.end,
            axis: if (l.end[0] - l.start[0]).abs() > (l.end[1] - l.start[1]).abs() {
                GuideAxis::Horizontal
            } else {
                GuideAxis::Vertical
            },
        })
        .filter(valid)
        .collect();
    if guides.len() < 2 {
        return None;
    }
    if mode == UprightMode::Level {
        let mut angles: Vec<_> = guides
            .iter()
            .map(|g| {
                let mut a = (g.end[1] - g.start[1]).atan2(g.end[0] - g.start[0]);
                while a > std::f64::consts::FRAC_PI_4 {
                    a -= std::f64::consts::FRAC_PI_2
                }
                while a < -std::f64::consts::FRAC_PI_4 {
                    a += std::f64::consts::FRAC_PI_2
                }
                a
            })
            .collect();
        angles.sort_by(f64::total_cmp);
        let a = angles[angles.len() / 2];
        let (s, c) = a.sin_cos();
        return Some(UprightResult {
            homography: Homography([[c, s, 0.], [-s, c, 0.], [0., 0., 1.]]),
            confidence: angles.iter().filter(|x| (**x - a).abs() < 0.025).count() as f64
                / angles.len() as f64,
            inliers: angles.iter().filter(|x| (**x - a).abs() < 0.025).count(),
        });
    }
    let selected: Vec<_> = guides
        .iter()
        .copied()
        .filter(|g| mode != UprightMode::Vertical || g.axis == GuideAxis::Vertical)
        .collect();
    if mode == UprightMode::Full
        && (selected
            .iter()
            .filter(|g| g.axis == GuideAxis::Horizontal)
            .count()
            < 2
            || selected
                .iter()
                .filter(|g| g.axis == GuideAxis::Vertical)
                .count()
                < 2)
    {
        return None;
    }
    if mode == UprightMode::Full {
        let horizontal: Vec<_> = selected
            .iter()
            .copied()
            .filter(|g| g.axis == GuideAxis::Horizontal)
            .collect();
        let vertical: Vec<_> = selected
            .iter()
            .copied()
            .filter(|g| g.axis == GuideAxis::Vertical)
            .collect();
        vanishing(&horizontal)?;
        vanishing(&vertical)?;
    }
    solve_guides(&selected)
}
