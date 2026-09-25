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
    /// Reserved cat-eye strength. Only zero is supported; nonzero returns an error.
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
/// Supported bokeh IDs are `circle`/`disc`, `hexagon`, and `octagon`.
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
        || options.cat_eye != 0.
        || !matches!(
            settings.bokeh.as_str(),
            "circle" | "disc" | "hexagon" | "octagon"
        )
    {
        return Err(engine_api::EngineError::invalid(
            "lens_blur",
            "invalid RGB, depth, settings or options",
        ));
    }
    if settings.amount == 0. || options.max_radius == 0. {
        return Ok(image.clone());
    }
    let w = image.width() as usize;
    let h = image.height() as usize;
    let n = w * h;
    let distance = |d: f32| {
        (settings.focus_range[0] - d)
            .max(d - settings.focus_range[1])
            .max(0.)
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
        let r = radius.ceil() as i32;
        let mut kernel = Vec::new();
        for dy in -r..=r {
            for dx in -r..=r {
                let blades = match settings.bokeh.as_str() {
                    "hexagon" => 6,
                    "octagon" => 8,
                    _ => 0,
                };
                let inside = if blades == 0 {
                    (dx as f32).hypot(dy as f32) <= radius
                } else {
                    let apothem = radius * (std::f32::consts::PI / blades as f32).cos();
                    (0..blades).all(|b| {
                        let angle = std::f32::consts::TAU * b as f32 / blades as f32;
                        dx as f32 * angle.cos() + dy as f32 * angle.sin() <= apothem + 1e-6
                    })
                };
                if inside {
                    kernel.push((dx, dy));
                }
            }
        }
        for i in 0..n {
            if membership[i].is_none() {
                continue;
            }
            let mut sum = [0_f64; 3];
            let mut count = 0;
            let mut support = 0;
            for &(dx, dy) in &kernel {
                let x = (i % w) as i64 + i64::from(dx);
                let y = (i / w) as i64 + i64::from(dy);
                if x < 0 || y < 0 || x >= w as i64 || y >= h as i64 {
                    continue;
                }
                support += 1;
                let j = y as usize * w + x as usize;
                if membership[j] != Some(layer) {
                    continue;
                }
                count += 1;
                let y = 0.2627 * image.planes()[0][j] as f64
                    + 0.6780 * image.planes()[1][j] as f64
                    + 0.0593 * image.planes()[2][j] as f64;
                let gain = if y > 1. {
                    1. + options.boost as f64 / 100. * (1. - 1. / y)
                } else {
                    1.
                };
                for (c, s) in sum.iter_mut().enumerate() {
                    *s += image.planes()[c][j] as f64 * gain;
                }
            }
            if count == 0 {
                continue;
            }
            // Normalize color by occupied taps, coverage by the in-image kernel.
            // Over-composite premultiplied color back-to-front, not a flat gather.
            let a = count as f64 / support as f64;
            for (c, s) in sum.iter().enumerate() {
                colors[i][c] = s / count as f64 * a + colors[i][c] * (1. - a);
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
