use crate::*;
/// Width/height are output pixels, origin is in level-zero document coordinates.
#[derive(Clone, Copy, Debug)]
pub struct Viewport {
    pub width: u32,
    pub height: u32,
    pub origin: Point,
    pub level: u8,
}
impl Viewport {
    fn validate(self) -> Result<usize> {
        let n = u64::from(self.width) * u64::from(self.height);
        if n > 16_777_216 || !self.origin.x.is_finite() || !self.origin.y.is_finite() {
            return Err(Error::Invalid("viewport"));
        }
        Ok(n as usize)
    }
    pub fn pixel_size(self) -> f64 {
        2.0_f64.powi(i32::from(self.level))
    }
}
#[derive(Clone, Debug)]
pub struct Raster<T> {
    pub width: u32,
    pub height: u32,
    pub data: Vec<T>,
}
/// Linear area coverage, not gamma corrected.
pub type CoverageRaster = Raster<f32>;
/// Premultiplied RGBA in the caller's document color space.
pub type RgbaRaster = Raster<Color>;
#[derive(Clone, Debug)]
pub struct ShapeLayer {
    pub shape: Shape,
    pub fill: Option<Fill>,
    pub stroke: Option<(Stroke, Fill)>,
    pub transform: Affine,
}
#[derive(Clone, Copy, Debug)]
pub struct VectorRenderer {
    pub tolerance: f64,
}
impl Default for VectorRenderer {
    fn default() -> Self {
        Self { tolerance: 1e-5 }
    }
}
impl VectorRenderer {
    /// Exact polygon/pixel intersection areas after tolerance-bounded curve flattening.
    pub fn coverage(&self, path: &Path, view: Viewport) -> Result<CoverageRaster> {
        let count = view.validate()?;
        let scale = view.pixel_size();
        let path =
            path.affine(Affine::scale(1. / scale) * Affine::translate(-view.origin.to_vec2()));
        let normalized = path.boolean(&Path::default(), Operation::Combine, self.tolerance)?;
        let contours = normalized.flattened(self.tolerance)?;
        let mut accum = vec![0.; count];
        for (points, _) in contours {
            if points.len() < 3 {
                continue;
            }
            let min_x = points
                .iter()
                .map(|p| p.x)
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.) as u32;
            let max_x = points
                .iter()
                .map(|p| p.x)
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .max(0.)
                .min(f64::from(view.width)) as u32;
            let min_y = points
                .iter()
                .map(|p| p.y)
                .fold(f64::INFINITY, f64::min)
                .floor()
                .max(0.) as u32;
            let max_y = points
                .iter()
                .map(|p| p.y)
                .fold(f64::NEG_INFINITY, f64::max)
                .ceil()
                .max(0.)
                .min(f64::from(view.height)) as u32;
            for y in min_y..max_y {
                for x in min_x..max_x {
                    let mut clipped = points.clone();
                    for (axis, edge, greater) in [
                        (0, f64::from(x), true),
                        (0, f64::from(x) + 1., false),
                        (1, f64::from(y), true),
                        (1, f64::from(y) + 1., false),
                    ] {
                        clipped = clip(&clipped, axis, edge, greater);
                    }
                    let area = clipped
                        .iter()
                        .zip(clipped.iter().cycle().skip(1))
                        .take(clipped.len())
                        .map(|(a, b)| {
                            (a.x - f64::from(x)) * (b.y - f64::from(y))
                                - (b.x - f64::from(x)) * (a.y - f64::from(y))
                        })
                        .sum::<f64>()
                        / 2.;
                    accum[(y * view.width + x) as usize] += area;
                }
            }
        }
        Ok(Raster {
            width: view.width,
            height: view.height,
            data: accum
                .into_iter()
                .map(|v| v.abs().clamp(0., 1.) as f32)
                .collect(),
        })
    }
    pub fn rgba(&self, path: &Path, fill: &Fill, view: Viewport) -> Result<RgbaRaster> {
        fill.validate()?;
        let coverage = self.coverage(path, view)?;
        let scale = view.pixel_size();
        let data = coverage
            .data
            .iter()
            .enumerate()
            .map(|(i, a)| {
                let p = view.origin
                    + Vec2::new(
                        (i % view.width as usize) as f64 + 0.5,
                        (i / view.width as usize) as f64 + 0.5,
                    ) * scale;
                let c = fill.sample(p);
                let alpha = c[3] * a;
                [c[0] * alpha, c[1] * alpha, c[2] * alpha, alpha]
            })
            .collect();
        Ok(Raster {
            width: view.width,
            height: view.height,
            data,
        })
    }
    pub fn layer(&self, layer: &ShapeLayer, view: Viewport) -> Result<RgbaRaster> {
        let path = layer.shape.path()?;
        let transformed = path.affine(layer.transform);
        let mut output = if let Some(fill) = &layer.fill {
            self.rgba(&transformed, fill, view)?
        } else {
            Raster {
                width: view.width,
                height: view.height,
                data: vec![[0.; 4]; view.validate()?],
            }
        };
        if let Some((stroke, fill)) = &layer.stroke {
            let outline = stroke
                .outline(&path, self.tolerance)?
                .affine(layer.transform);
            let raster = self.rgba(&outline, fill, view)?;
            for (dst, src) in output.data.iter_mut().zip(raster.data) {
                for i in 0..4 {
                    dst[i] = src[i] + dst[i] * (1. - src[3]);
                }
            }
        }
        Ok(output)
    }
}
fn clip(points: &[Point], axis: usize, edge: f64, greater: bool) -> Vec<Point> {
    let coord = |p: Point| if axis == 0 { p.x } else { p.y };
    let inside = |p: Point| {
        if greater {
            coord(p) >= edge
        } else {
            coord(p) <= edge
        }
    };
    let mut result = vec![];
    for (&a, &b) in points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .take(points.len())
    {
        let ai = inside(a);
        let bi = inside(b);
        if ai != bi {
            let t = (edge - coord(a)) / (coord(b) - coord(a));
            result.push(a.lerp(b, t));
        }
        if bi {
            result.push(b);
        }
    }
    result
}
