//! Magic wand: tolerance flood fill.

use std::collections::VecDeque;

use crate::filter::box_mean;
use crate::mask::{Image, Mask};

/// Magic-wand settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct WandOptions {
    /// Per-channel tolerance in 8-bit levels (`0..=255`).
    pub tolerance: f32,
    /// Only pixels connected to the seed (4-connected).
    pub contiguous: bool,
    /// Soften the edge by one pixel.
    pub antialias: bool,
    /// Seed colour averaging window (1 = point sample, 3 = 3×3, …).
    pub sample_size: u32,
}

impl Default for WandOptions {
    fn default() -> Self {
        Self {
            tolerance: 32.0,
            contiguous: true,
            antialias: true,
            sample_size: 1,
        }
    }
}

/// Selects pixels whose RGBA differs from the seed colour by at most the
/// tolerance in every channel.
pub fn magic_wand(img: &Image, seed: (u32, u32), o: &WandOptions) -> Mask {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut m = Mask::new(img.width, img.height);
    if w == 0 || h == 0 || seed.0 as usize >= w || seed.1 as usize >= h {
        return m;
    }
    let r = i64::from(o.sample_size.max(1) / 2);
    let mut s = [0.0f32; 4];
    let mut n = 0.0;
    for dy in -r..=r {
        for dx in -r..=r {
            let p = img.get(i64::from(seed.0) + dx, i64::from(seed.1) + dy);
            for c in 0..4 {
                s[c] += p[c];
            }
            n += 1.0;
        }
    }
    s.iter_mut().for_each(|v| *v /= n);
    let tol = o.tolerance.max(0.0) / 255.0 + 1e-6;
    let ok = |i: usize| {
        let p = img.data[i];
        (0..4).all(|c| (p[c] - s[c]).abs() <= tol)
    };
    let data = m.data_mut();
    if o.contiguous {
        let start = seed.1 as usize * w + seed.0 as usize;
        let mut seen = vec![false; w * h];
        let mut q = VecDeque::from([start]);
        seen[start] = true;
        while let Some(i) = q.pop_front() {
            if !ok(i) {
                continue;
            }
            data[i] = 1.0;
            let (x, y) = (i % w, i / w);
            let mut push = |j: usize| {
                if !seen[j] {
                    seen[j] = true;
                    q.push_back(j);
                }
            };
            if x > 0 {
                push(i - 1);
            }
            if x + 1 < w {
                push(i + 1);
            }
            if y > 0 {
                push(i - w);
            }
            if y + 1 < h {
                push(i + w);
            }
        }
    } else {
        for (i, v) in data.iter_mut().enumerate() {
            *v = f32::from(u8::from(ok(i)));
        }
    }
    if o.antialias {
        let b = box_mean(m.data(), w, h, 1);
        for (v, bb) in m.data_mut().iter_mut().zip(b) {
            *v = 0.5 * *v + 0.5 * bb;
        }
    }
    m
}
