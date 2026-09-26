//! Deterministic f32 reconstruction of premultiplied RGBA, with transparent extension.
use crate::Image;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Kernel {
    Nearest,
    Bilinear,
    Bicubic,
    Lanczos3,
    /// Chosen by TransformOp; standalone sampling defaults to Catmull–Rom.
    Automatic,
}

/// Position is a source pixel center: the first texel is at [0.5, 0.5].
/// Missing taps are transparent black, never clamped or renormalized away.
/// Channels are already premultiplied; do not multiply/unmultiply alpha here.
/// Accumulate y-major, then x-major, in f32 without fused multiply-add.
#[inline]
pub fn sample(input: &Image, position: [f32; 2], kernel: Kernel) -> [f32; 4] {
    let [x, y] = [position[0] - 0.5, position[1] - 0.5];
    if !x.is_finite()
        || !y.is_finite()
        || x < -3.
        || y < -3.
        || x > input.width as f32 + 2.
        || y > input.height as f32 + 2.
    {
        return [0.; 4];
    }
    if kernel == Kernel::Nearest {
        return texel(
            input,
            (x + 0.5).floor() as isize,
            (y + 0.5).floor() as isize,
        );
    }
    match kernel {
        Kernel::Bilinear => reconstruct::<2>(input, x, y, Kernel::Bilinear),
        Kernel::Lanczos3 => reconstruct::<6>(input, x, y, Kernel::Lanczos3),
        _ => reconstruct::<4>(input, x, y, Kernel::Bicubic),
    }
}
#[inline]
fn reconstruct<const N: usize>(input: &Image, x: f32, y: f32, kernel: Kernel) -> [f32; 4] {
    let x0 = x.floor() as isize - N as isize / 2 + 1;
    let y0 = y.floor() as isize - N as isize / 2 + 1;
    let mut wx = [0.; N];
    let mut wy = [0.; N];
    for i in 0..N {
        wx[i] = weight(x - (x0 + i as isize) as f32, kernel);
        wy[i] = weight(y - (y0 + i as isize) as f32, kernel);
    }
    // Normalize the complete Lanczos support, including taps outside the image.
    if N == 6 {
        let sx: f32 = wx.iter().sum();
        let sy: f32 = wy.iter().sum();
        for i in 0..N {
            wx[i] /= sx;
            wy[i] /= sy;
        }
    }
    let mut result = [0.; 4];
    for (j, &yv) in wy.iter().enumerate() {
        for (i, &xv) in wx.iter().enumerate() {
            let value = texel(input, x0 + i as isize, y0 + j as isize);
            let w = xv * yv;
            for c in 0..4 {
                result[c] += value[c] * w;
            }
        }
    }
    result
}
#[inline]
fn texel(input: &Image, x: isize, y: isize) -> [f32; 4] {
    if x < 0 || y < 0 || x as usize >= input.width || y as usize >= input.height {
        return [0.; 4];
    }
    let i = y as usize * input.width + x as usize;
    std::array::from_fn(|c| input.planes[c].get(i).copied().unwrap_or(0.))
}
#[inline]
fn weight(x: f32, kernel: Kernel) -> f32 {
    let x = x.abs();
    match kernel {
        Kernel::Nearest => {
            if x < 0.5 {
                1.
            } else {
                0.
            }
        }
        Kernel::Bilinear => (1. - x).max(0.),
        Kernel::Bicubic | Kernel::Automatic => {
            if x < 1. {
                ((1.5 * x - 2.5) * x) * x + 1.
            } else if x < 2. {
                ((-0.5 * x + 2.5) * x - 4.) * x + 2.
            } else {
                0.
            }
        }
        Kernel::Lanczos3 => {
            if x == 0. {
                1.
            } else if x >= 3. {
                0.
            } else {
                let p = std::f32::consts::PI * x;
                (p.sin() / p) * ((p / 3.).sin() / (p / 3.))
            }
        }
    }
}
