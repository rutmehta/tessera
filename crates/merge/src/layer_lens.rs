//! Calibrated lens preprocessing for registration and source-to-ideal geometry.
use crate::{LinearImage, Result, layers::LensCorrection};

fn model(c: &LensCorrection) -> lens::BrownConrady {
    lens::BrownConrady {
        k1: c.distortion[0],
        k2: c.distortion[1],
        k3: c.distortion[2],
        ..Default::default()
    }
}

pub(crate) fn validate(c: &LensCorrection) -> Result<()> {
    if c.distortion.iter().any(|v| !v.is_finite() || v.abs() > 1.) {
        return Err("nonfinite or excessive radial distortion calibration".into());
    }
    // Prove strictly increasing observed radius on ideal radius [0,2].
    // The derivative is a cubic in r²; check endpoints and every stationary point.
    let [a, b, d] = [
        3. * c.distortion[0],
        5. * c.distortion[1],
        7. * c.distortion[2],
    ];
    let mut probes = vec![0., 4.];
    if d == 0. {
        if b != 0. {
            probes.push(-a / (2. * b));
        }
    } else {
        let discriminant = 4. * b * b - 12. * d * a;
        if discriminant >= 0. {
            probes.push((-2. * b + discriminant.sqrt()) / (6. * d));
            probes.push((-2. * b - discriminant.sqrt()) / (6. * d));
        }
    }
    if probes
        .into_iter()
        .filter(|t| (0. ..=4.).contains(t))
        .any(|t| 1. + t * (a + t * (b + t * d)) < 0.05)
        || model(c).distort([2., 0.])[0] < 2_f64.sqrt()
    {
        return Err("radial calibration folds or does not cover the source rectangle".into());
    }
    Ok(())
}

pub(crate) fn undistort(p: [f64; 2], im: &LinearImage, c: &LensCorrection) -> Option<[f64; 2]> {
    let p = [
        2. * p[0] / im.width as f64 - 1.,
        2. * p[1] / im.height as f64 - 1.,
    ];
    let q = model(c).undistort(p)?;
    if q[0].hypot(q[1]) > 2. + 1e-9 {
        return None;
    }
    Some([
        (q[0] + 1.) * im.width as f64 / 2.,
        (q[1] + 1.) * im.height as f64 / 2.,
    ])
}

pub(crate) fn rectify(im: &LinearImage, c: &LensCorrection) -> LinearImage {
    let model = model(c);
    let mut out = im.clone();
    for (i, pixel) in out.pixels.iter_mut().enumerate() {
        let q = model.distort([
            2. * (i % im.width) as f64 / im.width as f64 + 1. / im.width as f64 - 1.,
            2. * (i / im.width) as f64 / im.height as f64 + 1. / im.height as f64 - 1.,
        ]);
        // Edge extension is only for the temporary registration view. Final
        // geometry renders original source support, with transparent exterior.
        let x = ((q[0] + 1.) * im.width as f64 / 2. - 0.5).clamp(0., (im.width - 1) as f64);
        let y = ((q[1] + 1.) * im.height as f64 / 2. - 0.5).clamp(0., (im.height - 1) as f64);
        *pixel = im.sample(x, y).unwrap_or([0.; 3]);
    }
    out
}
