//! Explicit sRGB transfer/primaries for perceptual operators. Not document ICC conversion.
//! Native nonlinear primitives use linear interpolation of 4097 mantissa knots
//! on [1,2], multiplied by a shared exponent table. This avoids backend libm
//! differences without clipping HDR. Tables are generated once on the CPU and
//! uploaded verbatim, not recomputed by shader transcendental instructions.
use std::sync::OnceLock;
pub(crate) const POWER_STRIDE: usize = 4097 + 277;
pub(crate) fn power_tables() -> &'static [f32] {
    static TABLES: OnceLock<Vec<f32>> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut out = Vec::new();
        for p in [2.4_f32, 1.0 / 2.4, 1.0 / 3.0] {
            out.extend((0..=4096).map(|i| (1.0 + i as f32 / 4096.0).powf(p)));
            out.extend((-149..=127).map(|e| (2.0_f64).powf(e as f64 * p as f64) as f32));
        }
        out
    })
}
fn power(v: f32, kind: usize) -> f32 {
    if v == 0.0 || !v.is_finite() {
        return v;
    }
    let mut bits = v.to_bits();
    let mut shift = 0;
    if bits >> 23 == 0 {
        bits = (v * 16777216.0).to_bits();
        shift = 24;
    }
    let e = (bits >> 23) as i32 - 127 - shift;
    let m = f32::from_bits((bits & 0x7fffff) | 0x3f800000);
    let x = (m - 1.0) * 4096.0;
    let i = x as usize;
    let f = x - i as f32;
    let t = &power_tables()[kind * POWER_STRIDE..];
    (t[i] + (t[i + 1] - t[i]) * f) * t[4097 + (e + 149) as usize]
}
fn root(v: f32) -> f32 {
    power(v.abs(), 2).copysign(v)
}
pub(super) fn lab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c.map(decode);
    let f = |v: f32| {
        if v > 216.0 / 24389.0 {
            root(v)
        } else {
            (24389.0 / 27.0 * v + 16.0) / 116.0
        }
    };
    let x = f((0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047);
    let y = f(0.2126729 * r + 0.7151522 * g + 0.0721750 * b);
    let z = f((0.0193339 * r + 0.119_192 * g + 0.9503041 * b) / 1.08883);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}
pub(super) fn from_lab(c: [f32; 3]) -> [f32; 3] {
    let y = (c[0] + 16.0) / 116.0;
    let x = y + c[1] / 500.0;
    let z = y - c[2] / 200.0;
    let f = |v: f32| {
        if v > 6.0 / 29.0 {
            v.powi(3)
        } else {
            (116.0 * v - 16.0) / (24389.0 / 27.0)
        }
    };
    let x = f(x) * 0.95047;
    let y = f(y);
    let z = f(z) * 1.08883;
    [
        3.2404542 * x - 1.5371385 * y - 0.4985314 * z,
        -0.969266 * x + 1.8760108 * y + 0.041556 * z,
        0.0556434 * x - 0.2040259 * y + 1.0572252 * z,
    ]
    .map(encode)
}

pub(super) fn decode(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        power((v + 0.055) / 1.055, 0)
    }
}
pub(super) fn encode(v: f32) -> f32 {
    if v <= 0.0031308 {
        12.92 * v
    } else {
        1.055 * power(v, 1) - 0.055
    }
}
pub(super) fn oklab(c: [f32; 3]) -> [f32; 3] {
    let [r, g, b] = c.map(decode);
    let l = root(0.41222146 * r + 0.53633255 * g + 0.051445995 * b);
    let m = root(0.2119035 * r + 0.6806995 * g + 0.10739696 * b);
    let s = root(0.08830246 * r + 0.28171885 * g + 0.6299787 * b);
    [
        0.21045426 * l + 0.7936178 * m - 0.004072047 * s,
        1.9779985 * l - 2.4285922 * m + 0.4505937 * s,
        0.025904037 * l + 0.78277177 * m - 0.80867577 * s,
    ]
}
pub(super) fn from_oklab(c: [f32; 3]) -> [f32; 3] {
    let [l, a, b] = c;
    let ll = (l + 0.39633778 * a + 0.21580376 * b).powi(3);
    let m = (l - 0.105561346 * a - 0.06385417 * b).powi(3);
    let s = (l - 0.08948418 * a - 1.2914855 * b).powi(3);
    [
        4.0767417 * ll - 3.3077116 * m + 0.23096994 * s,
        -1.268438 * ll + 2.6097574 * m - 0.3413194 * s,
        -0.0041960863 * ll - 0.7034186 * m + 1.7076147 * s,
    ]
    .map(encode)
}
