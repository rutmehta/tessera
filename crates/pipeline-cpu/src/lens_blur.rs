//! Full-image, normalized depth-layer reference blur (not a tile-local filter).
use crate::Image;
use engine_api::{EngineResult, recipe::settings::LensBlur};

/// Runtime controls absent from the recipe. Radius is in input-image pixels.
#[derive(Clone, Copy, Debug)]
pub struct LensBlurOptions {
    /// Maximum aperture circumradius, finite 0..=128; default 16 pixels.
    pub max_radius: f32,
    /// Uniform near-to-far depth bins, 2..=64; default 16.
    pub layers: usize,
    /// Specular gain, 0..=100 percent, applied above linear luminance 1.
    pub boost: f32,
    /// Radial cat-eye clipping strength, finite 0..=1. Zero disables clipping.
    pub cat_eye: f32,
}
impl Default for LensBlurOptions {
    fn default() -> Self {
        Self {
            max_radius: 16.,
            layers: 16,
            boost: 0.,
            cat_eye: 0.,
        }
    }
}

/// Blur scene-linear Rec.2020 RGB using same-size row-major near=0, far=1 depth.
///
/// `focus_range` is inclusive: its pixels retain their exact input bits. Amount
/// zero (or maximum radius zero) is an exact clone, after input validation.
/// Supported IDs: `circle`/`disc`, `bubble`, `5-blade`/`five-blade`/`pentagon`,
/// `ring`, `cat-eye`/`cat_eye`/`cat eye`, `oval`/`anamorphic`, `hexagon`, `octagon`.
/// `depth_model` is provenance only; this function never invokes inference.
///
/// Layers composite far-to-near using normalized masked colors and coverage;
/// final coverage normalization avoids black fringes at incomplete layer support.
/// The full image is required, not independently blurred tiles. This scalar
/// reference costs O(pixels * occupied_layers * radius²), with O(pixels) scratch.
/// See LENS_BLUR_M3.md for approximation limits and the radius/boost formulas.
pub fn lens_blur(
    image: &Image,
    depth: &[f32],
    settings: &LensBlur,
    options: LensBlurOptions,
) -> EngineResult<Image> {
    let bounded = |v: f32, lo: f32, hi: f32| v.is_finite() && (lo..=hi).contains(&v);
    if image.planes().len() != 3
        || image.planes().iter().flatten().any(|v| !v.is_finite())
        || depth.len() != image.width() as usize * image.height() as usize
        || depth.iter().any(|&d| !bounded(d, 0., 1.))
        || !bounded(settings.amount, 0., 100.)
        || !bounded(settings.focus_range[0], 0., 1.)
        || !bounded(settings.focus_range[1], settings.focus_range[0], 1.)
        || !bounded(options.max_radius, 0., 128.)
        || !(2..=64).contains(&options.layers)
        || !bounded(options.boost, 0., 100.)
        || !bounded(options.cat_eye, 0., 1.)
        || !matches!(
            settings.bokeh.as_str(),
            "circle"
                | "disc"
                | "hexagon"
                | "octagon"
                | "bubble"
                | "5-blade"
                | "five-blade"
                | "pentagon"
                | "ring"
                | "cat-eye"
                | "cat_eye"
                | "cat eye"
                | "oval"
                | "anamorphic"
        )
    {
        return Err(engine_api::EngineError::invalid(
            "lens_blur",
            "invalid RGB, depth, settings or options",
        ));
    }
    let adobe = settings.adobe.as_ref();
    let control = |get: fn(&engine_api::recipe::settings::AdobeLensBlur) -> Option<f32>,
                   default: f32| adobe.and_then(get).unwrap_or(default);
    let numbers = [
        control(|a| a.highlights_boost, 0.),
        control(|a| a.highlights_threshold, 100.),
        control(|a| a.cat_eye_amount, 0.),
        control(|a| a.cat_eye_scale, 100.),
        control(|a| a.bokeh_aspect, 0.),
        control(|a| a.bokeh_rotation, 0.),
        control(|a| a.bokeh_shape_detail, 0.),
        control(|a| a.spherical_aberration, 0.),
        control(|a| a.bokeh_shape, 0.),
        control(|a| a.focal_range_source, 0.),
    ];
    if numbers.iter().any(|n| !n.is_finite())
        || adobe.and_then(|a| a.focal_range).is_some_and(|range| {
            range.iter().any(|n| !bounded(*n, 0., 1.)) || range.windows(2).any(|w| w[0] > w[1])
        })
    {
        return Err(engine_api::EngineError::invalid(
            "lens_blur",
            "invalid approximate controls",
        ));
    }
    let boost = control(|a| a.highlights_boost, options.boost).clamp(0., 100.);
    let threshold = control(|a| a.highlights_threshold, 100.).clamp(0., 100.) / 100.;
    let cat_eye_control = (control(|a| a.cat_eye_amount, options.cat_eye * 100.) / 100.
        * control(|a| a.cat_eye_scale, 100.)
        / 100.)
        .clamp(0., 1.);
    let aspect = 2_f32.powf(control(|a| a.bokeh_aspect, 0.).clamp(-100., 100.) / 100.);
    let angle = control(|a| a.bokeh_rotation, 0.).to_radians();
    let detail = control(|a| a.bokeh_shape_detail, 0.).clamp(0., 100.) / 100.;
    let aberration = control(|a| a.spherical_aberration, 0.).clamp(-100., 100.) / 100.;
    if adobe.is_some_and(|a| a.active == Some(false)) {
        return Ok(image.clone());
    }
    if settings.amount == 0. || options.max_radius == 0. {
        return Ok(image.clone());
    }
    let w = image.width() as usize;
    let h = image.height() as usize;
    let n = w * h;
    let distance = |d: f32| {
        if let Some([a, b, c, e]) = adobe.and_then(|a| a.focal_range) {
            if d < b {
                ((b - d) / (b - a).max(f32::EPSILON)).min(1.)
            } else if d > c {
                ((d - c) / (e - c).max(f32::EPSILON)).min(1.)
            } else {
                0.
            }
        } else {
            (settings.focus_range[0] - d)
                .max(d - settings.focus_range[1])
                .max(0.)
        }
    };
    let membership: Vec<_> = depth
        .iter()
        .map(|&d| {
            if distance(d) == 0. {
                None
            } else {
                Some(((d * options.layers as f32) as usize).min(options.layers - 1))
            }
        })
        .collect();
    // Focus samples never enter a blurred layer, preventing sharp foreground
    // color from contaminating the background. Final output copies them exactly.
    // f64 sums keep all finite f32/HDR inputs safe, including boosted highlights.
    let mut colors = vec![[0_f64; 3]; n];
    let mut alpha = vec![0_f64; n];
    for layer in (0..options.layers).rev() {
        let members: Vec<_> = (0..n).filter(|&i| membership[i] == Some(layer)).collect();
        if members.is_empty() {
            continue;
        }
        let radius = options.max_radius * settings.amount / 100.
            * (members
                .iter()
                .map(|&i| distance(depth[i]) as f64)
                .sum::<f64>()
                / members.len() as f64) as f32;
        let r = (radius * aspect.max(1. / aspect)).ceil() as i32;
        let mut kernel = Vec::new();
        for dy in -r..=r {
            for dx in -r..=r {
                let blades = match settings.bokeh.as_str() {
                    "5-blade" | "five-blade" | "pentagon" => 5,
                    "hexagon" => 6,
                    "octagon" => 8,
                    _ => 0,
                };
                let px = (dx as f32 * angle.cos() + dy as f32 * angle.sin()) / aspect;
                let py = (-dx as f32 * angle.sin() + dy as f32 * angle.cos()) * aspect;
                let radial = px.hypot(py);
                let inside = if matches!(settings.bokeh.as_str(), "oval" | "anamorphic") {
                    (px * 2.).hypot(py) <= radius
                } else if blades == 0 {
                    radial <= radius
                        && (settings.bokeh != "ring" || radial >= radius * 0.7 || radius < 1.)
                } else {
                    let apothem = radius * (std::f32::consts::PI / blades as f32).cos();
                    (0..blades).all(|b| {
                        let angle = std::f32::consts::TAU * b as f32 / blades as f32;
                        px * angle.cos() + py * angle.sin() <= apothem + 1e-6
                    })
                };
                if inside {
                    let weight = if settings.bokeh == "bubble" && radial < radius * 0.75 {
                        0.2
                    } else {
                        1.
                    };
                    let rho = (radial / radius.max(f32::EPSILON)).clamp(0., 1.) as f64;
                    let weight = weight
                        * (1. + detail as f64 * rho + aberration as f64 * (2. * rho - 1.))
                            .max(0.01);
                    kernel.push((dx, dy, weight));
                }
            }
        }
        for i in 0..n {
            if membership[i].is_none() {
                continue;
            }
            let mut sum = [0_f64; 3];
            let mut count = 0.;
            let mut support = 0.;
            // The optical axis has a circular pupil; clipping increases toward
            // corners and is oriented radially, producing the cat-eye outline.
            let cat_eye = if matches!(settings.bokeh.as_str(), "cat-eye" | "cat_eye" | "cat eye") {
                cat_eye_control.max(0.75)
            } else {
                cat_eye_control
            };
            let nx = if w > 1 {
                2. * (i % w) as f32 / (w - 1) as f32 - 1.
            } else {
                0.
            };
            let ny = if h > 1 {
                2. * (i / w) as f32 / (h - 1) as f32 - 1.
            } else {
                0.
            };
            let shift = cat_eye * radius * 0.5;
            for &(dx, dy, weight) in &kernel {
                if cat_eye > 0.
                    && (dx != 0 || dy != 0)
                    && ((dx as f32 + nx * shift).hypot(dy as f32 + ny * shift) > radius
                        || (dx as f32 - nx * shift).hypot(dy as f32 - ny * shift) > radius)
                {
                    continue;
                }
                let x = (i % w) as i64 + i64::from(dx);
                let y = (i / w) as i64 + i64::from(dy);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    continue;
                }
                support += weight;
                let j = y as usize * w + x as usize;
                if membership[j] != Some(layer) {
                    continue;
                }
                count += weight;
                let y = 0.2627 * image.planes()[0][j] as f64
                    + 0.6780 * image.planes()[1][j] as f64
                    + 0.0593 * image.planes()[2][j] as f64;
                let gain = if y > threshold as f64 {
                    1. + boost as f64 / 100. * (1. - threshold as f64 / y)
                } else {
                    1.
                };
                for (c, s) in sum.iter_mut().enumerate() {
                    *s += image.planes()[c][j] as f64 * gain * weight;
                }
            }
            if count == 0. {
                continue;
            }
            // Normalize color by occupied taps, coverage by the in-image kernel.
            // Over-composite premultiplied color back-to-front, not a flat gather.
            let a = count / support;
            for (c, s) in sum.iter().enumerate() {
                colors[i][c] = s / count * a + colors[i][c] * (1. - a);
            }
            alpha[i] = a + alpha[i] * (1. - a);
        }
    }
    let planes = image
        .planes()
        .iter()
        .enumerate()
        .map(|(c, p)| {
            p.iter()
                .enumerate()
                .map(|(i, &v)| {
                    if membership[i].is_none() || alpha[i] == 0. {
                        v
                    } else {
                        (colors[i][c] / alpha[i]).clamp(-(f32::MAX as f64), f32::MAX as f64) as f32
                    }
                })
                .collect()
        })
        .collect();
    Image::new(image.width(), image.height(), planes)
}
