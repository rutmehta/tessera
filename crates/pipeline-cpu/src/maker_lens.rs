//! ENG-8: a camera's built-in maker-note lens correction as a calibration
//! sample, so it runs through the same resolved-lens stages (lateral CA,
//! vignetting, composed geometry) on the reference and resident paths.
//!
//! Fujifilm stores piecewise-linear splines over source (uncorrected) radii,
//! in half-diagonal units of the active area (see `raw_decode::FujifilmLens`).
//! The model follows darktable's `_init_coeffs_md_v2` and RawTherapee's
//! `FujiMetadataLensCorrection`: the output radius of source radius r is
//! r / m(r) with m = 1 + distortion/100; red/blue sample at (1 + ca) times the
//! green source radius; vignetting is the relative illumination at r, applied
//! before distortion. The output is scaled so the corrected frame has no
//! empty edges (darktable's autoscale over the frame-boundary radii).
//!
//! The splines are fitted once, deterministically, to the sample's
//! polynomials (radial ratio in 1, r, r², r³, r⁴, r⁶; CA ratio in 1, r², r⁴;
//! illumination in r², r⁴, r⁶). On the X-E2S fixture the fitted geometry is
//! within 0.5 px of the spline model at full resolution
//! (`tests/maker_lens.rs`).
use lens::{BrownConrady, CalibrationSample};
use raw_decode::{FujifilmLens, MakerLens, RawMetadata};

/// The built-in correction of `m`, if it carries one this engine applies.
pub(crate) fn sample(m: &RawMetadata) -> Option<CalibrationSample> {
    let [_, _, w, h] = m.default_crop;
    if w == 0 || h == 0 {
        return None;
    }
    match m.maker_lens.as_ref()? {
        MakerLens::Fujifilm(f) => fujifilm(f, f64::from(w), f64::from(h)),
    }
}

/// Linear interpolation with clamped ends (darktable's
/// `_interpolate_linear_spline`).
fn spline(x: &[f64], y: &[f64], at: f64) -> f64 {
    if at <= x[0] {
        return y[0];
    }
    for i in 1..x.len() {
        if at <= x[i] {
            return y[i - 1] + (at - x[i - 1]) * (y[i] - y[i - 1]) / (x[i] - x[i - 1]);
        }
    }
    y[x.len() - 1]
}

/// Samples of the fit domain [0, 1] (half-diagonal units).
const SAMPLES: usize = 1000;

