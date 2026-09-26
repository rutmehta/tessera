use crate::{
    Error, GlyphOutline, Layout, Result, TextBox, TextModel, TextRenderer, Warp, WarpKind,
};
use lyon_path::{
    Path,
    iterator::PathIterator,
    math::{Point, point},
};

/// Arc-length table for a single lyon contour. Curves are flattened to the
/// requested document-space tolerance. Disjoint contours are rejected.
#[derive(Clone, Debug)]
pub struct TextPath {
    segments: Vec<(Point, Point, f32, f32)>,
    length: f32,
}
impl TextPath {
    pub fn new(path: &Path, tolerance: f32) -> Result<Self> {
        if !tolerance.is_finite() || tolerance <= 0.0 {
            return Err(Error::Invalid("invalid path tolerance"));
        }
        let mut segments = Vec::new();
        let mut length = 0.0;
        let mut begun = false;
        for event in path.iter().flattened(tolerance) {
            use lyon_path::Event::*;
            let pair = match event {
                Begin { at } => {
                    if begun || !at.x.is_finite() || !at.y.is_finite() {
                        return Err(Error::Invalid("path must have one finite contour"));
                    }
                    begun = true;
                    None
                }
                Line { from, to } => Some((from, to)),
                End {
                    last,
                    first,
                    close: true,
                } => Some((last, first)),
                _ => None,
            };
            if let Some((from, to)) = pair {
                let distance = (to - from).length();
                if !distance.is_finite() {
                    return Err(Error::Invalid("non-finite path segment"));
                }
                if distance > 0.0 {
                    segments.push((from, to, length, distance));
                    length += distance;
                }
            }
            if segments.len() > 1_000_000 {
                return Err(Error::Invalid("path segment budget exceeded"));
            }
        }
        if length <= 0.0 || !length.is_finite() {
            return Err(Error::Invalid("empty path"));
        }
        Ok(Self { segments, length })
    }
    pub fn length(&self) -> f32 {
        self.length
    }
    /// Position and clockwise tangent; outside the contour returns None.
    pub fn sample(&self, distance: f32) -> Option<(Point, f32)> {
        if !distance.is_finite() || distance < 0.0 || distance > self.length {
            return None;
        }
        let index = self
            .segments
            .partition_point(|(_, _, start, len)| start + len < distance);
        let (from, to, start, len) = self.segments.get(index)?;
        let v = *to - *from;
        Some((from.lerp(*to, (distance - start) / len), v.y.atan2(v.x)))
    }
}
impl TextRenderer {
    /// Places a single point-text line on a contour. Baseline shifts and mark
    /// offsets follow the local normal. Glyphs beyond the end are omitted and
    /// set overflow. The editable source model is never rewritten.
    pub fn layout_on_path(
        &self,
        model: &TextModel,
        path: &TextPath,
        offset: f32,
    ) -> Result<Layout> {
        if !matches!(model.text_box, TextBox::Point) || !offset.is_finite() {
            return Err(Error::Invalid(
                "path text requires point text and finite offset",
            ));
        }
        let mut layout = self.layout(model)?;
        if layout.lines.len() > 1 {
            return Err(Error::Invalid("path text requires one line"));
        }
        let baseline = layout.lines.first().map_or(0.0, |l| l.baseline);
        let mut placed = Vec::new();
        for mut glyph in layout.glyphs {
            if let Some((at, angle)) = path.sample(offset + glyph.x) {
                let normal = glyph.y - baseline;
                glyph.x = at.x - normal * angle.sin();
                glyph.y = at.y + normal * angle.cos();
                glyph.angle = angle;
                placed.push(glyph);
            } else {
                layout.overflow = true;
            }
        }
        layout.glyphs = placed;
        if let Some(line) = layout.lines.first_mut() {
            line.glyphs = 0..layout.glyphs.len();
        }
        Ok(layout)
    }
}

/// Regular 64×8 deformation mesh. Sampling is bilinear, then flattened
/// contours are subdivided before mapping so straight glyph stems also bend.
struct Mesh {
    origin: Point,
    width: f32,
    height: f32,
    nodes: Vec<Point>,
}
impl Mesh {
    fn new(bounds: [f32; 4], warp: Warp) -> Self {
        let origin = point(bounds[0], bounds[1]);
        let width = (bounds[2] - bounds[0]).max(0.001);
        let height = (bounds[3] - bounds[1]).max(0.001);
        let mut nodes = Vec::new();
        for row in 0..=8 {
            for col in 0..=64 {
                let u = col as f32 / 64.0;
                let v = row as f32 / 8.0;
                let displacement = match warp.kind {
                    WarpKind::Arc => -4.0 * u * (1.0 - u),
                    WarpKind::Wave => (u * std::f32::consts::TAU).sin(),
                    WarpKind::Flag => (u * std::f32::consts::TAU).sin() * (0.5 + v),
                } * warp.amount
                    * width;
                nodes.push(point(
                    origin.x + u * width,
                    origin.y + v * height + displacement,
                ));
            }
        }
        Self {
            origin,
            width,
            height,
            nodes,
        }
    }
    fn map(&self, p: Point) -> Point {
        let u = ((p.x - self.origin.x) / self.width * 64.0).clamp(0.0, 64.0);
        let v = ((p.y - self.origin.y) / self.height * 8.0).clamp(0.0, 8.0);
        let col = (u as usize).min(63);
        let row = (v as usize).min(7);
        let a = self.nodes[row * 65 + col].lerp(self.nodes[row * 65 + col + 1], u - col as f32);
        let b = self.nodes[(row + 1) * 65 + col]
            .lerp(self.nodes[(row + 1) * 65 + col + 1], u - col as f32);
        a.lerp(b, v - row as f32)
    }
    fn line(&self, b: &mut lyon_path::path::Builder, from: Point, to: Point) {
        let delta = to - from;
        let steps = (delta.x.abs() / self.width * 128.0)
            .max(delta.y.abs() / self.height * 16.0)
            .ceil()
            .max(1.0) as usize;
        for i in 1..=steps {
            b.line_to(self.map(from.lerp(to, i as f32 / steps as f32)));
        }
    }
}
pub(crate) fn warp_outlines(outlines: &mut [GlyphOutline], warp: Warp, tolerance: f32) {
    if warp.amount == 0.0 {
        return;
    } // exact identity, including curve commands
    let mut bounds = [
        f32::INFINITY,
        f32::INFINITY,
        f32::NEG_INFINITY,
        f32::NEG_INFINITY,
    ];
    for outline in outlines.iter() {
        for event in outline.path.iter().flattened(tolerance) {
            let p = match event {
                lyon_path::Event::Begin { at } => at,
                lyon_path::Event::Line { to, .. } => to,
                _ => continue,
            };
            bounds[0] = bounds[0].min(p.x);
            bounds[1] = bounds[1].min(p.y);
            bounds[2] = bounds[2].max(p.x);
            bounds[3] = bounds[3].max(p.y);
        }
    }
    if !bounds[0].is_finite() {
        return;
    }
    let mesh = Mesh::new(bounds, warp);
    for outline in outlines {
        let mut builder = Path::builder();
        for event in outline.path.iter().flattened(tolerance) {
            use lyon_path::Event::*;
            match event {
                Begin { at } => {
                    builder.begin(mesh.map(at));
                }
                Line { from, to } => mesh.line(&mut builder, from, to),
                End { last, first, close } => {
                    if close {
                        mesh.line(&mut builder, last, first);
                    }
                    builder.end(close);
                }
                _ => unreachable!(),
            }
        }
        outline.path = builder.build();
    }
}
