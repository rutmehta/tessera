//! Scalar filter kernels. Spatial neighbourhoods clamp only at the canvas edge.
use crate::{Buffer, Effect, FilterParams, checkpoint, convolve, distort, large};
use engine_api::{
    EngineError, EngineResult,
    recipe::settings::{DetailSettings, LensBlur},
};
use std::sync::atomic::AtomicBool;

pub(crate) fn run(
    effect: Effect,
    src: &Buffer,
    p: &FilterParams,
    cancel: &AtomicBool,
) -> EngineResult<Buffer> {
    let mut out = src.clone();
    match effect {
        Effect::Gaussian => return large::gaussian(src, p.radius, cancel),
        Effect::Box => {
            let n = 2 * p.radius.ceil() as usize + 1;
            return convolve(src, &vec![1.0 / n as f32; n], cancel);
        }
        Effect::UnsharpMask | Effect::HighPass => {
            if p.radius == 0.0 {
                return Ok(out);
            }
            let blur = large::gaussian(src, p.radius, cancel)?;
            for (i, v) in out.pixels.iter_mut().enumerate() {
                for (c, value) in v.iter_mut().enumerate().take(3) {
                    let d = src.pixels[i][c] - blur.pixels[i][c];
                    *value = if effect == Effect::HighPass {
                        0.5 + d
                    } else {
                        *value
                            + if d.abs() >= p.threshold {
                                p.strength * d
                            } else {
                                0.0
                            }
                    };
                }
            }
        }
        Effect::Adjust => p.adjust.apply(&mut out.pixels)?,
        Effect::Distort(kind) => {
            out.pixels = distort::apply(kind, &p.distort, &src.pixels, src.w, src.h, cancel)?
        }
        Effect::SmartSharpen | Effect::ReduceNoise => {
            return detail(src, p, effect == Effect::SmartSharpen, cancel);
        }
        Effect::LensBlur => {
            let depth = p.depth.as_ref().ok_or_else(|| {
                EngineError::invalid("lens blur", "supply near-to-far depth/mask")
            })?;
            let image = to_image(src)?;
            let image = pipeline_cpu::lens_blur(
                &image,
                depth,
                &LensBlur {
                    amount: 100.0,
                    focus_range: p.focus,
                    ..Default::default()
                },
                pipeline_cpu::LensBlurOptions {
                    max_radius: p.radius,
                    ..Default::default()
                },
            )?;
            for (i, pixel) in out.pixels.iter_mut().enumerate() {
                for (c, v) in pixel.iter_mut().enumerate().take(3) {
                    *v = image.planes()[c][i];
                }
            }
        }
        Effect::OilPaint | Effect::LensFlare => {
            return Err(EngineError::invalid(
                "filters",
                "explicit placeholder: not implemented",
            ));
        }
        Effect::CameraRaw => {
            return Err(EngineError::invalid(
                "camera raw",
                "requires an injected CameraRawProcessor",
            ));
        }
        _ => {
            let r = p.radius.ceil() as i32;
            let mut values = Vec::new();
            for y in 0..src.h {
                checkpoint(cancel)?;
                for x in 0..src.w {
                    let i = y * src.w + x;
                    let a = src.pixels[i];
                    let v = &mut out.pixels[i];
                    match effect {
                        Effect::Motion | Effect::RadialSpin | Effect::RadialZoom => {
                            let n = (2 * r + 1).max(1);
                            let mut sum = [0.0; 4];
                            for j in 0..n {
                                let t = if n == 1 {
                                    0.0
                                } else {
                                    j as f32 / (n - 1) as f32 * 2.0 - 1.0
                                };
                                let cx = (src.w - 1) as f32 * 0.5;
                                let cy = (src.h - 1) as f32 * 0.5;
                                let dx = x as f32 - cx;
                                let dy = y as f32 - cy;
                                let (sx, sy) = match effect {
                                    Effect::Motion => (
                                        x as f32 + t * p.radius * p.angle.cos(),
                                        y as f32 + t * p.radius * p.angle.sin(),
                                    ),
                                    Effect::RadialSpin => {
                                        let (s, c) = (t * p.angle).sin_cos();
                                        (cx + c * dx - s * dy, cy + s * dx + c * dy)
                                    }
                                    _ => (
                                        cx + dx * (1.0 + t * p.angle),
                                        cy + dy * (1.0 + t * p.angle),
                                    ),
                                };
                                let q = distort::sample(&src.pixels, src.w, src.h, sx, sy, false);
                                for c in 0..4 {
                                    sum[c] += q[c] / n as f32;
                                }
                            }
                            *v = sum;
                        }
                        Effect::SurfaceBlur => {
                            if r == 0 || p.threshold == 0.0 {
                                continue;
                            }
                            let mut sum = [0.0; 4];
                            let mut total = 0.0;
                            for dy in -r..=r {
                                for dx in -r..=r {
                                    let q = src.at(x as i32 + dx, y as i32 + dy);
                                    let delta = (0..3).map(|c| (q[c] - a[c]).powi(2)).sum::<f32>();
                                    let weight = (-0.5
                                        * ((dx * dx + dy * dy) as f32 / p.radius.powi(2)
                                            + delta / p.threshold.powi(2)))
                                    .exp();
                                    total += weight;
                                    for c in 0..4 {
                                        sum[c] += q[c] * weight;
                                    }
                                }
                            }
                            *v = sum.map(|s| s / total);
                        }
                        Effect::Median | Effect::DustScratches => {
                            for c in 0..3 {
                                values.clear();
                                for dy in -r..=r {
                                    for dx in -r..=r {
                                        values.push(src.at(x as i32 + dx, y as i32 + dy)[c]);
                                    }
                                }
                                let mid = values.len() / 2;
                                values.select_nth_unstable_by(mid, f32::total_cmp);
                                let med = values[mid];
                                if effect == Effect::Median || (med - a[c]).abs() > p.threshold {
                                    v[c] = med;
                                }
                            }
                        }
                        Effect::AddNoise => {
                            for c in 0..3 {
                                v[c] = a[c]
                                    + p.strength
                                        * noise(
                                            x as u32,
                                            y as u32,
                                            if p.monochrome { 0 } else { c as u32 },
                                            p.seed,
                                            p.gaussian_noise,
                                        );
                            }
                        }
                        Effect::Emboss | Effect::FindEdges => {
                            for (c, value) in v.iter_mut().enumerate().take(3) {
                                if effect == Effect::Emboss {
                                    *value = 0.5
                                        + p.strength
                                            * (src.at(x as i32 + 1, y as i32 + 1)[c]
                                                - src.at(x as i32 - 1, y as i32 - 1)[c]);
                                } else {
                                    let mut gx = 0.0;
                                    let mut gy = 0.0;
                                    for dy in -1..=1 {
                                        for dx in -1..=1 {
                                            let q = src.at(x as i32 + dx, y as i32 + dy)[c];
                                            gx += q * dx as f32 * if dy == 0 { 2.0 } else { 1.0 };
                                            gy += q * dy as f32 * if dx == 0 { 2.0 } else { 1.0 };
                                        }
                                    }
                                    *value = gx.hypot(gy) / 4.0;
                                }
                            }
                        }
                        Effect::Solarize => {
                            for value in &mut v[..3] {
                                if *value >= p.threshold {
                                    *value = 1.0 - *value;
                                }
                            }
                        }
                        Effect::Clouds | Effect::DifferenceClouds => {
                            let scale = p.radius.max(1.0);
                            let mut value = 0.0;
                            let mut weight = 0.5;
                            for octave in 0..5 {
                                let f = (1u32 << octave) as f32 / scale;
                                value += weight * perlin(x as f32 * f, y as f32 * f, p.seed);
                                weight *= 0.5;
                            }
                            let value = 0.5 + 0.5 * value;
                            for c in 0..3 {
                                v[c] = if effect == Effect::Clouds {
                                    value
                                } else {
                                    (a[c] - value).abs()
                                };
                            }
                        }
                        _ => unreachable!(),
                    }
                }
            }
        }
    }
    checkpoint(cancel)?;
    Ok(out)
}