fn fujifilm(f: &FujifilmLens, w: f64, h: f64) -> Option<CalibrationSample> {
    let n = f.knots.len();
    if n < 2
        || [&f.distortion, &f.ca_red, &f.ca_blue, &f.vignetting]
            .iter()
            .any(|v| v.len() != n)
        || !(f.crop_factor.is_finite() && f.crop_factor > 0.)
    {
        return None;
    }
    // Source-radius knots, with an identity knot at the centre when absent.
    let (mut k, mut m, mut red, mut blue, mut vig) = (vec![], vec![], vec![], vec![], vec![]);
    if f.knots[0] > 0. {
        k.push(0.);
        m.push(1.);
        red.push(0.);
        blue.push(0.);
        vig.push(1.);
    }
    for i in 0..n {
        k.push(f.crop_factor * f.knots[i]);
        m.push(1. + f.distortion[i] / 100.);
        red.push(f.ca_red[i]);
        blue.push(f.ca_blue[i]);
        vig.push(f.vignetting[i] / 100.);
    }
    // Output radius of each source knot; the mapping must be invertible.
    let out: Vec<f64> = k.iter().zip(&m).map(|(r, m)| r / m).collect();
    if m.iter().any(|m| !(m.is_finite() && *m > 0.)) || out.windows(2).any(|w| w[1] <= w[0]) {
        return None;
    }
    // Source/output radius ratio at output radius r: the exact inverse of the
    // piecewise-linear source spline (m is linear in the source radius).
    let ratio = |r: f64| -> f64 {
        let last = out.len() - 1;
        if r <= 0. {
            return m[0];
        }
        if r >= out[last] {
            return m[last];
        }
        let i = (1..=last).find(|&i| r <= out[i]).unwrap_or(last);
        let slope = (m[i] - m[i - 1]) / (k[i] - k[i - 1]);
        (m[i - 1] - k[i - 1] * slope) / (1. - r * slope)
    };
    // Source radius of output radius r, for the CA splines (source radii).
    let source = |r: f64| r * ratio(r);
    // darktable autoscale: the largest channel ratio over the radii of the
    // frame boundary, so the corrected frame shows no empty edges.
    let hd = (w / 2.).hypot(h / 2.);
    let srr = (w / 2.).min(h / 2.) / hd;
    let mut z = 0f64;
    for i in 0..200 {
        let x = srr + (1. - srr) * f64::from(i) / 199.;
        let g = ratio(x);
        let rs = source(x);
        z = z
            .max(g)
            .max(g * (1. + spline(&k, &red, rs)))
            .max(g * (1. + spline(&k, &blue, rs)));
    }
    if !(z.is_finite() && (0.5..2.).contains(&z)) {
        return None;
    }
    let rho: Vec<f64> = (0..=SAMPLES).map(|i| i as f64 / SAMPLES as f64).collect();
    // Geometry: source/output ratio at output radius rho (pixel displacement
    // is rho · error; the frame area at rho grows with rho).
    let a = fit(
        &rho,
        |r| ratio(r / z) / z,
        |r| r.powf(1.5),
        &[
            |_| 1.,
            |r| r,
            |r| r * r,
            |r| r.powi(3),
            |r| r.powi(4),
            |r| r.powi(6),
        ],
    )?;
    let ca = |c: &[f64]| {
        let c = fit(
            &rho,
            |r| 1. + spline(&k, c, r),
            |r| r.powf(1.5),
            &[|_| 1., |r| r * r, |r| r.powi(4)],
        )?;
        Some([c[0], c[1], c[2]])
    };
    let v = fit(
        &rho,
        |r| spline(&k, &vig, r) - 1.,
        |r| r.sqrt(),
        &[|r| r * r, |r| r.powi(4), |r| r.powi(6)],
    )?;
    let sample = CalibrationSample {
        distortion: BrownConrady {
            k1: a[2],
            k2: a[4],
            k3: a[5],
            ..Default::default()
        },
        distortion_scale: a[0],
        radial_odd: [a[1], a[3]],
        coordinate_scale: [w / (2. * hd), h / (2. * hd)],
        ca_red: ca(&red)?,
        ca_blue: ca(&blue)?,
        vignette: [v[0], v[1], v[2]],
        ..Default::default()
    };
    lens::Profile {
        model: "maker note".into(),
        samples: vec![sample.clone()],
        ..Default::default()
    }
    .validate()
    .ok()?;
    Some(sample)
}

/// Weighted least squares by Householder QR: coefficients of `basis` that
/// best fit `target` over `x` with per-sample `weight`. Deterministic.
fn fit(
    x: &[f64],
    target: impl Fn(f64) -> f64,
    weight: impl Fn(f64) -> f64,
    basis: &[fn(f64) -> f64],
) -> Option<Vec<f64>> {
    let cols = basis.len();
    let mut a: Vec<Vec<f64>> = basis
        .iter()
        .map(|f| x.iter().map(|&r| weight(r) * f(r)).collect())
        .collect();
    let mut b: Vec<f64> = x.iter().map(|&r| weight(r) * target(r)).collect();
    for j in 0..cols {
        let norm = a[j][j..].iter().map(|v| v * v).sum::<f64>().sqrt();
        if !(norm.is_finite() && norm > 1e-300) {
            return None;
        }
        let alpha = if a[j][j] > 0. { -norm } else { norm };
        let mut v: Vec<f64> = a[j][j..].to_vec();
        v[0] -= alpha;
        let vv = v.iter().map(|x| x * x).sum::<f64>();
        let reflect = |col: &mut [f64]| {
            let s = 2. * v.iter().zip(col.iter()).map(|(a, b)| a * b).sum::<f64>() / vv;
            for (c, v) in col.iter_mut().zip(&v) {
                *c -= s * v;
            }
        };
        for col in a.iter_mut().skip(j) {
            reflect(&mut col[j..]);
        }
        reflect(&mut b[j..]);
    }
    let mut c = vec![0.; cols];
    for j in (0..cols).rev() {
        let s = (j + 1..cols).map(|i| a[i][j] * c[i]).sum::<f64>();
        c[j] = (b[j] - s) / a[j][j];
    }
    c.iter().all(|v| v.is_finite()).then_some(c)
}
