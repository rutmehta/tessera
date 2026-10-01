//! B5-20b: Adaptive Wide Angle above the dense displacement lattice.
//!
//! `transform::adaptive::Adaptive::solve` emits one lattice vertex per output
//! pixel corner, capped at [`DENSE_VERTICES`] (`(w + 1)(h + 1)`), so a
//! 6000 × 4000 photo cannot be solved directly. The cap stays a cap on the
//! *solve lattice*: [`Lattice::solve`] solves a uniformly scaled copy of the
//! same stored recipe on a lattice of at most `max_vertices`, and [`render`]
//! samples the full-resolution layer through it with the shared
//! `transform::sample` kernel. The stored recipe (source-pixel coordinates) is
//! never changed and no field is added.
//!
//! # Scaling (exactness)
//! With factor `f`, focal lengths, the optical centre and the traced samples
//! scale by `f`; the objective (constraint, membrane, bending and anchor
//! terms) is quadratic in output pixels, so the scaled minimiser is `f` times
//! the full one and the residual check scales with `line_tolerance · f`.
//! - When `f · size` is an integer for every source/output axis (chosen via
//!   their gcd) the scaled recipe is exact, including lens profiles.
//! - Otherwise each axis is rounded up and the crop absorbs the half-size
//!   shift, so Manual cameras stay exact; the mesh domain (and a profile's
//!   axis normalization) differ by under one coarse pixel in several
//!   thousand.
//! - The solver densifies each source segment into `ceil(len / 4)` (≤ 64)
//!   samples; short segments get the full-resolution sample count by
//!   inserting those samples before scaling, so both solves weigh the same
//!   points.
//!
//! What remains is the bilinear interpolation of a smooth map between coarse
//! vertices (spacing `1 / f` px) and a transparent-hole boundary that moves
//! by at most one coarse cell. Tolerances are stated and tested in
//! `tests/adaptive_lattice_equivalence.rs`.
//!
//! # Limits
//! Layers up to [`MAX_PIXELS`] (the transform crate's image limit).
//! Rendering is deterministic: every output pixel is computed independently
//! of the thread partition.
use compositor::{
    geom::Rect,
    raster::{Depth, Raster},
};
use engine_api::{EngineError, EngineResult, jobs::CancellationToken, tile::TILE_SIZE};
use std::{
    cell::Cell,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use transform::{
    Image, Point,
    adaptive::{Adaptive, CameraModel, LineConstraint},
    displacement::Displacement,
    sample::{Kernel, sample},
};

/// The dense (one vertex per pixel corner) lattice cap.
pub const DENSE_VERTICES: usize = transform::displacement::MAX_VERTICES;
/// Solve-lattice budget for layers over [`DENSE_VERTICES`]: 2,097,152
/// vertices (about 1448², a vertex every 3.4 px on a 24 MP layer).
pub const COARSE_VERTICES: usize = 1 << 21;
/// Largest layer Adaptive Wide Angle renders.
pub const MAX_PIXELS: usize = transform::MAX_IMAGE_PIXELS;

/// Whether a `width × height` output fits the dense lattice.
pub fn dense_fits(width: usize, height: usize) -> bool {
    width
        .checked_add(1)
        .zip(height.checked_add(1))
        .and_then(|(w, h)| w.checked_mul(h))
        .is_some_and(|n| n <= DENSE_VERTICES)
}

/// The user-facing refusal for layers Adaptive Wide Angle cannot render
/// (`None`: supported, through the dense or the coarse lattice).
pub fn size_refusal(width: usize, height: usize) -> Option<String> {
    let pixels = width.checked_mul(height);
    (width == 0 || height == 0 || pixels.is_none_or(|n| n > MAX_PIXELS)).then(|| {
        format!(
            "Adaptive Wide Angle supports layers up to {} megapixels; this layer is {width} × {height} ({:.1} megapixels)",
            MAX_PIXELS / 1_000_000,
            width as f64 * height as f64 / 1e6
        )
    })
}

thread_local! {
    static FORCED: Cell<Option<usize>> = const { Cell::new(None) };
}

/// Test hook: inside `f`, `adaptive_wide_angle` stages evaluated on this
/// thread take the coarse path with `max_vertices`, whatever the layer size.
pub fn with_coarse_budget<R>(max_vertices: usize, f: impl FnOnce() -> R) -> R {
    struct Restore(Option<usize>);
    impl Drop for Restore {
        fn drop(&mut self) {
            FORCED.with(|c| c.set(self.0));
        }
    }
    let _restore = Restore(FORCED.with(|c| c.replace(Some(max_vertices))));
    f()
}

/// The coarse budget to use for a `width × height` layer, `None` for the
/// dense path.
pub(crate) fn coarse_budget(width: usize, height: usize) -> Option<usize> {
    FORCED
        .with(Cell::get)
        .or_else(|| (!dense_fits(width, height)).then_some(COARSE_VERTICES))
}

/// A solved coarse lattice for a full-resolution recipe.
#[derive(Clone, Debug, PartialEq)]
pub struct Lattice {
    field: Displacement,
    factor: f64,
    source: [usize; 2],
    output: [usize; 2],
}

impl Lattice {
    /// Solves `recipe` (full-resolution, as stored) on a lattice of at most
    /// `max_vertices` vertices. Recipes that already fit are solved as is.
    pub fn solve(recipe: &Adaptive, max_vertices: usize) -> transform::Result<Self> {
        let invalid = |m: &str| transform::Error::Invalid(m.into());
        let sizes = [
            recipe.source_width,
            recipe.source_height,
            recipe.output_width,
            recipe.output_height,
        ];
        if sizes.contains(&0) || max_vertices < 16 {
            return Err(invalid(
                "adaptive lattice needs a nonempty canvas and budget",
            ));
        }
        let (factor, coarse) = factor_and_sizes(sizes, max_vertices)
            .ok_or_else(|| invalid("adaptive lattice budget too small for this canvas"))?;
        let field = if factor == 1.0 {
            recipe.solve()?
        } else {
            scale_recipe(recipe, factor, coarse).solve()?
        };
        Ok(Self {
            field,
            factor,
            source: [recipe.source_width, recipe.source_height],
            output: [recipe.output_width, recipe.output_height],
        })
    }

    /// Coarse lattice size over full size (≤ 1).
    pub fn factor(&self) -> f64 {
        self.factor
    }

    /// Solve-lattice vertices.
    pub fn vertices(&self) -> usize {
        self.field.coordinates.len()
    }

    /// Full-resolution output position -> full-resolution source position
    /// (pixel-centre convention); `None` in holes and outside the source.
    pub fn inverse(&self, p: Point) -> Option<Point> {
        let f = self.factor;
        let s = self.field.inverse([p[0] * f, p[1] * f])?;
        let s = [s[0] / f, s[1] / f];
        (s[0] >= 0. && s[1] >= 0. && s[0] <= self.source[0] as f64 && s[1] <= self.source[1] as f64)
            .then_some(s)
    }
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 { a } else { gcd(b, a % b) }
}

fn lattice(w: usize, h: usize) -> usize {
    (w + 1).saturating_mul(h + 1)
}

/// The factor and the coarse `[source w, source h, output w, output h]`.
fn factor_and_sizes(sizes: [usize; 4], max_vertices: usize) -> Option<(f64, [usize; 4])> {
    let [_, _, ow, oh] = sizes;
    if lattice(ow, oh) <= max_vertices {
        return Some((1.0, sizes));
    }
    // Largest f with (ow f + 1)(oh f + 1) <= max_vertices.
    let (a, b, c) = (
        ow as f64 * oh as f64,
        (ow + oh) as f64,
        1.0 - max_vertices as f64,
    );
    let f_max = ((-b + (b * b - 4.0 * a * c).sqrt()) / (2.0 * a)).min(1.0);
    // Exact: f = m / g makes every axis an integer.
    let g = sizes.into_iter().fold(0, gcd);
    let m = (f_max * g as f64).floor() as usize;
    if m >= 1 && m < g && m as f64 / g as f64 >= 0.75 * f_max {
        let coarse = sizes.map(|n| n / g * m);
        if lattice(coarse[2], coarse[3]) <= max_vertices && !coarse.contains(&0) {
            return Some((m as f64 / g as f64, coarse));
        }
    }
    // Rounded up per axis; shrink until the output lattice fits.
    let mut f = f_max;
    for _ in 0..64 {
        let coarse = sizes.map(|n| ((n as f64 * f).ceil() as usize).max(1));
        if lattice(coarse[2], coarse[3]) <= max_vertices {
            return Some((f, coarse));
        }
        f *= 0.999;
    }
    None
}

/// The solver's densification count for one source segment.
fn steps(a: Point, b: Point) -> usize {
    let length = (b[0] - a[0]).hypot(b[1] - a[1]);
    (length / 4.).ceil().clamp(1., 64.) as usize
}

/// `line` scaled by `f`, with full-resolution sample counts where the scaled
/// segment would be densified differently (`insert`), clamped to the canvas.
fn scaled_line(line: &LineConstraint, f: f64, bound: Point, insert: bool) -> LineConstraint {
    let s = |p: Point| {
        [
            (p[0] * f).clamp(0., bound[0]),
            (p[1] * f).clamp(0., bound[1]),
        ]
    };
    let mut points = Vec::with_capacity(line.points.len());
    for pair in line.points.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        points.push(s(a));
        let full = steps(a, b);
        if insert && full != steps(s(a), s(b)) {
            for i in 1..full {
                let t = i as f64 / full as f64;
                points.push(s([a[0] * (1. - t) + b[0] * t, a[1] * (1. - t) + b[1] * t]));
            }
        }
    }
    if let Some(last) = line.points.last() {
        points.push(s(*last));
    }
    LineConstraint {
        points,
        orientation: line.orientation,
        weight: line.weight,
    }
}

