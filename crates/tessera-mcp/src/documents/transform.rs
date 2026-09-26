//! Affine resampling of layer rasters (`transform_layer`).
use compositor::{Affine, Raster, Rect, TileDelta};
use engine_api::EngineResult;
use engine_api::document::Interpolation;

use super::dense::{self, Dense};

/// `outer ∘ inner`: apply `inner`, then `outer`.
pub(crate) fn compose(outer: &Affine, inner: &Affine) -> Affine {
    let [a, b, c, d, e, f] = outer.m;
    let [p, q, r, s, t, u] = inner.m;
    Affine {
        m: [
            a * p + b * s,
            a * q + b * t,
            a * r + b * u + c,
            d * p + e * s,
            d * q + e * t,
            d * r + e * u + f,
        ],
    }
}

fn premul(p: [f32; 4], channels: u8) -> [f32; 4] {
    if channels < 4 {
        return p;
    }
    [p[0] * p[3], p[1] * p[3], p[2] * p[3], p[3]]
}

fn unpremul(p: [f32; 4], channels: u8) -> [f32; 4] {
    if channels < 4 {
        return p;
    }
    if p[3] <= 1e-7 {
        return [0.0; 4];
    }
    [p[0] / p[3], p[1] / p[3], p[2] / p[3], p[3].min(1.0)]
}

fn cubic(t: f32) -> [f32; 4] {
    // Catmull-Rom (a = −0.5).
    let a = -0.5f32;
    let w = |x: f32| {
        let x = x.abs();
        if x < 1.0 {
            (a + 2.0) * x * x * x - (a + 3.0) * x * x + 1.0
        } else if x < 2.0 {
            a * x * x * x - 5.0 * a * x * x + 8.0 * a * x - 4.0 * a
        } else {
            0.0
        }
    };
    [w(1.0 + t), w(t), w(1.0 - t), w(2.0 - t)]
}

/// Tile deltas replacing `raster` by its image under `t` (a map from
/// current to new canvas coordinates). Pixels no longer covered read the
/// raster default (transparent content, reveal-all masks).
pub(crate) fn resample(
    raster: &Raster,
    t: &Affine,
    interpolation: Interpolation,
) -> EngineResult<Vec<TileDelta>> {
    let canvas = Rect::of_extent(raster.extent());
    let Some(src_bounds) = raster.bounds() else {
        return Ok(Vec::new());
    };
    let inv = t.inverse().expect("validated invertible");
    let pad = 2;
    let read = dense::read(raster, src_bounds.inflate(pad))?;
    let channels = raster.channels();
    let def = raster.default_value();
    let outside = premul(
        {
            let mut o = [0.0; 4];
            for v in o.iter_mut().take(channels as usize) {
                *v = def;
            }
            o
        },
        channels,
    );
    let src = Dense {
        rect: read.rect,
        px: read.px.iter().map(|p| premul(*p, channels)).collect(),
    };
    let dst_bounds = t.map_rect(&src_bounds).inflate(1).intersect(&canvas);
    let region = src_bounds.union(&dst_bounds).intersect(&canvas);
    let sample = |x: f32, y: f32| -> [f32; 4] {
        match interpolation {
            Interpolation::Nearest => src.get(x.floor() as i64, y.floor() as i64, outside),
            Interpolation::Bilinear => {
                let (fx, fy) = (x - 0.5, y - 0.5);
                let (ix, iy) = (fx.floor(), fy.floor());
                let (tx, ty) = (fx - ix, fy - iy);
                let (ix, iy) = (ix as i64, iy as i64);
                let mut o = [0.0; 4];
                for (dy, wy) in [(0, 1.0 - ty), (1, ty)] {
                    for (dx, wx) in [(0, 1.0 - tx), (1, tx)] {
                        let p = src.get(ix + dx, iy + dy, outside);
                        for c in 0..4 {
                            o[c] += p[c] * wx * wy;
                        }
                    }
                }
                o
            }
            Interpolation::Bicubic => {
                let (fx, fy) = (x - 0.5, y - 0.5);
                let (ix, iy) = (fx.floor(), fy.floor());
                let (wx, wy) = (cubic(fx - ix), cubic(fy - iy));
                let (ix, iy) = (ix as i64, iy as i64);
                let mut o = [0.0; 4];
                for (j, wyj) in wy.iter().enumerate() {
                    for (i, wxi) in wx.iter().enumerate() {
                        let p = src.get(ix + i as i64 - 1, iy + j as i64 - 1, outside);
                        for c in 0..4 {
                            o[c] += p[c] * wxi * wyj;
                        }
                    }
                }
                if channels >= 4 {
                    o[3] = o[3].clamp(0.0, 1.0);
                    for c in 0..3 {
                        o[c] = o[c].clamp(0.0, o[3].max(0.0) * 64.0);
                    }
                }
                o
            }
        }
    };
    dense::deltas(raster, region, |x, y, p| {
        let (cx, cy) = (f64::from(x) + 0.5, f64::from(y) + 0.5);
        let inside = dst_bounds.x0 <= i64::from(x)
            && i64::from(x) < dst_bounds.x1
            && dst_bounds.y0 <= i64::from(y)
            && i64::from(y) < dst_bounds.y1;
        let v = if inside {
            let (sx, sy) = inv.apply(cx, cy);
            sample(sx as f32, sy as f32)
        } else {
            outside
        };
        let v = unpremul(v, channels);
        p[..channels as usize].copy_from_slice(&v[..channels as usize]);
    })
}
