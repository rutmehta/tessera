use crate::*;
pub use lyon::path::{LineCap, LineJoin};
use lyon::{
    math::point,
    path::Path as LyonPath,
    tessellation::{BuffersBuilder, StrokeOptions, StrokeTessellator, StrokeVertex, VertexBuffers},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Alignment {
    Inside,
    Center,
    Outside,
}
#[derive(Clone, Debug)]
pub struct Stroke {
    pub width: f64,
    pub alignment: Alignment,
    pub dashes: Vec<f64>,
    pub dash_offset: f64,
    pub cap: LineCap,
    pub join: LineJoin,
    pub miter_limit: f64,
}
impl Default for Stroke {
    fn default() -> Self {
        Self {
            width: 1.,
            alignment: Alignment::Center,
            dashes: vec![],
            dash_offset: 0.,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.,
        }
    }
}
impl Stroke {
    fn validate(&self) -> Result<()> {
        if !self.width.is_finite()
            || self.width < 0.
            || self.width > f64::from(f32::MAX) / 2.
            || !self.miter_limit.is_finite()
            || self.miter_limit < 1.
            || self.miter_limit > f64::from(f32::MAX)
            || !self.dash_offset.is_finite()
            || self.dashes.iter().any(|v| !v.is_finite() || *v <= 0.)
        {
            return Err(Error::Invalid("stroke"));
        }
        Ok(())
    }
    /// Dash distances are document-space arc lengths of the flattened path.
    pub fn dashed(&self, path: &Path, tolerance: f64) -> Result<Path> {
        self.validate()?;
        if self.dashes.is_empty() {
            path.validate()?;
            return Ok(path.clone());
        }
        let mut pattern = self.dashes.clone();
        if pattern.len() % 2 == 1 {
            pattern.extend(self.dashes.iter().copied());
        }
        let total: f64 = pattern.iter().sum();
        if !total.is_finite() {
            return Err(Error::Invalid("dash total"));
        }
        let mut result = Path::default();
        for (mut points, closed) in path.flattened(tolerance)? {
            if points.len() < 2 {
                continue;
            }
            if closed {
                points.push(points[0]);
            }
            let base = result.subpaths.len();
            let mut offset = self.dash_offset.rem_euclid(total);
            let mut index = 0;
            while offset >= pattern[index] {
                offset -= pattern[index];
                index = (index + 1) % pattern.len();
            }
            let mut left = pattern[index] - offset;
            let mut run = vec![];
            for pair in points.windows(2) {
                let length = pair[0].distance(pair[1]);
                if length == 0. {
                    continue;
                }
                let mut pos = 0.;
                while pos < length {
                    let step = left.min(length - pos);
                    if step <= 0. || pos + step == pos {
                        return Err(Error::Invalid("dash precision"));
                    }
                    if index % 2 == 0 {
                        if run.is_empty() {
                            run.push(pair[0].lerp(pair[1], pos / length));
                        }
                        run.push(pair[0].lerp(pair[1], (pos + step) / length));
                    }
                    pos += step;
                    left -= step;
                    if left <= f64::EPSILON * total {
                        if !run.is_empty() {
                            result.subpaths.extend(Path::polyline(&run, false).subpaths);
                            run.clear();
                        }
                        index = (index + 1) % pattern.len();
                        left = pattern[index];
                    }
                }
            }
            if !run.is_empty() {
                result.subpaths.extend(Path::polyline(&run, false).subpaths);
            }
            // Join a dash crossing the closure rather than cap it twice.
            if closed && result.subpaths.len() > base {
                let last = result.subpaths.len() - 1;
                let first_at_seam = result.subpaths[base].anchors[0].point == points[0];
                let last_at_seam = result.subpaths[last].anchors.last().unwrap().point == points[0];
                if first_at_seam && last_at_seam {
                    if base == last {
                        let s = &mut result.subpaths[base];
                        s.anchors.pop();
                        s.closed = true;
                    } else {
                        let first = result.subpaths.remove(base);
                        let last = result.subpaths.last_mut().unwrap();
                        last.anchors.extend(first.anchors.into_iter().skip(1));
                    }
                }
            }
        }
        Ok(result)
    }
    pub fn outline(&self, path: &Path, tolerance: f64) -> Result<Path> {
        self.validate()?;
        if !tolerance.is_finite() || tolerance <= 0. || tolerance > f64::from(f32::MAX) {
            return Err(Error::Invalid("stroke tolerance"));
        }
        if self.alignment != Alignment::Center && path.subpaths.iter().any(|s| !s.closed) {
            return Err(Error::Invalid("aligned strokes require closed paths"));
        }
        let dashed = self.dashed(path, tolerance)?;
        if self.width == 0. {
            return Ok(Path::default());
        }
        let mut builder = LyonPath::builder();
        let convert = |p: Point| point(p.x as f32, p.y as f32);
        for s in &dashed.subpaths {
            if let Some(first) = s.anchors.first() {
                if s.anchors.iter().any(|a| {
                    [a.point, a.incoming, a.outgoing]
                        .iter()
                        .any(|p| p.x.abs() > 1e15 || p.y.abs() > 1e15)
                }) {
                    return Err(Error::Invalid("lyon coordinate range"));
                }
                builder.begin(convert(first.point));
                for pair in s.anchors.windows(2) {
                    builder.cubic_bezier_to(
                        convert(pair[0].outgoing),
                        convert(pair[1].incoming),
                        convert(pair[1].point),
                    );
                }
                if s.closed {
                    let last = s.anchors.last().unwrap();
                    builder.cubic_bezier_to(
                        convert(last.outgoing),
                        convert(first.incoming),
                        convert(first.point),
                    );
                }
                builder.end(s.closed);
            }
        }
        let options = StrokeOptions::default()
            .with_line_width(
                (self.width
                    * if self.alignment == Alignment::Center {
                        1.
                    } else {
                        2.
                    }) as f32,
            )
            .with_line_cap(self.cap)
            .with_line_join(self.join)
            .with_miter_limit(self.miter_limit as f32)
            .with_tolerance(tolerance as f32);
        let mut mesh: VertexBuffers<Point, u32> = VertexBuffers::new();
        StrokeTessellator::new()
            .tessellate_path(
                &builder.build(),
                &options,
                &mut BuffersBuilder::new(&mut mesh, |v: StrokeVertex<'_, '_>| {
                    let p = v.position();
                    Point::new(f64::from(p.x), f64::from(p.y))
                }),
            )
            .map_err(|_| Error::Invalid("stroke tessellation"))?;
        let mut triangles = Path::default();
        for indices in mesh.indices.as_chunks::<3>().0 {
            let mut p = [
                mesh.vertices[indices[0] as usize],
                mesh.vertices[indices[1] as usize],
                mesh.vertices[indices[2] as usize],
            ];
            if (p[1] - p[0]).cross(p[2] - p[0]) < 0. {
                p.swap(1, 2);
            }
            triangles.subpaths.extend(Path::polyline(&p, true).subpaths);
        }
        let empty = Path::default();
        triangles.boolean(
            if self.alignment == Alignment::Center {
                &empty
            } else {
                path
            },
            match self.alignment {
                Alignment::Center => Operation::Combine,
                Alignment::Inside => Operation::Intersect,
                Alignment::Outside => Operation::Subtract,
            },
            tolerance,
        )
    }
}