/// `recipe` for a canvas scaled by `f` to `sizes` = `[source w, source h,
/// output w, output h]` (each `ceil(n · f)` or exact; see the module docs).
/// Also what the workspace preview solves on its proxy.
pub fn scale_recipe(recipe: &Adaptive, f: f64, sizes: [usize; 4]) -> Adaptive {
    let mut a = recipe.clone();
    [
        a.source_width,
        a.source_height,
        a.output_width,
        a.output_height,
    ] = sizes;
    a.camera = match &recipe.camera {
        CameraModel::Manual {
            focal_px,
            center,
            projection,
        } => CameraModel::Manual {
            focal_px: focal_px * f,
            center: [center[0] * f, center[1] * f],
            projection: *projection,
        },
        CameraModel::Profile {
            focal_px,
            center,
            calibration,
        } => CameraModel::Profile {
            focal_px: focal_px * f,
            center: [center[0] * f, center[1] * f],
            calibration: calibration.clone(),
        },
    };
    a.output_focal_px *= f;
    // Output = F·ray + source/2 − crop: keep coarse output = f · full output
    // when an axis was rounded up.
    let half = |coarse: usize, full: usize| coarse as f64 / 2. - f * full as f64 / 2.;
    a.crop = [
        f * recipe.crop[0] + half(sizes[0], recipe.source_width),
        f * recipe.crop[1] + half(sizes[1], recipe.source_height),
    ];
    a.line_tolerance = (recipe.line_tolerance * f).max(1e-3);
    let bound = [sizes[0] as f64, sizes[1] as f64];
    let lines = |insert| -> Vec<_> {
        recipe
            .lines
            .iter()
            .map(|l| scaled_line(l, f, bound, insert))
            .collect()
    };
    a.lines = lines(true);
    // Keep within the recipe's input-sample limit.
    if a.lines.iter().map(|l| l.points.len()).sum::<usize>() > 16_384 {
        a.lines = lines(false);
    }
    a
}

