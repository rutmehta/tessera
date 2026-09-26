//! Quick selection: region growing from brush strokes on colour and
//! texture similarity.
//!
//! Features per pixel are CIE Lab (scaled to ~unit range) and the local
//! standard deviation of luminance (5×5), the texture cue. The seed pixels
//! under the stroke define a per-feature mean and spread; the region grows
//! 4-connected through pixels whose worst normalized feature distance is
//! within `threshold`. A small closing removes pinholes left by noise.

use std::collections::VecDeque;

use crate::filter::box_mean;
use crate::mask::{Image, Mask};
use crate::ops;
use crate::range::srgb_to_lab;

/// Quick-selection settings.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct QuickOptions {
    /// Brush radius, pixels.
    pub radius: f32,
    /// Growth threshold in seed standard deviations.
    pub threshold: f32,
    /// Weight of the texture feature (0 = colour only).
    pub texture_weight: f32,
    /// Closing radius applied to the grown region, pixels.
    pub close: f32,
}

impl Default for QuickOptions {
    fn default() -> Self {
        Self {
            radius: 10.0,
            threshold: 4.0,
            texture_weight: 1.0,
            close: 1.5,
        }
    }
}

const NF: usize = 4;

fn features(img: &Image) -> Vec<[f32; NF]> {
    let (w, h) = (img.width as usize, img.height as usize);
    let l = img.luminance();
    let l2: Vec<f32> = l.iter().map(|v| v * v).collect();
    let (m, m2) = (box_mean(&l, w, h, 2), box_mean(&l2, w, h, 2));
    img.data
        .iter()
        .enumerate()
        .map(|(i, p)| {
            let lab = srgb_to_lab([p[0], p[1], p[2]]);
            [
                lab[0] / 100.0,
                lab[1] / 100.0,
                lab[2] / 100.0,
                (m2[i] - m[i] * m[i]).max(0.0).sqrt(),
            ]
        })
        .collect()
}

/// Pixels within `radius` of the polyline `stroke`.
fn seeds(w: usize, h: usize, stroke: &[[f32; 2]], radius: f32) -> Vec<usize> {
    let mut hit = vec![false; w * h];
    let segs: Vec<([f32; 2], [f32; 2])> = if stroke.len() == 1 {
        vec![(stroke[0], stroke[0])]
    } else {
        stroke.windows(2).map(|s| (s[0], s[1])).collect()
    };
    for (a, b) in segs {
        let x0 = (a[0].min(b[0]) - radius).floor().max(0.0) as usize;
        let y0 = (a[1].min(b[1]) - radius).floor().max(0.0) as usize;
        let x1 = ((a[0].max(b[0]) + radius).ceil().max(0.0) as usize).min(w);
        let y1 = ((a[1].max(b[1]) + radius).ceil().max(0.0) as usize).min(h);
        let (dx, dy) = (b[0] - a[0], b[1] - a[1]);
        let len2 = dx * dx + dy * dy;
        for y in y0..y1 {
            for x in x0..x1 {
                let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
                let t = if len2 > 0.0 {
                    (((px - a[0]) * dx + (py - a[1]) * dy) / len2).clamp(0.0, 1.0)
                } else {
                    0.0
                };
                let (cx, cy) = (a[0] + t * dx, a[1] + t * dy);
                if (px - cx).hypot(py - cy) <= radius {
                    hit[y * w + x] = true;
                }
            }
        }
    }
    hit.iter()
        .enumerate()
        .filter(|(_, h)| **h)
        .map(|(i, _)| i)
        .collect()
}

/// Grows a selection from `stroke`; the result is added to (or, with
/// `subtract`, removed from) `previous`.
pub fn quick_select(
    img: &Image,
    stroke: &[[f32; 2]],
    o: &QuickOptions,
    previous: Option<&Mask>,
    subtract: bool,
) -> Mask {
    let (w, h) = (img.width as usize, img.height as usize);
    let mut grown = Mask::new(img.width, img.height);
    let seed = if stroke.is_empty() {
        Vec::new()
    } else {
        seeds(w, h, stroke, o.radius.max(0.5))
    };
    if !seed.is_empty() {
        let f = features(img);
        let n = seed.len() as f32;
        let mut mean = [0.0f32; NF];
        for &i in &seed {
            for k in 0..NF {
                mean[k] += f[i][k] / n;
            }
        }
        let mut sd = [0.0f32; NF];
        for &i in &seed {
            for k in 0..NF {
                sd[k] += (f[i][k] - mean[k]).powi(2) / n;
            }
        }
        let floor = [0.02, 0.02, 0.02, 0.01];
        let scale: [f32; NF] = std::array::from_fn(|k| sd[k].sqrt().max(floor[k]));
        let weight = [1.0, 1.0, 1.0, o.texture_weight.max(0.0)];
        let dist = |p: &[f32; NF]| {
            (0..NF)
                .map(|k| weight[k] * (p[k] - mean[k]).abs() / scale[k])
                .fold(0.0f32, f32::max)
        };
        let data = grown.data_mut();
        let mut seen = vec![false; w * h];
        let mut q: VecDeque<usize> = VecDeque::new();
        for &i in &seed {
            seen[i] = true;
            data[i] = 1.0;
            q.push_back(i);
        }
        while let Some(i) = q.pop_front() {
            let (x, y) = (i % w, i / w);
            let nb = [
                (x > 0).then(|| i - 1),
                (x + 1 < w).then(|| i + 1),
                (y > 0).then(|| i - w),
                (y + 1 < h).then(|| i + w),
            ];
            for j in nb.into_iter().flatten() {
                if seen[j] {
                    continue;
                }
                seen[j] = true;
                if dist(&f[j]) <= o.threshold {
                    data[j] = 1.0;
                    q.push_back(j);
                }
            }
        }
        if o.close > 0.0 {
            grown = ops::contract(&ops::grow(&grown, o.close), o.close);
        }
    }
    match previous {
        Some(p) if p.extent() == grown.extent() => {
            let op = if subtract {
                ops::Combine::Subtract
            } else {
                ops::Combine::Add
            };
            ops::combine(p, &grown, op).unwrap_or(grown)
        }
        _ if subtract => Mask::new(img.width, img.height),
        _ => grown,
    }
}
