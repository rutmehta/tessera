use crate::Image;
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{GeometrySettings, UprightMode},
};
use lens::{Guide, GuideAxis, Homography};

// Newton inversion of the complete green lookup, including non-Brown native models.
pub(crate) fn undistort(
    p: [f64; 2],
    map: &impl Fn([f64; 2], usize) -> Option<[f64; 2]>,
) -> Option<[f64; 2]> {
    let mut q = p;
    for _ in 0..30 {
        let a = map(q, 1)?;
        let e = [a[0] - p[0], a[1] - p[1]];
        if e[0].hypot(e[1]) < 1e-9 {
            return Some(q);
        }
        let h = 1e-5;
        let x = map([q[0] + h, q[1]], 1)?;
        let y = map([q[0], q[1] + h], 1)?;
        let j = [
            (x[0] - a[0]) / h,
            (y[0] - a[0]) / h,
            (x[1] - a[1]) / h,
            (y[1] - a[1]) / h,
        ];
        let det = j[0] * j[3] - j[1] * j[2];
        if det.abs() < 1e-12 {
            return None;
        }
        q[0] -= (j[3] * e[0] - j[1] * e[1]) / det;
        q[1] -= (-j[2] * e[0] + j[0] * e[1]) / det;
    }
    None
}
pub(crate) fn inverse(
    image: &Image,
    s: &GeometrySettings,
    map: &impl Fn([f64; 2], usize) -> Option<[f64; 2]>,
) -> EngineResult<Homography> {
    if s.upright.mode == UprightMode::Off {
        if !s.upright.guides.is_empty() {
            return Err(EngineError::invalid(
                "upright",
                "guides require Guided mode",
            ));
        }
        return Ok(Homography::IDENTITY);
    }
    let result = if s.upright.mode == UprightMode::Guided {
        if !(2..=4).contains(&s.upright.guides.len()) {
            return Err(EngineError::invalid(
                "upright",
                "two to four guides required",
            ));
        }
        let guides = s
            .upright
            .guides
            .iter()
            .map(|g| {
                if g.start
                    .iter()
                    .chain(g.end.iter())
                    .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
                {
                    return None;
                }
                let start = undistort(g.start.map(|v| 2. * v as f64 - 1.), map)?;
                let end = undistort(g.end.map(|v| 2. * v as f64 - 1.), map)?;
                // Engine schema has no explicit guide axis; infer dominant pixel direction.
                let axis = if (end[0] - start[0]).abs() * image.width() as f64
                    >= (end[1] - start[1]).abs() * image.height() as f64
                {
                    GuideAxis::Horizontal
                } else {
                    GuideAxis::Vertical
                };
                Some(Guide { start, end, axis })
            })
            .collect::<Option<Vec<_>>>()
            .ok_or_else(|| EngineError::invalid("upright", "invalid or noninvertible guide"))?;
        Some(
            lens::guided_upright(&guides)
                .ok_or_else(|| EngineError::invalid("upright", "degenerate guide configuration"))?,
        )
    } else {
        if !s.upright.guides.is_empty() {
            return Err(EngineError::invalid(
                "upright",
                "guides require Guided mode",
            ));
        }
        let (gray, _) = crate::lens_resolve::analysis_images(image)?;
        let lines = lens::detect_lines(&gray, 0.02, 12)
            .into_iter()
            .filter_map(|mut l| {
                l.start = undistort(l.start, map)?;
                l.end = undistort(l.end, map)?;
                l.points = l
                    .points
                    .into_iter()
                    .map(|p| undistort(p, map))
                    .collect::<Option<_>>()?;
                Some(l)
            })
            .collect::<Vec<_>>();
        let mode = match s.upright.mode {
            UprightMode::Level => lens::UprightMode::Level,
            UprightMode::Vertical => lens::UprightMode::Vertical,
            UprightMode::Full => lens::UprightMode::Full,
            _ => lens::UprightMode::Auto,
        };
        lens::estimate_upright(&lines, mode)
    };
    Ok(result
        .and_then(|r| r.homography.inverse())
        .unwrap_or(Homography::IDENTITY))
}
