//! Editable layer alignment and blending. See ../LAYERS.md for coordinate contracts.
use crate::{
    LinearImage, Result,
    features::{self, H, ID},
};
use transform::{Kernel, Operation, TransformOp, free::FreeTransform};
/// Centered, axis-normalized radial calibration. Distortion maps ideal to
/// observed radius by r * (1 + k1*r² + k2*r⁴ + k3*r⁶). Vignette coefficients
/// describe observed illumination, corrected by its reciprocal in linear RGB.
#[derive(Clone, Copy, Debug, Default)]
pub struct LensCorrection {
    pub distortion: [f64; 3],
    pub vignette: [f64; 3],
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AlignMode {
    Auto,
    Perspective,
    Cylindrical,
    Spherical,
    Collage,
    Reposition,
}
#[derive(Clone, Debug)]
pub struct AlignOptions {
    pub mode: AlignMode,
    pub reference: usize,
    pub seed: u64,
    pub vignette_removal: bool,
    pub geometric_distortion: bool,
    /// One calibration per input, in input order. Required for lens removal.
    pub lens_corrections: Vec<LensCorrection>,
}
impl Default for AlignOptions {
    fn default() -> Self {
        Self {
            mode: AlignMode::Auto,
            reference: 0,
            seed: 1,
            vignette_removal: false,
            geometric_distortion: false,
            lens_corrections: Vec::new(),
        }
    }
}
#[derive(Clone, Debug)]
pub struct AlignedLayers {
    pub width: usize,
    pub height: usize,
    pub origin: [f64; 2],
    pub transforms: Vec<TransformOp>,
    pub images: Vec<LinearImage>,
    pub coverage: Vec<Vec<bool>>,
    /// Multiplicative vignette corrections at original source pixel centers.
    /// Empty when vignette removal is disabled; otherwise one plane per input.
    pub source_gains: Vec<Vec<f32>>,
}
fn translation(x: f64, y: f64) -> H {
    [[1., 0., x], [0., 1., y], [0., 0., 1.]]
}
fn validate_images(images: &[LinearImage]) -> Result<()> {
    if images.is_empty() || images.len() > 128 {
        return Err("expected 1..128 layers".into());
    }
    for im in images {
        im.validate()?;
    }
    Ok(())
}
pub fn align_layers(images: &[LinearImage], options: &AlignOptions) -> Result<AlignedLayers> {
    validate_images(images)?;
    if options.reference >= images.len() {
        return Err("invalid alignment reference".into());
    }
    if options.vignette_removal || options.geometric_distortion {
        if options.lens_corrections.len() != images.len() {
            return Err("lens correction requires one calibration per input".into());
        }
        if options.geometric_distortion {
            for calibration in &options.lens_corrections {
                crate::layer_lens::validate(calibration)?;
            }
        }
    }
    if options.vignette_removal {
        let mut corrected = images.to_vec();
        let mut gains = Vec::new();
        for (im, calibration) in corrected.iter_mut().zip(&options.lens_corrections) {
            let [a, b, c] = calibration.vignette;
            let mut plane = Vec::with_capacity(im.pixels.len());
            for (i, pixel) in im.pixels.iter_mut().enumerate() {
                let r2 = (2. * (i % im.width) as f64 / im.width as f64 + 1. / im.width as f64 - 1.)
                    .powi(2)
                    + (2. * (i / im.width) as f64 / im.height as f64 + 1. / im.height as f64 - 1.)
                        .powi(2);
                let illumination = 1. + r2 * (a + r2 * (b + r2 * c));
                if !illumination.is_finite() || !(0.05..=20.).contains(&illumination) {
                    return Err("invalid vignette calibration illumination".into());
                }
                let gain = (1. / illumination) as f32;
                for value in pixel {
                    *value *= gain;
                    if !value.is_finite() {
                        return Err("vignette correction overflow".into());
                    }
                }
                plane.push(gain);
            }
            gains.push(plane);
        }
        let mut out = align_prepared(&corrected, options)?;
        out.source_gains = gains;
        return Ok(out);
    }
    align_prepared(images, options)
}

fn align_prepared(images: &[LinearImage], options: &AlignOptions) -> Result<AlignedLayers> {
    let originals = images;
    let rectified;
    let images = if options.geometric_distortion {
        rectified = images
            .iter()
            .zip(&options.lens_corrections)
            .map(|(im, calibration)| crate::layer_lens::rectify(im, calibration))
            .collect::<Vec<_>>();
        &rectified
    } else {
        images
    };
    let mut matrices = vec![ID; images.len()];
    let reference = &images[options.reference];
    let mut order: Vec<_> = (0..images.len()).collect();
    order.sort_by_key(|i| i.abs_diff(options.reference));
    let mut connected = vec![false; images.len()];
    connected[options.reference] = true;
    for i in order {
        let im = &images[i];
        if i == options.reference {
            continue;
        }
        let rigid = || -> Result<H> {
            for input in [reference, im] {
                let mean = input
                    .pixels
                    .iter()
                    .map(|p| (p[0] as f64 + 2. * p[1] as f64 + p[2] as f64) / 4.)
                    .sum::<f64>()
                    / input.pixels.len() as f64;
                let variance = input
                    .pixels
                    .iter()
                    .map(|p| ((p[0] as f64 + 2. * p[1] as f64 + p[2] as f64) / 4. - mean).powi(2))
                    .sum::<f64>()
                    / input.pixels.len() as f64;
                if variance < 1e-10 || input.width < 16 || input.height < 16 {
                    return Err("insufficient texture for layer registration".into());
                }
            }
            let a = crate::alignment::align_global(reference, im, 1.)?;
            let (s, c) = if options.mode == AlignMode::Reposition {
                (0., 1.)
            } else {
                a.rotation_radians.sin_cos()
            };
            let cx = (reference.width as f64 - 1.) / 2.;
            let cy = (reference.height as f64 - 1.) / 2.;
            features::inverse([
                [c, -s, cx - c * cx + s * cy + a.translation[0]],
                [s, c, cy - s * cx - c * cy + a.translation[1]],
                [0., 0., 1.],
            ])
            .ok_or_else(|| "singular alignment".into())
        };
        if options.mode != AlignMode::Reposition {
            // Connect through already registered neighbors; the last crop need not overlap reference.
            let mut candidates: Vec<_> = (0..images.len()).filter(|j| connected[*j]).collect();
            candidates.sort_by_key(|j| i.abs_diff(*j));
            let mut registered = None;
            for j in candidates {
                if let Ok(h) = features::register_seeded(im, &images[j], options.seed) {
                    registered = Some(features::multiply(
                        matrices[j],
                        if options.mode == AlignMode::Collage {
                            features::similarity(im, &images[j], h)
                        } else {
                            h
                        },
                    ));
                    break;
                }
            }
            matrices[i] = if let Some(h) = registered {
                h
            } else {
                let h = rigid()?;
                if options.mode == AlignMode::Collage {
                    features::similarity(im, reference, h)
                } else {
                    h
                }
            };
            connected[i] = true;
            continue;
        }
        matrices[i] = match options.mode {
            AlignMode::Collage | AlignMode::Reposition => rigid()?,
            _ => features::register_seeded(im, reference, options.seed).or_else(|_| rigid())?,
        };
    }
    render_aligned(originals, &matrices, options)
}
fn projection(p: [f64; 2], reference: &LinearImage, mode: AlignMode) -> [f64; 2] {
    let f = reference.width.max(reference.height) as f64;
    let cx = reference.width as f64 / 2.;
    let cy = reference.height as f64 / 2.;
    let x = (p[0] - cx) / f;
    let y = (p[1] - cy) / f;
    let r = (1. + x * x).sqrt();
    [
        cx + f * x.atan(),
        cy + f * if mode == AlignMode::Spherical {
            (y / r).atan()
        } else {
            y / r
        },
    ]
}
fn projection_mesh(
    im: &LinearImage,
    h: H,
    reference: &LinearImage,
    mode: AlignMode,
    calibration: Option<&LensCorrection>,
) -> Result<transform::warp::WarpMesh> {
    // Degree-elevated bilinear patches: convex hull bounds are conservative.
    // Refine until all 5x5 probes in every patch agree to 0.05 destination pixel.
    let map = |u: f64, v: f64| {
        let p = [u * im.width as f64, v * im.height as f64];
        let p = if let Some(calibration) = calibration {
            crate::layer_lens::undistort(p, im, calibration).unwrap_or([f64::NAN; 2])
        } else {
            p
        };
        let p = features::apply(h, p[0], p[1]);
        if matches!(mode, AlignMode::Cylindrical | AlignMode::Spherical) {
            projection(p, reference, mode)
        } else {
            p
        }
    };
    for segments in [4, 8, 16, 32, 64, 128] {
        let mut mesh = transform::warp::WarpMesh::identity(im.width as f64, im.height as f64);
        mesh.u_splits = (0..=segments).map(|i| i as f64 / segments as f64).collect();
        mesh.v_splits = mesh.u_splits.clone();
        mesh.control_points = vec![vec![[0.; 2]; 3 * segments + 1]; 3 * segments + 1];
        let mut error = 0_f64;
        for y in 0..segments {
            for x in 0..segments {
                let corners = [
                    map(x as f64 / segments as f64, y as f64 / segments as f64),
                    map((x + 1) as f64 / segments as f64, y as f64 / segments as f64),
                    map(x as f64 / segments as f64, (y + 1) as f64 / segments as f64),
                    map(
                        (x + 1) as f64 / segments as f64,
                        (y + 1) as f64 / segments as f64,
                    ),
                ];
                let lerp = |u: f64, v: f64| -> [f64; 2] {
                    std::array::from_fn(|k| {
                        corners[0][k] * (1. - u) * (1. - v)
                            + corners[1][k] * u * (1. - v)
                            + corners[2][k] * (1. - u) * v
                            + corners[3][k] * u * v
                    })
                };
                for j in 0..4 {
                    for i in 0..4 {
                        mesh.control_points[y * 3 + j][x * 3 + i] =
                            lerp(i as f64 / 3., j as f64 / 3.);
                    }
                }
                for j in 0..5 {
                    for i in 0..5 {
                        let u = i as f64 / 4.;
                        let v = j as f64 / 4.;
                        let a = lerp(u, v);
                        let b = map(
                            (x as f64 + u) / segments as f64,
                            (y as f64 + v) / segments as f64,
                        );
                        let e = (a[0] - b[0]).hypot(a[1] - b[1]);
                        if !e.is_finite() {
                            return Err("nonfinite projection".into());
                        }
                        error = error.max(e);
                    }
                }
            }
        }
        mesh.validate().map_err(|e| e.to_string())?;
        if error <= 0.05 {
            return Ok(mesh);
        }
    }
    Err("projection mesh exceeds sampled 0.05 pixel tolerance".into())
}
fn render_aligned(
    images: &[LinearImage],
    matrices: &[H],
    options: &AlignOptions,
) -> Result<AlignedLayers> {
    let mut bounds = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
    let mut edge = Vec::new();
    for (index, (im, h)) in images.iter().zip(matrices).enumerate() {
        let matrix = features::multiply(
            translation(0.5, 0.5),
            features::multiply(*h, translation(-0.5, -0.5)),
        );
        let free = FreeTransform { matrix };
        let mut b = free
            .bounds(im.width as f64, im.height as f64)
            .map_err(|e| e.to_string())?;
        let operation = if options.geometric_distortion
            || matches!(options.mode, AlignMode::Cylindrical | AlignMode::Spherical)
        {
            let calibration = options
                .geometric_distortion
                .then(|| &options.lens_corrections[index]);
            let mesh = projection_mesh(
                im,
                matrix,
                &images[options.reference],
                options.mode,
                calibration,
            )?;
            b = [[f64::INFINITY; 2], [f64::NEG_INFINITY; 2]];
            for p in mesh.control_points.iter().flatten() {
                for k in 0..2 {
                    b[0][k] = b[0][k].min(p[k]);
                    b[1][k] = b[1][k].max(p[k]);
                }
            }
            Operation::Warp(mesh)
        } else {
            Operation::Free(free)
        };
        if b.iter().flatten().any(|v| !v.is_finite()) {
            return Err("invalid union bounds".into());
        }
        for k in 0..2 {
            bounds[0][k] = bounds[0][k].min(b[0][k]);
            bounds[1][k] = bounds[1][k].max(b[1][k]);
        }
        edge.push(operation);
    }
    let origin = bounds[0].map(f64::floor);
    let width = (bounds[1][0].ceil() - origin[0]) as usize;
    let height = (bounds[1][1].ceil() - origin[1]) as usize;
    if width
        .checked_mul(height)
        .is_none_or(|n| n == 0 || n > 64 * 1024 * 1024)
    {
        return Err("alignment canvas exceeds 64 MP".into());
    }
    let mut out = AlignedLayers {
        width,
        height,
        origin,
        transforms: Vec::new(),
        images: Vec::new(),
        coverage: Vec::new(),
        source_gains: Vec::new(),
    };
    for (im, mut operation) in images.iter().zip(edge) {
        match &mut operation {
            Operation::Free(t) => {
                t.matrix = features::multiply(translation(-origin[0], -origin[1]), t.matrix)
            }
            Operation::Warp(mesh) => {
                for p in mesh.control_points.iter_mut().flatten() {
                    p[0] -= origin[0];
                    p[1] -= origin[1];
                }
            }
            _ => unreachable!(),
        }
        let op = TransformOp {
            version: 1,
            operation,
            kernel: Kernel::Bilinear,
        };
        let input = transform::Image::new(
            im.width,
            im.height,
            std::array::from_fn(|c| {
                im.pixels
                    .iter()
                    .map(|p| if c < 3 { p[c] } else { 1. })
                    .collect()
            }),
        )
        .map_err(|e| e.to_string())?;
        let rendered = op
            .apply(&input, width, height, 0)
            .map_err(|e| e.to_string())?;
        let coverage: Vec<_> = rendered.planes[3].iter().map(|a| *a > 1e-6).collect();
        let pixels = (0..width * height)
            .map(|i| {
                std::array::from_fn(|c| {
                    if coverage[i] {
                        rendered.planes[c][i] / rendered.planes[3][i]
                    } else {
                        0.
                    }
                })
            })
            .collect();
        out.images.push(LinearImage {
            width,
            height,
            pixels,
            color_matrix: im.color_matrix,
            as_shot_neutral: im.as_shot_neutral,
        });
        out.coverage.push(coverage);
        out.transforms.push(op);
    }
    Ok(out)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlendMode {
    Panorama,
    StackImages,
}
#[derive(Clone, Debug)]
pub struct BlendOptions {
    pub mode: BlendMode,
    pub seamless_tones: bool,
    pub fill_transparent: bool,
    pub pyramid_levels: usize,
    pub seed: u64,
}
impl Default for BlendOptions {
    fn default() -> Self {
        Self {
            mode: BlendMode::Panorama,
            seamless_tones: true,
            fill_transparent: false,
            pyramid_levels: 5,
            seed: 1,
        }
    }
}
#[derive(Clone, Debug)]
pub struct BlendedLayers {
    pub masks: Vec<Vec<f32>>,
    pub corrections: Vec<Vec<[f32; 3]>>,
    pub image: LinearImage,
    pub coverage: Vec<bool>,
    pub fill_mask: Vec<bool>,
}
fn focus(im: &LinearImage, coverage: &[bool], levels: usize) -> Vec<f64> {
    let (mut w, mut h) = (im.width, im.height);
    let mut gray: Vec<[f32; 3]> = im
        .pixels
        .iter()
        .map(|p| [(p[0] + p[1] + p[2]) / 3.; 3])
        .collect();
    let mut valid: Vec<[f32; 1]> = coverage
        .iter()
        .map(|v| [if *v { 1. } else { 0. }])
        .collect();
    let mut bands = Vec::new();
    let mut dims = Vec::new();
    for _ in 0..levels {
        if w < 2 || h < 2 {
            break;
        }
        let (nw, nh) = (w.div_ceil(2), h.div_ceil(2));
        let next = crate::blend::down(&gray, w, h);
        let next_valid = crate::blend::down(&valid, w, h);
        let expanded = crate::blend::up(&next, nw, nh, w, h);
        let support: Vec<[f32; 3]> = next_valid.iter().map(|v| [v[0]; 3]).collect();
        let support = crate::blend::up(&support, nw, nh, w, h);
        let energy: Vec<f32> = gray
            .iter()
            .zip(&expanded)
            .enumerate()
            .map(|(i, (g, e))| {
                if valid[i][0] > 0.99999 && support[i][0] > 0.99999 {
                    (g[0] - e[0]).abs()
                } else {
                    0.
                }
            })
            .collect();
        let mut integrated = vec![[0.; 3]; w * h];
        for y in 0..h {
            for x in 0..w {
                let mut sum = 0.;
                let mut n = 0.;
                for yy in y.saturating_sub(2)..=(y + 2).min(h - 1) {
                    for xx in x.saturating_sub(2)..=(x + 2).min(w - 1) {
                        sum += energy[yy * w + xx];
                        n += 1.;
                    }
                }
                integrated[y * w + x] = [sum / n; 3];
            }
        }
        bands.push(integrated);
        dims.push((w, h));
        gray = next;
        valid = next_valid;
        w = nw;
        h = nh;
    }
    if bands.is_empty() {
        return vec![0.; im.width * im.height];
    }
    let mut score = bands.pop().unwrap();
    for l in (0..bands.len()).rev() {
        let (w, h) = dims[l + 1];
        let (nw, nh) = dims[l];
        score = crate::blend::up(&score, w, h, nw, nh);
        for (p, b) in score.iter_mut().zip(&bands[l]) {
            for c in 0..3 {
                p[c] += b[c];
            }
        }
    }
    score.iter().map(|p| p[0] as f64).collect()
}
pub fn blend_layers(
    images: &[LinearImage],
    coverage: &[Vec<bool>],
    options: &BlendOptions,
) -> Result<BlendedLayers> {
    validate_images(images)?;
    let (w, h) = (images[0].width, images[0].height);
    let n = w * h;
    if coverage.len() != images.len()
        || coverage.iter().any(|c| c.len() != n)
        || images.iter().any(|im| im.width != w || im.height != h)
    {
        return Err("aligned images and coverage must share canvas".into());
    }
    if !(1..=12).contains(&options.pyramid_levels) {
        return Err("pyramid levels must be 1..12".into());
    }
    let mut toned = images.to_vec();
    if options.seamless_tones {
        for k in 1..images.len() {
            let mut sums = [[0_f64; 3]; 2];
            let mut count = 0;
            for (i, covered) in coverage[k].iter().enumerate() {
                if *covered && let Some(j) = (0..k).find(|j| coverage[*j][i]) {
                    for (sum, pixel) in sums
                        .iter_mut()
                        .zip([toned[j].pixels[i], images[k].pixels[i]])
                    {
                        for (value, p) in sum.iter_mut().zip(pixel) {
                            *value += p as f64;
                        }
                    }
                    count += 1;
                }
            }
            if count > 0 {
                let gain: [f32; 3] = std::array::from_fn(|c| {
                    if sums[1][c].abs() > 1e-10 {
                        (sums[0][c] / sums[1][c]).clamp(0.125, 8.) as f32
                    } else {
                        1.
                    }
                });
                for p in &mut toned[k].pixels {
                    for c in 0..3 {
                        p[c] *= gain[c];
                    }
                }
            }
        }
    }
    let scores: Vec<_> = if options.mode == BlendMode::StackImages {
        toned
            .iter()
            .zip(coverage)
            .map(|(im, c)| focus(im, c, options.pyramid_levels))
            .collect()
    } else {
        Vec::new()
    };
    let mut masks = vec![vec![0.; n]; images.len()];
    let mut union = vec![false; n];
    for i in 0..n {
        let mut best: Option<usize> = None;
        for k in 0..images.len() {
            if coverage[k][i]
                && best.is_none_or(|old| !scores.is_empty() && scores[k][i] > scores[old][i])
            {
                best = Some(k)
            }
        }
        if let Some(k) = best {
            masks[k][i] = 1.;
            union[i] = true;
        }
    }
    if options.mode == BlendMode::Panorama {
        masks = crate::layer_cut::masks(&toned, coverage);
    }
    let mut image = images[0].clone();
    image.pixels.fill([0.; 3]);
    for (im, mask) in toned.iter().zip(&masks) {
        for (i, p) in im.pixels.iter().enumerate() {
            if mask[i] > 0. {
                image.pixels[i] = *p;
            }
        }
    }
    if options.mode == BlendMode::Panorama {
        let mut blender = crate::blend::Blender::new(w, h, options.pyramid_levels);
        for ((im, mask), covered) in toned.iter().zip(&masks).zip(coverage) {
            blender.add_covered(
                im.pixels.clone(),
                mask.iter().map(|v| [*v]).collect(),
                covered,
            );
        }
        image.pixels = blender.finish();
        for (p, c) in image.pixels.iter_mut().zip(&union) {
            if !c {
                *p = [0.; 3];
            }
        }
    }
    let corrections = images
        .iter()
        .enumerate()
        .map(|(k, im)| {
            im.pixels
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    std::array::from_fn(|c| {
                        if masks[k][i] > 0. {
                            image.pixels[i][c] - p[c]
                        } else {
                            0.
                        }
                    })
                })
                .collect()
        })
        .collect();
    let fill_mask = union
        .iter()
        .map(|v| options.fill_transparent && !v)
        .collect();
    Ok(BlendedLayers {
        masks,
        corrections,
        image,
        coverage: union,
        fill_mask,
    })
}
