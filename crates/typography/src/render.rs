use crate::{Error, Glyph, Layout, Result, TextModel, TextRenderer};
use lyon_path::{Path, math::point};
use tiny_skia::{FillRule, Paint, PathBuilder as SkiaBuilder, Pixmap, Transform};

/// Local interleaved premultiplied sRGBA8. This is deliberately NOT the
/// compositor Raster, whose pixels are straight RGBA. See NEEDS.md.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Raster {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}
/// Half-open pixel bounds in the zoomed document coordinate system.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Bounds {
    pub x0: i32,
    pub y0: i32,
    pub x1: i32,
    pub y1: i32,
}
#[derive(Clone, Debug)]
pub struct RenderedText {
    pub raster: Raster,
    pub bounds: Bounds,
    pub zoom: f32,
    pub layout: Layout,
}
/// Exact quadratic/cubic contours in document coordinates for M5-13.
#[derive(Clone, Debug)]
pub struct GlyphOutline {
    pub path: Path,
    pub color: [u8; 4],
    pub cluster: usize,
}
struct OutlineBuilder<'a> {
    builder: lyon_path::path::Builder,
    glyph: &'a Glyph,
    scale: f32,
    open: bool,
}
impl OutlineBuilder<'_> {
    fn point(&self, x: f32, y: f32) -> lyon_path::math::Point {
        let (sin, cos) = self.glyph.angle.sin_cos();
        let (x, y) = (x * self.scale, -y * self.scale);
        point(
            self.glyph.x + cos * x - sin * y,
            self.glyph.y + sin * x + cos * y,
        )
    }
}
impl ttf_parser::OutlineBuilder for OutlineBuilder<'_> {
    fn move_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.builder.end(false);
        }
        self.builder.begin(self.point(x, y));
        self.open = true;
    }
    fn line_to(&mut self, x: f32, y: f32) {
        self.builder.line_to(self.point(x, y));
    }
    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.builder
            .quadratic_bezier_to(self.point(x1, y1), self.point(x, y));
    }
    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.builder
            .cubic_bezier_to(self.point(x1, y1), self.point(x2, y2), self.point(x, y));
    }
    fn close(&mut self) {
        self.builder.end(true);
        self.open = false;
    }
}
impl TextRenderer {
    /// Exports unhinted, positioned contours. Re-resolves variable coordinates
    /// on the same font face used for shaping. Spaces have empty contours.
    pub fn outlines(&self, model: &TextModel, layout: &Layout) -> Result<Vec<GlyphOutline>> {
        self.outlines_at_tolerance(model, layout, 0.01)
    }
    fn outlines_at_tolerance(
        &self,
        model: &TextModel,
        layout: &Layout,
        tolerance: f32,
    ) -> Result<Vec<GlyphOutline>> {
        model.validate()?;
        let mut outlines = Vec::new();
        for glyph in &layout.glyphs {
            let run = model
                .runs
                .get(glyph.run)
                .ok_or(Error::Invalid("layout/model mismatch"))?;
            if [glyph.x, glyph.y, glyph.angle]
                .iter()
                .any(|v| !v.is_finite())
            {
                return Err(Error::Invalid("non-finite glyph position"));
            }
            let path = self
                .fonts
                .with_face_data(glyph.font, |data, index| {
                    let mut face = ttf_parser::Face::parse(data, index).map_err(|_| Error::Font)?;
                    for (tag, value) in &run.axes {
                        face.set_variation(
                            ttf_parser::Tag::from_bytes_lossy(tag.as_bytes()),
                            *value,
                        );
                    }
                    let mut builder = OutlineBuilder {
                        builder: Path::builder(),
                        glyph,
                        scale: run.size / face.units_per_em() as f32,
                        open: false,
                    };
                    face.outline_glyph(ttf_parser::GlyphId(glyph.id), &mut builder);
                    if builder.open {
                        builder.builder.end(false);
                    }
                    Ok::<_, Error>(builder.builder.build())
                })
                .ok_or(Error::Font)??;
            outlines.push(GlyphOutline {
                path,
                color: run.color,
                cluster: glyph.cluster,
            });
        }
        crate::geometry::warp_outlines(&mut outlines, model.warp, tolerance);
        Ok(outlines)
    }
    pub fn render(&self, model: &TextModel, zoom: f32) -> Result<RenderedText> {
        let layout = if let Some(path) = &model.path {
            let contour = crate::TextPath::new(&path.to_lyon()?, 0.01)?;
            self.layout_on_path(model, &contour, path.offset)?
        } else {
            self.layout(model)?
        };
        self.render_layout(model, layout, zoom)
    }
    /// Rasterizes a layout (including one positioned on a path), never scales
    /// an existing bitmap. No grid fitting, LCD fringes or hinting is applied.
    pub fn render_layout(
        &self,
        model: &TextModel,
        layout: Layout,
        zoom: f32,
    ) -> Result<RenderedText> {
        if !zoom.is_finite() || zoom <= 0.0 || zoom > 4096.0 {
            return Err(Error::Invalid("zoom must be in (0, 4096]"));
        }
        let outlines = self.outlines_at_tolerance(model, &layout, 0.1 / zoom)?;
        let mut paths = Vec::new();
        let mut extent: Option<[f32; 4]> = None;
        for outline in outlines {
            let mut builder = SkiaBuilder::new();
            for event in outline.path.iter() {
                use lyon_path::Event::*;
                match event {
                    Begin { at } => builder.move_to(at.x * zoom, at.y * zoom),
                    Line { to, .. } => builder.line_to(to.x * zoom, to.y * zoom),
                    Quadratic { ctrl, to, .. } => {
                        builder.quad_to(ctrl.x * zoom, ctrl.y * zoom, to.x * zoom, to.y * zoom)
                    }
                    Cubic {
                        ctrl1, ctrl2, to, ..
                    } => builder.cubic_to(
                        ctrl1.x * zoom,
                        ctrl1.y * zoom,
                        ctrl2.x * zoom,
                        ctrl2.y * zoom,
                        to.x * zoom,
                        to.y * zoom,
                    ),
                    End { close: true, .. } => builder.close(),
                    End { .. } => {}
                }
            }
            if let Some(path) = builder.finish() {
                let b = path.bounds();
                let e = extent.get_or_insert([b.left(), b.top(), b.right(), b.bottom()]);
                e[0] = e[0].min(b.left());
                e[1] = e[1].min(b.top());
                e[2] = e[2].max(b.right());
                e[3] = e[3].max(b.bottom());
                paths.push((path, outline.color));
            }
        }
        let Some(e) = extent else {
            return Ok(RenderedText {
                raster: Raster::default(),
                bounds: Bounds::default(),
                zoom,
                layout,
            });
        };
        if e.iter().any(|v| !v.is_finite() || v.abs() > 100_000_000.0) {
            return Err(Error::Invalid("raster coordinate limit exceeded"));
        }
        let bounds = Bounds {
            x0: e[0].floor() as i32 - 1,
            y0: e[1].floor() as i32 - 1,
            x1: e[2].ceil() as i32 + 1,
            y1: e[3].ceil() as i32 + 1,
        };
        let width = (bounds.x1 - bounds.x0) as u32;
        let height = (bounds.y1 - bounds.y0) as u32;
        if u64::from(width) * u64::from(height) > 64 * 1024 * 1024 {
            return Err(Error::Invalid("raster exceeds 64 megapixel budget"));
        }
        let mut pixmap =
            Pixmap::new(width, height).ok_or(Error::Invalid("raster allocation failed"))?;
        let transform = Transform::from_translate(-bounds.x0 as f32, -bounds.y0 as f32);
        for (path, color) in paths {
            let mut paint = Paint::default();
            paint.set_color_rgba8(color[0], color[1], color[2], color[3]);
            paint.anti_alias = true;
            pixmap.fill_path(&path, &paint, FillRule::Winding, transform, None);
        }
        Ok(RenderedText {
            raster: Raster {
                width,
                height,
                rgba: pixmap.take(),
            },
            bounds,
            zoom,
            layout,
        })
    }
}
