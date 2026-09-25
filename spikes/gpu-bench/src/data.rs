//! Synthetic inputs and scalar f32 CPU reference implementations.
//!
//! The references deliberately mirror the GPU summation order so that the
//! reported error measures backend arithmetic (fast-math, FMA contraction,
//! pow/cbrt precision), not algorithmic differences.

use std::thread;

pub const W: usize = 4096;
pub const H: usize = 4096;
pub const LUT_N: usize = 33;
pub const GF_R: i32 = 8;
pub const GF_EPS: f32 = 1e-3;

pub type Px = [f32; 4];

/// Run `f(y, row)` over all rows of `out` on all cores (scalar code per pixel).
pub fn par_rows<T: Send, F: Fn(usize, &mut [T]) + Sync>(out: &mut [T], width: usize, f: F) {
    let threads = thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(8);
    let rows = out.len() / width;
    let per = rows.div_ceil(threads);
    thread::scope(|s| {
        for (ci, chunk) in out.chunks_mut(per * width).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (ri, row) in chunk.chunks_mut(width).enumerate() {
                    f(ci * per + ri, row);
                }
            });
        }
    });
}

/// Smooth-plus-detail linear RGB test image in (0, 1).
pub fn ground_truth() -> Vec<Px> {
    let mut img = vec![[0.0f32; 4]; W * H];
    par_rows(&mut img, W, |y, row| {
        let yf = y as f64;
        for (x, p) in row.iter_mut().enumerate() {
            let xf = x as f64;
            let detail = 0.04 * (0.9 * xf).sin() * (0.7 * yf).cos();
            let r = 0.5 + 0.40 * (0.011 * xf + 0.003 * yf).sin() + detail;
            let g = 0.5 + 0.40 * (0.007 * xf - 0.013 * yf + 1.0).sin() - detail;
            let b = 0.5 + 0.40 * (0.005 * (xf + yf) + 2.0).cos() + 0.5 * detail;
            *p = [r as f32, g as f32, b as f32, 1.0];
        }
    });
    img
}

/// RGGB mosaic of the ground truth, quantised to u16.
pub fn mosaic(gt: &[Px]) -> Vec<u16> {
    let mut cfa = vec![0u16; W * H];
    par_rows(&mut cfa, W, |y, row| {
        for (x, v) in row.iter_mut().enumerate() {
            let ch = match (y & 1, x & 1) {
                (0, 0) => 0,
                (1, 1) => 2,
                _ => 1,
            };
            let s = gt[y * W + x][ch].clamp(0.0, 1.0);
            *v = (s * 65535.0).round() as u16;
        }
    });
    cfa
}

/// Ground truth plus deterministic hash noise (±0.02), the guided-filter input.
pub fn noisy(gt: &[Px]) -> Vec<Px> {
    let mut out = vec![[0.0f32; 4]; W * H];
    par_rows(&mut out, W, |y, row| {
        for (x, p) in row.iter_mut().enumerate() {
            let src = gt[y * W + x];
            let mut o = src;
            for (c, v) in o.iter_mut().take(3).enumerate() {
                let mut h = (x as u32).wrapping_mul(0x9E37_79B1)
                    ^ (y as u32).wrapping_mul(0x85EB_CA77)
                    ^ (c as u32).wrapping_mul(0xC2B2_AE3D);
                h ^= h >> 15;
                h = h.wrapping_mul(0x2C1B_3C6D);
                h ^= h >> 12;
                let n = (h & 0xffff) as f32 / 65535.0 - 0.5;
                *v += 0.04 * n;
            }
            *p = o;
        }
    });
    out
}

/// 33^3 LUT over Oklab (L in [0,1], a,b in [-0.5,0.5]); layout (b*N + a)*N + L.
/// Creative transform: L^0.9, 10 degree hue rotation, 1.15x chroma.
pub fn make_lut() -> Vec<Px> {
    let n = LUT_N;
    let mut lut = vec![[0.0f32; 4]; n * n * n];
    let th = 10f64.to_radians();
    for k in 0..n {
        for j in 0..n {
            for i in 0..n {
                let l = i as f64 / 32.0;
                let a = j as f64 / 32.0 - 0.5;
                let b = k as f64 / 32.0 - 0.5;
                let l2 = l.powf(0.9);
                let a2 = 1.15 * (a * th.cos() - b * th.sin());
                let b2 = 1.15 * (a * th.sin() + b * th.cos());
                lut[(k * n + j) * n + i] = [l2 as f32, a2 as f32, b2 as f32, 0.0];
            }
        }
    }
    lut
}

