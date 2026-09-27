//! Sequential panorama in unbalanced scene-linear camera RGB.
//!
//! FAST-9 / 256-bit BRIEF, normalized homography RANSAC and robust direct
//! refinement register each view to its predecessor. Homographies map input
//! pixels to the first view's perspective plane. Cylindrical/spherical output
//! then maps rays from that plane about the first view's image center, using
//! the explicit focal length in pixels (square pixels, centered principal point).
//! This is a planar/pure-rotation model: parallax, large rotations, exposure
//! changes, automatic ordering and full-360 seams are not solved here.
//! Laplacian bands are blended with Gaussian feather masks. No tone/WB applied.
//! Boundary warp deforms a separable ruled mesh before crop or synthesis.
//! `auto_crop` selects the largest entirely covered axis-aligned rectangle.
//! `panorama_with_fill` accepts a filters::caf adapter (the package graph prevents
//! a direct dependency). Coverage records resampled source support, not synthesis.
use crate::{
    LinearImage, Result,
    blend::Blender,
    features::{self, H, ID},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Projection {
    /// Perspective up to 100° horizontal / 80° vertical; cylindrical above
    /// 100° horizontal, spherical above 80° vertical (vertical takes priority).
    Auto,
    Perspective,
    Cylindrical,
    Spherical,
}
#[derive(Clone, Debug)]
pub struct PanoramaOptions {
    /// 0 is identity; 100 pulls the external boundary to the full canvas.
    /// Applied before crop/fill. Values above 100 are rejected.
    pub boundary_warp: u8,
    pub projection: Projection,
    pub focal_pixels: f64,
    pub auto_crop: bool,
    pub fill_edges: bool,
    pub pyramid_levels: usize,
}
impl Default for PanoramaOptions {
    fn default() -> Self {
        Self {
            boundary_warp: 0,
            projection: Projection::Perspective,
            focal_pixels: 1000.,
            auto_crop: true,
            fill_edges: false,
            pyramid_levels: 5,
        }
    }
}
#[derive(Debug)]
pub struct PanoramaResult {
    /// Resolved projection; never Auto.
    pub projection: Projection,
    pub image: LinearImage,
    pub recipe: engine_api::recipe::Recipe,
    pub coverage: Vec<bool>,
    /// Canvas origin in the projected first-view coordinate system, plus crop.
    /// With boundary_warp > 0, interior pixels are nonlinearly deformed;
    /// origin + homographies alone no longer map output pixels to inputs.
    pub origin: [f64; 2],
    /// Estimated input-to-first-view perspective homographies (before projection).
    pub transforms: Vec<[[f64; 3]; 3]>,
}
fn project(p: [f64; 2], c: [f64; 2], o: &PanoramaOptions) -> [f64; 2] {
    let x = (p[0] - c[0]) / o.focal_pixels;
    let y = (p[1] - c[1]) / o.focal_pixels;
    let r = (1. + x * x).sqrt();
    match o.projection {
        Projection::Auto => unreachable!("projection is resolved before rendering"),
        Projection::Perspective => p,
        Projection::Cylindrical => [
            c[0] + o.focal_pixels * x.atan(),
            c[1] + o.focal_pixels * y / r,
        ],
        Projection::Spherical => [
            c[0] + o.focal_pixels * x.atan(),
            c[1] + o.focal_pixels * (y / r).atan(),
        ],
    }
}
fn unproject(p: [f64; 2], c: [f64; 2], o: &PanoramaOptions) -> [f64; 2] {
    if o.projection == Projection::Perspective {
        return p;
    }
    let theta = (p[0] - c[0]) / o.focal_pixels;
    let v = (p[1] - c[1]) / o.focal_pixels;
    let y = if o.projection == Projection::Spherical {
        v.tan()
    } else {
        v
    };
    [
        c[0] + o.focal_pixels * theta.tan(),
        c[1] + o.focal_pixels * y / theta.cos(),
    ]
}
/// Two separable ruled meshes: horizontal boundary vertices on every row,
/// then vertical vertices on every column. Interior vertices interpolate the
/// boundary displacement. Each nondegenerate cell has positive Jacobian;
/// inverse rasterization avoids cracks and never invents support inside holes.
fn boundary_mesh(
    mut pixels: Vec<[f32; 3]>,
    mut mask: Vec<bool>,
    w: usize,
    h: usize,
    amount: u8,
) -> (Vec<[f32; 3]>, Vec<bool>) {
    if amount == 0 {
        return (pixels, mask);
    }
    let strength = f64::from(amount) / 100.;
    for vertical in [false, true] {
        let (lines, length) = if vertical { (w, h) } else { (h, w) };
        let mut dst = vec![[0.; 3]; w * h];
        let mut covered = vec![false; w * h];
        for line in 0..lines {
            let index = |t: usize| if vertical { t * w + line } else { line * w + t };
            let Some(left) = (0..length).find(|&t| mask[index(t)]) else {
                continue;
            };
            let right = (left..length).rfind(|&t| mask[index(t)]).unwrap();
            // A singleton has no area to stretch; leave it unchanged.
            if left == right {
                dst[index(left)] = pixels[index(left)];
                covered[index(left)] = true;
                continue;
            }
            let start = left as f64 * (1. - strength);
            let end = right as f64 + strength * (length - 1 - right) as f64;
            for t in start.ceil() as usize..=end.floor() as usize {
                let source = (left as f64
                    + (t as f64 - start) / (end - start) * (right - left) as f64)
                    .clamp(left as f64, right as f64);
                let a = source.floor() as usize;
                let b = source.ceil() as usize;
                if !mask[index(a)] || !mask[index(b)] {
                    continue;
                }
                let f = (source - a as f64) as f32;
                dst[index(t)] = std::array::from_fn(|c| {
                    pixels[index(a)][c] * (1. - f) + pixels[index(b)][c] * f
                });
                covered[index(t)] = true;
            }
        }
        pixels = dst;
        mask = covered;
    }
    (pixels, mask)
}

fn rectangle(mask: &[bool], w: usize, h: usize) -> [usize; 4] {
    let mut heights = vec![0; w];
    let mut best = [0; 4];
    let mut area = 0;
    for y in 0..h {
        for x in 0..w {
            heights[x] = if mask[y * w + x] { heights[x] + 1 } else { 0 };
        }
        let mut stack: Vec<(usize, usize)> = Vec::new();
        for (x, height) in heights
            .iter()
            .copied()
            .chain(std::iter::once(0))
            .enumerate()
        {
            let mut start = x;
            while let Some(&(left, old)) = stack.last() {
                if old <= height {
                    break;
                }
                stack.pop();
                let a = old * (x - left);
                if a > area {
                    area = a;
                    best = [left, y + 1 - old, x - left, old];
                }
                start = left;
            }
            stack.push((start, height));
        }
    }
    best
}
fn safe_transform(h: H, im: &LinearImage) -> Result<()> {
    let mut sign = 0.;
    for (x, y) in [
        (0., 0.),
        ((im.width - 1) as f64, 0.),
        (0., (im.height - 1) as f64),
        ((im.width - 1) as f64, (im.height - 1) as f64),
    ] {
        let d = h[2][0] * x + h[2][1] * y + h[2][2];
        if !d.is_finite() || d.abs() < 1e-6 || sign * d < 0. {
            return Err("homography crosses projective horizon".into());
        }
        sign = d;
    }
    if features::inverse(h).is_none() {
        return Err("singular panorama transform".into());
    }
    Ok(())
}
pub fn panorama(images: &[LinearImage], options: &PanoramaOptions) -> Result<PanoramaResult> {
    panorama_with_fill(images, options, |_, _| {
        Err("fill_edges requires a filters::caf adapter via panorama_with_fill".into())
    })
}

/// Stitch with caller-supplied content-aware synthesis. This dependency-inverted
/// adapter avoids the filters -> compositor -> merge package cycle.
/// Called only for uncovered pixels after mesh warp, when crop is disabled.
/// Return canvas-sized scene-linear RGB; covered samples are preserved here.
pub fn panorama_with_fill(
    images: &[LinearImage],
    options: &PanoramaOptions,
    fill: impl FnOnce(&LinearImage, &[bool]) -> Result<Vec<[f32; 3]>>,
) -> Result<PanoramaResult> {
    if images.is_empty() || images.len() > 128 {
        return Err("panorama requires 1..128 ordered images".into());
    }
    if !options.focal_pixels.is_finite()
        || options.boundary_warp > 100
        || options.focal_pixels <= 0.
        || !(1..=12).contains(&options.pyramid_levels)
    {
        return Err("invalid panorama options".into());
    }
    let first = &images[0];
    for image in images {
        image.validate()?;
        if image.width < 2 || image.height < 2 {
            return Err("degenerate panorama dimensions".into());
        }
        if image.color_matrix != first.color_matrix
            || image.as_shot_neutral != first.as_shot_neutral
        {
            return Err("incompatible camera color metadata".into());
        }
    }
    let mut transforms = vec![ID];
    for i in 1..images.len() {
        let pair = features::register(&images[i], &images[i - 1])?;
        let h = features::multiply(transforms[i - 1], pair);
        safe_transform(h, &images[i])?;
        transforms.push(h);
    }
    let center = [
        (first.width - 1) as f64 / 2.,
        (first.height - 1) as f64 / 2.,
    ];
    let mut resolved = options.clone();
    if resolved.projection == Projection::Auto {
        let mut low = [f64::INFINITY; 2];
        let mut high = [f64::NEG_INFINITY; 2];
        for (im, transform) in images.iter().zip(&transforms) {
            // Sample the boundary in ray space, not output aspect ratio or
            // image count. Vertical elevation can peak between corners.
            let steps = im.width.max(im.height);
            for t in 0..=steps {
                let s = t as f64 / steps as f64;
                for (x, y) in [
                    (s * (im.width - 1) as f64, 0.),
                    (s * (im.width - 1) as f64, (im.height - 1) as f64),
                    (0., s * (im.height - 1) as f64),
                    ((im.width - 1) as f64, s * (im.height - 1) as f64),
                ] {
                    let p = features::apply(*transform, x, y);
                    let x = (p[0] - center[0]) / options.focal_pixels;
                    let y = (p[1] - center[1]) / options.focal_pixels;
                    let angles = [x.atan(), (y / (1. + x * x).sqrt()).atan()];
                    for k in 0..2 {
                        low[k] = low[k].min(angles[k]);
                        high[k] = high[k].max(angles[k]);
                    }
                }
            }
        }
        resolved.projection = if (high[1] - low[1]).to_degrees() > 80. {
            Projection::Spherical
        } else if (high[0] - low[0]).to_degrees() > 100. {
            Projection::Cylindrical
        } else {
            Projection::Perspective
        };
    }
    let options = &resolved;
    let mut lo = [f64::INFINITY; 2];
    let mut hi = [f64::NEG_INFINITY; 2];
    for (im, h) in images.iter().zip(&transforms) {
        // Dense boundary samples include curved projection extrema.
        for t in 0..=im.width.max(im.height) {
            let s = t as f64 / im.width.max(im.height) as f64;
            for (x, y) in [
                (s * (im.width - 1) as f64, 0.),
                (s * (im.width - 1) as f64, (im.height - 1) as f64),
                (0., s * (im.height - 1) as f64),
                ((im.width - 1) as f64, s * (im.height - 1) as f64),
            ] {
                let p = project(features::apply(*h, x, y), center, options);
                for k in 0..2 {
                    if !p[k].is_finite() || p[k].abs() > 1e7 {
                        return Err("unbounded panorama".into());
                    }
                    lo[k] = lo[k].min(p[k]);
                    hi[k] = hi[k].max(p[k]);
                }
            }
        }
    }
    let mut origin = [lo[0].floor(), lo[1].floor()];
    let w = (hi[0].ceil() - origin[0] + 1.) as usize;
    let h = (hi[1].ceil() - origin[1] + 1.) as usize;
    let n = w.checked_mul(h).ok_or("panorama allocation overflow")?;
    // Bound peak working allocation: one streamed source pyramid plus accumulators.
    if n == 0 || n > 8 * 1024 * 1024 || w > 65536 || h > 65536 {
        return Err("panorama output exceeds 8 megapixel working limit".into());
    }
    let mut coverage = vec![false; n];
    let mut blender = Blender::new(w, h, options.pyramid_levels);
    for (im, transform) in images.iter().zip(&transforms) {
        let inv = features::inverse(*transform).ok_or("singular homography")?;
        let mut pixels = vec![[0.; 3]; n];
        let mut weights = vec![[0.]; n];
        for y in 0..h {
            for x in 0..w {
                let p = unproject(
                    [x as f64 + origin[0], y as f64 + origin[1]],
                    center,
                    options,
                );
                let q = features::apply(inv, p[0], p[1]);
                if let Some(rgb) = im.sample(q[0], q[1]) {
                    let i = y * w + x;
                    coverage[i] = true;
                    pixels[i] = rgb;
                    weights[i][0] = (q[0]
                        .min(q[1])
                        .min((im.width - 1) as f64 - q[0])
                        .min((im.height - 1) as f64 - q[1])
                        + 1.)
                        .min(32.) as f32;
                }
            }
        }
        blender.add(pixels, weights);
    }
    let mut pixels = blender.finish();
    if options.boundary_warp > 0 {
        (pixels, coverage) = boundary_mesh(pixels, coverage, w, h, options.boundary_warp);
    }
    let (mut width, mut height) = (w, h);
    if options.auto_crop {
        let [x, y, cw, ch] = rectangle(&coverage, w, h);
        if cw == 0 || ch == 0 {
            return Err("no covered panorama rectangle".into());
        }
        let mut cropped = Vec::with_capacity(cw * ch);
        for yy in y..y + ch {
            cropped.extend_from_slice(&pixels[yy * w + x..yy * w + x + cw]);
        }
        pixels = cropped;
        coverage = vec![true; cw * ch];
        width = cw;
        height = ch;
        origin[0] += x as f64;
        origin[1] += y as f64;
    } else {
        for (p, covered) in pixels.iter_mut().zip(&coverage) {
            if !covered {
                *p = [0.; 3];
            }
        }
    }
    let mut image = LinearImage {
        width,
        height,
        pixels,
        color_matrix: first.color_matrix,
        as_shot_neutral: first.as_shot_neutral,
    };
    image.validate()?;
    if !options.auto_crop && options.fill_edges && coverage.iter().any(|v| !v) {
        let filled = fill(&image, &coverage)?;
        if filled.len() != image.pixels.len() || filled.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid content-aware fill output".into());
        }
        for ((pixel, covered), new) in image.pixels.iter_mut().zip(&coverage).zip(filled) {
            if !covered {
                *pixel = new;
            }
        }
    }
    Ok(PanoramaResult {
        projection: options.projection,
        recipe: crate::auto_recipe(&image)?,
        image,
        coverage,
        origin,
        transforms,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn largest_covered_rectangle_matches_brute_force() {
        let (w, h) = (5, 4);
        let mut seed = 71u64;
        for _ in 0..100 {
            let mask: Vec<_> = (0..w * h)
                .map(|_| {
                    seed ^= seed << 13;
                    seed ^= seed >> 7;
                    seed ^= seed << 17;
                    !seed.is_multiple_of(3)
                })
                .collect();
            let mut best = 0;
            for y in 0..h {
                for x in 0..w {
                    for yy in y + 1..=h {
                        for xx in x + 1..=w {
                            if (y..yy).all(|j| (x..xx).all(|i| mask[j * w + i])) {
                                best = best.max((yy - y) * (xx - x));
                            }
                        }
                    }
                }
            }
            let [x, y, rw, rh] = rectangle(&mask, w, h);
            assert_eq!(rw * rh, best);
            assert!((y..y + rh).all(|j| (x..x + rw).all(|i| mask[j * w + i])));
        }
    }
    #[test]
    fn output_dimension_limit_is_checked_before_blend_allocation() {
        let im = LinearImage {
            width: 65537,
            height: 2,
            pixels: vec![[0.5; 3]; 65537 * 2],
            color_matrix: ID,
            as_shot_neutral: [1.; 3],
        };
        assert!(
            panorama(&[im], &PanoramaOptions::default())
                .unwrap_err()
                .contains("limit")
        );
    }
}