/// Renders `input` through `lattice` at full resolution: premultiplied
/// bicubic sampling (the displacement renderer's kernel), straight F32 RGBA
/// out, transparent in holes. `cancel` is checked per output tile.
pub fn render(
    input: &Raster,
    lattice: &Lattice,
    cancel: &CancellationToken,
) -> EngineResult<Raster> {
    let bad = |m: String| EngineError::invalid("adaptive_wide_angle", m);
    let e = input.extent();
    let (w, h) = (e.width as usize, e.height as usize);
    if lattice.source != [w, h] || lattice.output != [w, h] {
        return Err(bad(format!(
            "lattice {:?} → {:?} does not match the {w}×{h} layer",
            lattice.source, lattice.output
        )));
    }
    if w * h > MAX_PIXELS {
        return Err(bad(size_refusal(w, h).unwrap_or_default()));
    }
    let workers = std::thread::available_parallelism().map_or(4, usize::from);
    let (cols, rows) = input.grid();
    let ts = TILE_SIZE as usize;

    // Premultiplied planes, one tile row per work item.
    let mut planes: [Vec<f32>; 4] = std::array::from_fn(|_| vec![0.0f32; w * h]);
    {
        let [r, g, b, a] = &mut planes;
        let bands = Mutex::new(
            r.chunks_mut(ts * w)
                .zip(g.chunks_mut(ts * w))
                .zip(b.chunks_mut(ts * w))
                .zip(a.chunks_mut(ts * w))
                .enumerate()
                .map(|(ty, (((r, g), b), a))| (ty as u32, [r, g, b, a]))
                .collect::<Vec<_>>(),
        );
        let channels = input.channels().min(4);
        let result: EngineResult<()> = std::thread::scope(|scope| {
            let jobs: Vec<_> = (0..workers)
                .map(|_| {
                    scope.spawn(|| -> EngineResult<()> {
                        let mut buf = Vec::new();
                        loop {
                            let Some((ty, band)) = bands.lock().unwrap().pop() else {
                                return Ok(());
                            };
                            cancel.check()?;
                            for tx in 0..cols {
                                input.read_tile(tx, ty, &mut buf)?;
                                let layout = input.layout(tx, ty);
                                let (tw, th) = (layout.extent.width, layout.extent.height);
                                for y in 0..th {
                                    for x in 0..tw {
                                        let mut p = [0.0f32; 4];
                                        for (c, v) in
                                            p.iter_mut().enumerate().take(channels as usize)
                                        {
                                            if let Some(i) =
                                                layout.index(c as u8, x as i32, y as i32)
                                            {
                                                *v = buf[i];
                                            }
                                        }
                                        let o = y as usize * w + (tx * TILE_SIZE + x) as usize;
                                        for c in 0..3 {
                                            band[c][o] = p[c] * p[3];
                                        }
                                        band[3][o] = p[3];
                                    }
                                }
                            }
                        }
                    })
                })
                .collect();
            jobs.into_iter().try_for_each(|j| {
                j.join()
                    .unwrap_or_else(|_| Err(bad("render worker panicked".into())))
            })
        });
        result?;
    }
    let image = Image::new(w, h, planes).map_err(|e| bad(e.to_string()))?;

    // Output tiles, rendered independently and installed in raster order.
    let mut out = Raster::new(e, 4, Depth::F32, 0.0);
    let next = AtomicUsize::new(0);
    let total = cols as usize * rows as usize;
    let rendered: EngineResult<Vec<_>> = std::thread::scope(|scope| {
        let jobs: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| -> EngineResult<Vec<_>> {
                    let mut mine = Vec::new();
                    loop {
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        if i >= total {
                            return Ok(mine);
                        }
                        cancel.check()?;
                        let (tx, ty) = ((i % cols as usize) as u32, (i / cols as usize) as u32);
                        let (x0, y0) = (i64::from(tx * TILE_SIZE), i64::from(ty * TILE_SIZE));
                        let rect =
                            Rect::new(x0, y0, x0 + i64::from(TILE_SIZE), y0 + i64::from(TILE_SIZE));
                        mine.extend(out.render_region(rect, |x, y, px| {
                            let p = [f64::from(x) + 0.5, f64::from(y) + 0.5];
                            let rgba = lattice
                                .inverse(p)
                                .map(|s| [s[0] as f32, s[1] as f32])
                                .filter(|s| s.iter().all(|v| v.is_finite()))
                                .map_or([0.0; 4], |s| sample(&image, s, Kernel::Bicubic));
                            let a = rgba[3];
                            for c in 0..3 {
                                px[c] = if a > 0.0 { rgba[c] / a } else { 0.0 };
                            }
                            px[3] = a;
                        })?);
                    }
                })
            })
            .collect();
        let mut all = Vec::with_capacity(total);
        for j in jobs {
            all.extend(
                j.join()
                    .unwrap_or_else(|_| Err(bad("render worker panicked".into())))?,
            );
        }
        Ok(all)
    });
    let mut tiles = rendered?;
    tiles.sort_by_key(|&(tx, ty, _)| (ty, tx));
    for (tx, ty, tile) in tiles {
        out.set_slot(tx, ty, Some(tile), 1)?;
    }
    cancel.check()?;
    Ok(out)
}