// --------------------------------------------------------------- references

fn refl(v: i32, n: i32) -> i32 {
    let mut r = v;
    if r < 0 {
        r = -r;
    }
    if r >= n {
        r = 2 * n - 2 - r;
    }
    r
}

pub fn cpu_demosaic(cfa: &[u16]) -> Vec<Px> {
    let (wi, hi) = (W as i32, H as i32);
    let inv: f32 = 1.0 / 65535.0;
    let f = |x: i32, y: i32| cfa[(refl(y, hi) * wi + refl(x, wi)) as usize] as f32 * inv;
    let mut out = vec![[0.0f32; 4]; W * H];
    par_rows(&mut out, W, |y, row| {
        let y = y as i32;
        for (x, p) in row.iter_mut().enumerate() {
            let x = x as i32;
            let c = f(x, y);
            let (l, r, u, d) = (f(x - 1, y), f(x + 1, y), f(x, y - 1), f(x, y + 1));
            let cross = (l + r + u + d) * 0.25;
            let diag =
                (f(x - 1, y - 1) + f(x + 1, y - 1) + f(x - 1, y + 1) + f(x + 1, y + 1)) * 0.25;
            let horiz = (l + r) * 0.5;
            let vert = (u + d) * 0.5;
            let o = match (y & 1, x & 1) {
                (0, 0) => [c, cross, diag],
                (0, _) => [horiz, c, vert],
                (_, 0) => [vert, c, horiz],
                _ => [diag, cross, c],
            };
            *p = [o[0], o[1], o[2], 1.0];
        }
    });
    out
}

fn add(a: Px, b: Px) -> Px {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2], a[3] + b[3]]
}
fn mul(a: Px, b: Px) -> Px {
    [a[0] * b[0], a[1] * b[1], a[2] * b[2], a[3] * b[3]]
}
fn scale(a: Px, s: f32) -> Px {
    [a[0] * s, a[1] * s, a[2] * s, a[3] * s]
}

/// Horizontal clipped-window mean of (a, b), optionally squaring a for the second output.
fn hbox(a: &[Px], b: Option<&[Px]>) -> (Vec<Px>, Vec<Px>) {
    let wi = W as i32;
    let mut out = vec![[[0.0f32; 4]; 2]; W * H];
    par_rows(&mut out, W, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let x = x as i32;
            let (x0, x1) = ((x - GF_R).max(0), (x + GF_R).min(wi - 1));
            let (mut s, mut s2) = ([0.0f32; 4], [0.0f32; 4]);
            for k in x0..=x1 {
                let i = y * W + k as usize;
                let v = a[i];
                s = add(s, v);
                s2 = add(
                    s2,
                    match b {
                        Some(b) => b[i],
                        None => mul(v, v),
                    },
                );
            }
            let inv = 1.0 / (x1 - x0 + 1) as f32;
            *o = [scale(s, inv), scale(s2, inv)];
        }
    });
    out.into_iter().map(|[p, q]| (p, q)).unzip()
}

fn vbox(a: &[Px], b: &[Px]) -> Vec<[Px; 2]> {
    let hi = H as i32;
    let mut out = vec![[[0.0f32; 4]; 2]; W * H];
    par_rows(&mut out, W, |y, row| {
        let yi = y as i32;
        let (y0, y1) = ((yi - GF_R).max(0), (yi + GF_R).min(hi - 1));
        let inv = 1.0 / (y1 - y0 + 1) as f32;
        for (x, o) in row.iter_mut().enumerate() {
            let (mut s, mut s2) = ([0.0f32; 4], [0.0f32; 4]);
            for k in y0..=y1 {
                let i = k as usize * W + x;
                s = add(s, a[i]);
                s2 = add(s2, b[i]);
            }
            *o = [scale(s, inv), scale(s2, inv)];
        }
    });
    out
}