pub(crate) fn hash(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846ca68b);
    v ^ (v >> 16)
}
pub(crate) fn noise(x: u32, y: u32, c: u32, seed: u32, gaussian: bool) -> f32 {
    let key =
        x.wrapping_mul(0x9e3779b9) ^ y.wrapping_mul(0x85ebca6b) ^ c.wrapping_mul(0xc2b2ae35) ^ seed;
    let u = ((hash(key) >> 8) as f32 + 0.5) / 16777216.0;
    if gaussian {
        let v = ((hash(key ^ 0xa511e9b3) >> 8) as f32 + 0.5) / 16777216.0;
        (-2.0 * u.ln()).sqrt() * (std::f32::consts::TAU * v).cos()
    } else {
        2.0 * u - 1.0
    }
}
fn perlin(x: f32, y: f32, seed: u32) -> f32 {
    let ix = x.floor() as i32;
    let iy = y.floor() as i32;
    let fx = x - x.floor();
    let fy = y - y.floor();
    let gradient = |dx: i32, dy: i32| {
        let h = hash((ix + dx) as u32 ^ ((iy + dy) as u32).wrapping_mul(0x9e3779b9) ^ seed) & 7;
        let gx = fx - dx as f32;
        let gy = fy - dy as f32;
        match h {
            0 => gx,
            1 => -gx,
            2 => gy,
            3 => -gy,
            4 => (gx + gy) * std::f32::consts::FRAC_1_SQRT_2,
            5 => (gx - gy) * std::f32::consts::FRAC_1_SQRT_2,
            6 => (-gx + gy) * std::f32::consts::FRAC_1_SQRT_2,
            _ => (-gx - gy) * std::f32::consts::FRAC_1_SQRT_2,
        }
    };
    let fade = |t: f32| t * t * t * (t * (t * 6.0 - 15.0) + 10.0);
    let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
    lerp(
        lerp(gradient(0, 0), gradient(1, 0), fade(fx)),
        lerp(gradient(0, 1), gradient(1, 1), fade(fx)),
        fade(fy),
    )
}
fn to_image(src: &Buffer) -> EngineResult<pipeline_cpu::Image> {
    pipeline_cpu::Image::new(
        src.w as u32,
        src.h as u32,
        (0..3)
            .map(|c| src.pixels.iter().map(|p| p[c]).collect())
            .collect(),
    )
}
fn detail(
    src: &Buffer,
    p: &FilterParams,
    sharpen: bool,
    cancel: &AtomicBool,
) -> EngineResult<Buffer> {
    let mut settings = DetailSettings::default();
    settings.sharpening.amount = if sharpen {
        (100.0 * p.strength).min(150.0)
    } else {
        0.0
    };
    settings.sharpening.radius = p.radius.clamp(0.5, 3.0);
    settings.sharpening.detail = 100.0;
    settings.noise_reduction.luminance = if sharpen {
        0.0
    } else {
        (100.0 * p.strength).min(100.0)
    };
    settings.noise_reduction.color = settings.noise_reduction.luminance;
    let image = to_image(src)?;
    let halo = pipeline_cpu::detail_halo(&settings);
    let mut out = src.clone();
    for coord in image.coords() {
        checkpoint(cancel)?;
        let mut tile = image.tile(coord, halo, 1)?;
        pipeline_cpu::detail(&mut tile, &settings)?;
        let l = tile.layout();
        let data = tile.samples::<f32>()?;
        for y in 0..l.extent.height {
            for x in 0..l.extent.width {
                let dst = &mut out.pixels[(coord.y as usize * 256 + y as usize) * src.w
                    + coord.x as usize * 256
                    + x as usize];
                for (c, value) in dst.iter_mut().enumerate().take(3) {
                    *value = data[l.index(c as u8, x as i32, y as i32).unwrap()];
                }
            }
        }
    }
    Ok(out)
}