/// Solve on the coarse lattice (`max_vertices`) and render at full size.
pub fn evaluate(
    input: &Raster,
    recipe: &Adaptive,
    max_vertices: usize,
    cancel: &CancellationToken,
) -> EngineResult<Raster> {
    cancel.check()?;
    let lattice = Lattice::solve(recipe, max_vertices)
        .map_err(|e| EngineError::invalid("adaptive_wide_angle", e.to_string()))?;
    render(input, &lattice, cancel)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn factor_is_exact_when_the_axes_share_a_divisor() {
        let (f, s) = factor_and_sizes([6000, 4000, 6000, 4000], COARSE_VERTICES).unwrap();
        assert_eq!(
            s.map(|n| n as f64),
            [6000., 4000., 6000., 4000.].map(|n| n * f)
        );
        assert!(lattice(s[2], s[3]) <= COARSE_VERTICES);
        // 5212 × 3468 share only 4: rounded up per axis instead.
        let (f, s) = factor_and_sizes([5212, 3468, 5212, 3468], COARSE_VERTICES).unwrap();
        assert!(lattice(s[2], s[3]) <= COARSE_VERTICES);
        assert!(f > 0.3 && (s[0] as f64 - 5212. * f) < 1. && (s[1] as f64 - 3468. * f) < 1.);
        assert_eq!(
            factor_and_sizes([100, 80, 100, 80], COARSE_VERTICES)
                .unwrap()
                .0,
            1.0
        );
    }

    #[test]
    fn short_segments_keep_the_full_resolution_sample_count() {
        let line = LineConstraint {
            points: vec![[0., 0.], [40., 0.], [1000., 0.]],
            orientation: transform::adaptive::LineOrientation::Horizontal,
            weight: 1.,
        };
        let l = scaled_line(&line, 0.25, [1e6, 1e6], true);
        // 40 px -> 10 steps (10 px scaled would be 3): 9 inserted; 960 px ->
        // 64 both ways (240 px scaled -> 60): 63 inserted.
        assert_eq!(l.points.len(), 1 + 9 + 1 + 63 + 1);
        assert!(l.points.windows(2).all(|p| steps(p[0], p[1]) == 1));
        assert_eq!(l.points[10], [10., 0.]);
    }

    #[test]
    fn refusal_is_the_absolute_pixel_limit() {
        assert!(size_refusal(6000, 4000).is_none());
        assert!(size_refusal(10000, 10000).is_none());
        let why = size_refusal(12000, 9000).unwrap();
        assert!(
            why.contains("100 megapixels") && why.contains("12000 × 9000"),
            "{why}"
        );
        assert!(dense_fits(4095, 4095) && !dense_fits(4096, 4096) && !dense_fits(5000, 3500));
    }
}