pub fn cpu_guided(i: &[Px]) -> Vec<Px> {
    let (t1, t2) = hbox(i, None);
    let mv = vbox(&t1, &t2);
    let (a, b): (Vec<Px>, Vec<Px>) = mv
        .into_iter()
        .map(|[m, m2]| {
            let mut a = [0.0f32; 4];
            let mut b = [0.0f32; 4];
            for c in 0..4 {
                let v = (m2[c] - m[c] * m[c]).max(0.0);
                a[c] = v / (v + GF_EPS);
                b[c] = m[c] - a[c] * m[c];
            }
            (a, b)
        })
        .unzip();
    let (t1, t2) = hbox(&a, Some(&b));
    let mv = vbox(&t1, &t2);
    mv.into_iter()
        .zip(i)
        .map(|([ma, mb], iv)| add(mul(ma, *iv), mb))
        .collect()
}

#[allow(clippy::excessive_precision)]
pub fn to_oklab(c: [f32; 3]) -> [f32; 3] {
    let l = 0.4122214708 * c[0] + 0.5363325363 * c[1] + 0.0514459929 * c[2];
    let m = 0.2119034982 * c[0] + 0.6806995451 * c[1] + 0.1073969566 * c[2];
    let s = 0.0883024619 * c[0] + 0.2817188376 * c[1] + 0.6299787005 * c[2];
    let (l_, m_, s_) = (l.cbrt(), m.cbrt(), s.cbrt());
    [
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    ]
}

#[allow(clippy::excessive_precision)]
pub fn from_oklab(c: [f32; 3]) -> [f32; 3] {
    let l_ = c[0] + 0.3963377774 * c[1] + 0.2158037573 * c[2];
    let m_ = c[0] - 0.1055613458 * c[1] - 0.0638541728 * c[2];
    let s_ = c[0] - 0.0894841775 * c[1] - 1.2914855480 * c[2];
    let (l, m, s) = (l_ * l_ * l_, m_ * m_ * m_, s_ * s_ * s_);
    [
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    ]
}

pub fn cpu_lut(src: &[Px], lut: &[Px]) -> Vec<Px> {
    let n = LUT_N;
    let node = |i: usize, j: usize, k: usize| {
        let v = lut[(k * n + j) * n + i];
        [v[0], v[1], v[2]]
    };
    let lerp = |a: [f32; 3], b: [f32; 3], t: f32| {
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    let mut out = vec![[0.0f32; 4]; W * H];
    par_rows(&mut out, W, |y, row| {
        for (x, o) in row.iter_mut().enumerate() {
            let p = src[y * W + x];
            let lab = to_oklab([p[0], p[1], p[2]]);
            let t = [lab[0], lab[1] + 0.5, lab[2] + 0.5].map(|v| v.clamp(0.0, 1.0) * 32.0);
            let i0 = t.map(|v| (v.floor() as usize).min(31));
            let fr = [
                t[0] - i0[0] as f32,
                t[1] - i0[1] as f32,
                t[2] - i0[2] as f32,
            ];
            let (a, b, c) = (i0[0], i0[1], i0[2]);
            let c00 = lerp(node(a, b, c), node(a + 1, b, c), fr[0]);
            let c10 = lerp(node(a, b + 1, c), node(a + 1, b + 1, c), fr[0]);
            let c01 = lerp(node(a, b, c + 1), node(a + 1, b, c + 1), fr[0]);
            let c11 = lerp(node(a, b + 1, c + 1), node(a + 1, b + 1, c + 1), fr[0]);
            let c0 = lerp(c00, c10, fr[1]);
            let c1 = lerp(c01, c11, fr[1]);
            let rgb = from_oklab(lerp(c0, c1, fr[2]));
            *o = [rgb[0], rgb[1], rgb[2], p[3]];
        }
    });
    out
}

/// (max abs error, mean abs error) over RGBA channels, in linear units.
pub fn errors(got: &[f32], reference: &[Px]) -> (f64, f64) {
    assert_eq!(got.len(), reference.len() * 4);
    let mut max = 0.0f64;
    let mut sum = 0.0f64;
    for (g, r) in got.chunks_exact(4).zip(reference) {
        for c in 0..4 {
            assert!(
                g[c].is_finite() && r[c].is_finite(),
                "non-finite accuracy input"
            );
            let e = (g[c] as f64 - r[c] as f64).abs();
            max = max.max(e);
            sum += e;
        }
    }
    (max, sum / (reference.len() * 4) as f64)
}
